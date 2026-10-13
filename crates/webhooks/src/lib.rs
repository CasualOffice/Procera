//! Outgoing webhooks (BPMS-143, docs/13 §2.5, docs/06 §3.8, WH-01..04, SSRF-01..07).
//!
//! - **Endpoints**: an https URL, the event types it wants (or every type), a description and an
//!   on/off switch. Its signing secret (`whsec_…`, shown once) is sealed under the workspace key;
//!   rolling it keeps the previous one valid for 24 hours, during which both signatures are sent.
//! - **Events** are thin (docs/08 §6): the event's id and type, when it happened, the workspace,
//!   and only ids, states and version numbers from the domain event, never form contents. The
//!   receiver reads details through the API with its own token. Catalogue: [`EVENTS`].
//! - **Queueing**: the outbox bus calls [`Webhooks::enqueue`] for every event; matching enabled
//!   endpoints get a delivery row (one per endpoint and event) in the same transaction as the
//!   consumer's inbox record, so nothing is queued twice.
//! - **Delivery**: [`Webhooks::run_due`] (scheduler, `/__test/drain`) leases due deliveries, POSTs
//!   them through the egress guard ([`egress`]) with Standard Webhooks headers (`webhook-id`,
//!   `webhook-timestamp`, `webhook-signature: v1,<base64 HMAC-SHA256 of id.timestamp.body>`), and
//!   records the attempt. 2xx is delivered; anything else retries after 1 min, 5 min, 30 min, 2 h, 6 h
//!   and 12 h (with jitter, honouring `Retry-After` up to an hour), then gives up. `410 Gone` turns
//!   the endpoint off at once; three deliveries given up in a row turn it off and tell its creator.
//!   A delivery can be replayed, and a test event sent on demand.

pub mod egress;

use std::sync::Arc;

use base64::Engine;
use hmac::{Hmac, Mac};
use procera_audit::{ActorKind, AuditEvent, Category, Outcome, append};
use procera_db::{Db, WsCtx, WsTx};
use procera_kernel::{Clock, IdGen};
use procera_keys::Keys;
use procera_notify::Notify;
use serde::Serialize;
use serde_json::{Map, Value, json};
use sha2::Sha256;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

/// Retry delays after a failed attempt, in seconds (about 21 hours in all).
const BACKOFF: &[i64] = &[60, 300, 1800, 7200, 21_600, 43_200];
/// Deliveries given up in a row before an endpoint is turned off.
const MAX_ABANDONED: i32 = 3;
const MAX_PAYLOAD: usize = 20 * 1024;
const ROTATION_OVERLAP: Duration = Duration::hours(24);

/// (domain topic, public type, what it means). Only these reach webhooks.
pub const EVENTS: &[(&str, &str, &str)] = &[
    ("instance.started.v1", "request.started", "Someone started a request."),
    ("instance.completed.v1", "request.completed", "A request finished (approved, rejected or done)."),
    ("sla.breached.v1", "request.overdue", "A request step went past its deadline."),
    ("artifact.published.v1", "document.published", "An SOP or policy version was published."),
    ("artifact.review_overdue.v1", "document.review_overdue", "An SOP or policy is past its review date."),
    ("announcement.published.v1", "announcement.published", "An announcement went out."),
    ("campaign.recipient.completed.v1", "acknowledgement.completed", "Someone acknowledged a document."),
    ("membership.activated.v1", "member.joined", "Someone joined the workspace."),
    ("membership.deactivated.v1", "member.deactivated", "Someone was deactivated."),
    ("membership.reactivated.v1", "member.reactivated", "Someone was reactivated."),
    ("envelope.sent.v1", "signature.requested", "A document was sent for signature."),
    ("envelope.completed.v1", "signature.completed", "Everyone signed a document."),
    ("envelope.declined.v1", "signature.declined", "Someone declined to sign."),
    ("envelope.voided.v1", "signature.cancelled", "A document sent for signature was cancelled."),
    ("envelope.expired.v1", "signature.expired", "A document expired before everyone signed."),
];
pub const TEST_EVENT: &str = "webhook.test";

/// The public type for a domain topic.
pub fn public_type(topic: &str) -> Option<&'static str> {
    EVENTS.iter().find(|e| e.0 == topic).map(|e| e.1)
}

fn known_type(t: &str) -> bool {
    t == "*" || EVENTS.iter().any(|e| e.1 == t)
}

/// Only identifiers, states, outcomes and version numbers leave Procera.
fn thin(payload: &Value) -> Value {
    let mut out = Map::new();
    if let Value::Object(m) = payload {
        for (k, v) in m {
            let keep = k == "id"
                || k.ends_with("_id")
                || k.ends_with("_ids")
                || matches!(
                    k.as_str(),
                    "status"
                        | "state"
                        | "outcome"
                        | "result"
                        | "display_id"
                        | "version_no"
                        | "semver"
                        | "kind"
                        | "node_id"
                        | "type"
                );
            let scalar = matches!(v, Value::String(_) | Value::Number(_) | Value::Bool(_) | Value::Null)
                || matches!(v, Value::Array(a) if a.iter().all(|x| matches!(x, Value::String(_))));
            if keep && scalar {
                out.insert(k.clone(), v.clone());
            }
        }
    }
    Value::Object(out)
}

#[derive(Debug, thiserror::Error)]
pub enum WebhookError {
    #[error("not found")]
    NotFound,
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Internal(String),
}

type R<T> = Result<T, WebhookError>;

fn internal(e: impl std::fmt::Display) -> WebhookError {
    WebhookError::Internal(e.to_string())
}

impl From<sqlx::Error> for WebhookError {
    fn from(e: sqlx::Error) -> Self {
        internal(e)
    }
}

/// Who acts, for the audit record.
#[derive(Debug, Clone)]
pub struct Actor {
    pub membership: Uuid,
    pub subject_ref: Uuid,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Endpoint {
    pub id: Uuid,
    pub url: String,
    pub description: Option<String>,
    pub events: Vec<String>,
    pub enabled: bool,
    pub disabled_reason: Option<String>,
    pub created_by: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    pub revision: i32,
    /// Ends the previous secret's grace period after a roll.
    #[serde(with = "time::serde::rfc3339::option")]
    pub previous_secret_until: Option<OffsetDateTime>,
    pub last_delivery: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Delivery {
    pub id: Uuid,
    pub endpoint_id: Uuid,
    pub event_id: Uuid,
    pub event_type: String,
    pub state: String,
    pub attempts: i32,
    #[serde(with = "time::serde::rfc3339::option")]
    pub next_attempt_at: Option<OffsetDateTime>,
    pub last_status: Option<i32>,
    pub last_error: Option<String>,
    pub last_duration_ms: Option<i32>,
    pub response_excerpt: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub delivered_at: Option<OffsetDateTime>,
}

/// What a delivery pass did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Pass {
    pub delivered: usize,
    pub failed: usize,
}

#[derive(Clone)]
pub struct Webhooks {
    db: Db,
    keys: Keys,
    notify: Notify,
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdGen>,
    policy: egress::Policy,
}

impl std::fmt::Debug for Webhooks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Webhooks")
    }
}

/// `whsec_` + base64 of 24 random bytes (the Standard Webhooks format).
fn new_secret() -> (String, Vec<u8>) {
    let raw = procera_crypto::random_bytes::<24>().to_vec();
    (format!("whsec_{}", base64::engine::general_purpose::STANDARD.encode(&raw)), raw)
}

/// `v1,<base64 HMAC-SHA256(secret, "{id}.{timestamp}.{body}")>`.
pub fn signature(secret: &[u8], id: &str, timestamp: i64, body: &[u8]) -> String {
    // HMAC takes a key of any length, so this never fails.
    let Ok(mut m) = <Hmac<Sha256> as Mac>::new_from_slice(secret) else { return String::new() };
    m.update(id.as_bytes());
    m.update(b".");
    m.update(timestamp.to_string().as_bytes());
    m.update(b".");
    m.update(body);
    format!("v1,{}", base64::engine::general_purpose::STANDARD.encode(m.finalize().into_bytes()))
}

type EndpointRow = (
    Uuid,
    String,
    Option<String>,
    Vec<String>,
    bool,
    Option<String>,
    Uuid,
    OffsetDateTime,
    i32,
    Option<OffsetDateTime>,
);
const ENDPOINT_COLS: &str = "id, url, description, events, enabled, disabled_reason, created_by, created_at, revision, secret_prev_until";
type DeliveryRow = (
    Uuid,
    Uuid,
    Uuid,
    String,
    String,
    i32,
    Option<OffsetDateTime>,
    Option<i32>,
    Option<String>,
    Option<i32>,
    Option<String>,
    OffsetDateTime,
    Option<OffsetDateTime>,
);
const DELIVERY_COLS: &str = "id, endpoint_id, event_id, event_type, state, attempts, next_attempt_at, last_status, last_error, last_duration_ms, response_excerpt, created_at, delivered_at";

fn delivery(r: DeliveryRow) -> Delivery {
    Delivery {
        id: r.0,
        endpoint_id: r.1,
        event_id: r.2,
        event_type: r.3,
        state: r.4,
        attempts: r.5,
        next_attempt_at: r.6,
        last_status: r.7,
        last_error: r.8,
        last_duration_ms: r.9,
        response_excerpt: r.10,
        created_at: r.11,
        delivered_at: r.12,
    }
}

/// RFC 3339, as every time in the API is written.
fn iso(t: OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Rfc3339).unwrap_or_default()
}

impl Webhooks {
    pub fn new(db: Db, keys: Keys, notify: Notify, clock: Arc<dyn Clock>, ids: Arc<dyn IdGen>) -> Self {
        Self { db, keys, notify, clock, ids, policy: egress::Policy::from_env() }
    }

    async fn audit(
        &self,
        tx: &mut WsTx<'_>,
        actor: Option<&Actor>,
        action: &str,
        id: Uuid,
        diff: Value,
    ) -> R<()> {
        let mut ev = AuditEvent::new(
            actor.map(|a| a.subject_ref),
            if actor.is_some() { ActorKind::User } else { ActorKind::System },
            Category::Config,
            action,
            Outcome::Allowed,
        )
        .object("webhook_endpoint", id);
        ev.ip = actor.and_then(|a| a.ip.clone());
        ev.user_agent = actor.and_then(|a| a.user_agent.clone());
        ev.diff = Some(diff);
        append(tx, self.clock.as_ref(), self.ids.as_ref(), &ev).await.map_err(internal)?;
        Ok(())
    }

    fn check_events(events: &[String]) -> R<Vec<String>> {
        let mut e: Vec<String> =
            events.iter().map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect();
        e.sort();
        e.dedup();
        if e.is_empty() {
            return Err(WebhookError::Invalid("Choose at least one event (or every event).".into()));
        }
        if let Some(bad) = e.iter().find(|t| !known_type(t)) {
            return Err(WebhookError::Invalid(format!("“{bad}” isn't an event type.")));
        }
        if e.iter().any(|t| t == "*") {
            return Ok(vec!["*".into()]);
        }
        Ok(e)
    }

    /// Refuses URLs the egress guard would refuse (checked again at every delivery).
    async fn check_url(&self, url: &str) -> R<String> {
        if url.trim().chars().count() > 2000 {
            return Err(WebhookError::Invalid("The URL is too long.".into()));
        }
        let t = egress::parse(url).map_err(|e| WebhookError::Invalid(e.to_string()))?;
        match egress::resolve(&t, &self.policy).await {
            Ok(_) | Err(egress::EgressError::Failed(_)) => Ok(t.url.to_string()), // a name that doesn't resolve yet is allowed
            Err(egress::EgressError::Refused(m)) => Err(WebhookError::Invalid(m)),
        }
    }

    async fn endpoint_row(&self, tx: &mut WsTx<'_>, id: Uuid) -> R<Endpoint> {
        let r: EndpointRow =
            sqlx::query_as(&format!("SELECT {ENDPOINT_COLS} FROM webhook_endpoints WHERE id = $1"))
                .bind(id)
                .fetch_optional(tx.conn())
                .await?
                .ok_or(WebhookError::NotFound)?;
        let last: Option<(String, Option<i32>, OffsetDateTime)> = sqlx::query_as(
            "SELECT state, last_status, created_at FROM webhook_deliveries WHERE endpoint_id = $1 AND attempts > 0 ORDER BY created_at DESC LIMIT 1",
        )
        .bind(id)
        .fetch_optional(tx.conn())
        .await?;
        Ok(Endpoint {
            id: r.0,
            url: r.1,
            description: r.2,
            events: r.3,
            enabled: r.4,
            disabled_reason: r.5,
            created_by: r.6,
            created_at: r.7,
            revision: r.8,
            previous_secret_until: r.9.filter(|t| *t > self.clock.now()),
            last_delivery: last
                .map(|(state, status, at)| json!({ "state": state, "status": status, "at": iso(at) })),
        })
    }

    pub async fn list(&self, tx: &mut WsTx<'_>) -> R<Vec<Endpoint>> {
        let ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM webhook_endpoints ORDER BY created_at")
            .fetch_all(tx.conn())
            .await?;
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            out.push(self.endpoint_row(tx, id).await?);
        }
        Ok(out)
    }

    pub async fn get(&self, tx: &mut WsTx<'_>, id: Uuid) -> R<Endpoint> {
        self.endpoint_row(tx, id).await
    }

    /// A new endpoint; returns it with its secret (shown only now).
    pub async fn create(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        url: &str,
        events: &[String],
        description: Option<&str>,
    ) -> R<(Endpoint, String)> {
        let url = self.check_url(url).await?;
        let events = Self::check_events(events)?;
        let description = description.map(str::trim).filter(|d| !d.is_empty());
        if description.is_some_and(|d| d.chars().count() > 200) {
            return Err(WebhookError::Invalid("The description can be at most 200 characters.".into()));
        }
        let n: i64 =
            sqlx::query_scalar("SELECT count(*) FROM webhook_endpoints").fetch_one(tx.conn()).await?;
        if n >= 20 {
            return Err(WebhookError::Invalid("A workspace can have at most 20 webhook endpoints.".into()));
        }
        let id = self.ids.new_id();
        let (shown, raw) = new_secret();
        let sealed = self.keys.seal(tx, &format!("webhook:{id}"), &raw).await.map_err(internal)?;
        sqlx::query("INSERT INTO webhook_endpoints (workspace_id, id, url, description, events, enabled, secret_sealed, created_by, created_at) VALUES ($1,$2,$3,$4,$5,true,$6,$7,$8)")
            .bind(tx.ctx.workspace_id).bind(id).bind(&url).bind(description).bind(&events).bind(sealed).bind(actor.membership).bind(self.clock.now())
            .execute(tx.conn()).await?;
        self.audit(tx, Some(actor), "webhook.created", id, json!({ "url": url, "events": events })).await?;
        Ok((self.endpoint_row(tx, id).await?, shown))
    }

    /// Changes an endpoint (`If-Match` revision). Turning it back on clears why it was turned off.
    #[allow(clippy::too_many_arguments)]
    pub async fn update(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        id: Uuid,
        revision: Option<i32>,
        url: Option<&str>,
        events: Option<&[String]>,
        description: Option<Option<&str>>,
        enabled: Option<bool>,
    ) -> R<Endpoint> {
        let cur = self.endpoint_row(tx, id).await?;
        if revision.is_some_and(|r| r != cur.revision) {
            return Err(WebhookError::Conflict("stale".into()));
        }
        let url = match url {
            Some(u) => self.check_url(u).await?,
            None => cur.url.clone(),
        };
        let events = match events {
            Some(e) => Self::check_events(e)?,
            None => cur.events.clone(),
        };
        let description = match description {
            Some(d) => d.map(str::trim).filter(|d| !d.is_empty()).map(str::to_owned),
            None => cur.description.clone(),
        };
        let enabled = enabled.unwrap_or(cur.enabled);
        sqlx::query("UPDATE webhook_endpoints SET url = $2, events = $3, description = $4, enabled = $5, disabled_reason = CASE WHEN $5 THEN NULL ELSE disabled_reason END, abandoned_streak = CASE WHEN $5 THEN 0 ELSE abandoned_streak END, revision = revision + 1 WHERE id = $1")
            .bind(id).bind(&url).bind(&events).bind(&description).bind(enabled).execute(tx.conn()).await?;
        self.audit(
            tx,
            Some(actor),
            "webhook.updated",
            id,
            json!({ "url": url, "events": events, "enabled": enabled }),
        )
        .await?;
        self.endpoint_row(tx, id).await
    }

    pub async fn delete(&self, tx: &mut WsTx<'_>, actor: &Actor, id: Uuid) -> R<()> {
        let cur = self.endpoint_row(tx, id).await?;
        sqlx::query("DELETE FROM webhook_deliveries WHERE endpoint_id = $1")
            .bind(id)
            .execute(tx.conn())
            .await?;
        sqlx::query("DELETE FROM webhook_endpoints WHERE id = $1").bind(id).execute(tx.conn()).await?;
        self.audit(tx, Some(actor), "webhook.deleted", id, json!({ "url": cur.url })).await
    }

    /// Rolls the secret; the previous one keeps signing for 24 hours. Returns the new secret.
    pub async fn rotate(&self, tx: &mut WsTx<'_>, actor: &Actor, id: Uuid) -> R<String> {
        self.endpoint_row(tx, id).await?;
        let (shown, raw) = new_secret();
        let sealed = self.keys.seal(tx, &format!("webhook:{id}"), &raw).await.map_err(internal)?;
        sqlx::query("UPDATE webhook_endpoints SET secret_prev_sealed = secret_sealed, secret_prev_until = $2, secret_sealed = $3, revision = revision + 1 WHERE id = $1")
            .bind(id).bind(self.clock.now() + ROTATION_OVERLAP).bind(sealed).execute(tx.conn()).await?;
        self.audit(
            tx,
            Some(actor),
            "webhook.secret_rotated",
            id,
            json!({ "previous_valid_until": self.clock.now() + ROTATION_OVERLAP }),
        )
        .await?;
        Ok(shown)
    }

    /// Queues a `webhook.test` event for one endpoint (delivered by the next pass).
    pub async fn test(&self, tx: &mut WsTx<'_>, actor: &Actor, id: Uuid) -> R<Delivery> {
        let ep = self.endpoint_row(tx, id).await?;
        let event = self.ids.new_id();
        let body = json!({ "id": event, "type": TEST_EVENT, "occurred_at": iso(self.clock.now()), "workspace_id": tx.ctx.workspace_id, "data": { "endpoint_id": ep.id } });
        let d = self
            .insert_delivery(tx, ep.id, event, TEST_EVENT, &body)
            .await?
            .ok_or_else(|| internal("duplicate test event"))?;
        self.audit(tx, Some(actor), "webhook.tested", id, json!({ "delivery_id": d })).await?;
        self.delivery(tx, d).await
    }

    async fn insert_delivery(
        &self,
        tx: &mut WsTx<'_>,
        endpoint: Uuid,
        event: Uuid,
        event_type: &str,
        body: &Value,
    ) -> R<Option<Uuid>> {
        let id = self.ids.new_id();
        let n = sqlx::query("INSERT INTO webhook_deliveries (workspace_id, id, endpoint_id, event_id, event_type, payload, state, attempts, next_attempt_at, created_at) VALUES ($1,$2,$3,$4,$5,$6,'pending',0,$7,$7) ON CONFLICT (workspace_id, endpoint_id, event_id) DO NOTHING")
            .bind(tx.ctx.workspace_id).bind(id).bind(endpoint).bind(event).bind(event_type).bind(body).bind(self.clock.now())
            .execute(tx.conn()).await?.rows_affected();
        Ok((n == 1).then_some(id))
    }

    pub async fn delivery(&self, tx: &mut WsTx<'_>, id: Uuid) -> R<Delivery> {
        let r: DeliveryRow =
            sqlx::query_as(&format!("SELECT {DELIVERY_COLS} FROM webhook_deliveries WHERE id = $1"))
                .bind(id)
                .fetch_optional(tx.conn())
                .await?
                .ok_or(WebhookError::NotFound)?;
        Ok(delivery(r))
    }

    /// An endpoint's deliveries, newest first (at most 200), and the event body of each.
    pub async fn deliveries(&self, tx: &mut WsTx<'_>, endpoint: Uuid) -> R<Vec<Value>> {
        self.endpoint_row(tx, endpoint).await?;
        let rows: Vec<(DeliveryRow, Value)> = sqlx::query_as::<_, (Uuid, Uuid, Uuid, String, String, i32, Option<OffsetDateTime>, Option<i32>, Option<String>, Option<i32>, Option<String>, OffsetDateTime, Option<OffsetDateTime>, Value)>(
            &format!("SELECT {DELIVERY_COLS}, payload FROM webhook_deliveries WHERE endpoint_id = $1 ORDER BY created_at DESC LIMIT 200"),
        )
        .bind(endpoint)
        .fetch_all(tx.conn())
        .await?
        .into_iter()
        .map(|r| ((r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7, r.8, r.9, r.10, r.11, r.12), r.13))
        .collect();
        Ok(rows
            .into_iter()
            .map(|(r, p)| {
                let mut v = serde_json::to_value(delivery(r)).unwrap_or_default();
                v["payload"] = p;
                v
            })
            .collect())
    }

    /// Sends a delivery again as soon as possible (a new attempt series; same event id).
    pub async fn replay(&self, tx: &mut WsTx<'_>, actor: &Actor, id: Uuid) -> R<Delivery> {
        let d = self.delivery(tx, id).await?;
        sqlx::query("UPDATE webhook_deliveries SET state = 'pending', attempts = 0, next_attempt_at = $2 WHERE id = $1").bind(id).bind(self.clock.now()).execute(tx.conn()).await?;
        self.audit(
            tx,
            Some(actor),
            "webhook.replayed",
            d.endpoint_id,
            json!({ "delivery_id": id, "event_id": d.event_id }),
        )
        .await?;
        self.delivery(tx, id).await
    }

    /// The bus side: queues the event for every enabled endpoint that wants it. Returns how many.
    pub async fn enqueue(
        &self,
        tx: &mut WsTx<'_>,
        event_id: Uuid,
        topic: &str,
        aggregate_id: Uuid,
        payload: &Value,
        occurred_at: OffsetDateTime,
    ) -> R<usize> {
        let Some(kind) = public_type(topic) else { return Ok(0) };
        let endpoints: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM webhook_endpoints WHERE enabled AND ('*' = ANY(events) OR $1 = ANY(events))",
        )
        .bind(kind)
        .fetch_all(tx.conn())
        .await?;
        if endpoints.is_empty() {
            return Ok(0);
        }
        let mut data = thin(payload);
        let subject = topic.split('.').next().unwrap_or("subject");
        let key = match subject {
            "instance" | "sla" => "request_id",
            "artifact" => "document_id",
            "membership" => "membership_id",
            "envelope" => "envelope_id",
            other => other,
        };
        data.as_object_mut().map(|m| m.entry(key.to_owned()).or_insert(json!(aggregate_id)));
        let body = json!({ "id": event_id, "type": kind, "occurred_at": iso(occurred_at), "workspace_id": tx.ctx.workspace_id, "data": data });
        if serde_json::to_vec(&body).map_or(0, |b| b.len()) > MAX_PAYLOAD {
            tracing::warn!(topic, "webhook payload over 20 KB; skipped");
            return Ok(0);
        }
        let mut n = 0;
        for ep in endpoints {
            n += usize::from(self.insert_delivery(tx, ep, event_id, kind, &body).await?.is_some());
        }
        Ok(n)
    }

    /// The scheduled pass: lease due deliveries, send them outside any transaction, record each.
    pub async fn run_due(&self) -> Result<Pass, String> {
        let workspaces: Vec<Uuid> =
            sqlx::query_scalar("SELECT workspace_id FROM discovery.webhook_due_workspaces($1, 200)")
                .bind(self.clock.now())
                .fetch_all(self.db.pool())
                .await
                .map_err(|e| e.to_string())?;
        let mut pass = Pass::default();
        for ws in workspaces {
            let ctx = WsCtx { workspace_id: ws };
            // Lease a batch (pushed two minutes out so a crash retries it, never double-sends fast).
            let mut tx = self.db.begin_ws(ctx).await.map_err(|e| e.to_string())?;
            let due: Vec<(Uuid, Uuid, Uuid, Value, i32)> = sqlx::query_as(
                "UPDATE webhook_deliveries SET next_attempt_at = $1 + interval '2 minutes' WHERE id IN (
                   SELECT d.id FROM webhook_deliveries d JOIN webhook_endpoints e ON e.workspace_id = d.workspace_id AND e.id = d.endpoint_id
                    WHERE d.state = 'pending' AND d.next_attempt_at <= $1 AND e.enabled ORDER BY d.next_attempt_at LIMIT 50 FOR UPDATE OF d SKIP LOCKED)
                 RETURNING id, endpoint_id, event_id, payload, attempts",
            )
            .bind(self.clock.now())
            .fetch_all(tx.conn())
            .await
            .map_err(|e| e.to_string())?;
            tx.commit().await.map_err(|e| e.to_string())?;
            for (id, endpoint, event, payload, attempts) in due {
                match self.attempt(ctx, id, endpoint, event, &payload, attempts).await {
                    Ok(true) => pass.delivered += 1,
                    Ok(false) => pass.failed += 1,
                    Err(e) => {
                        tracing::error!(delivery = %id, error = %e, "webhook attempt could not be recorded");
                        pass.failed += 1;
                    }
                }
            }
        }
        Ok(pass)
    }

    /// One attempt; returns whether it was delivered.
    async fn attempt(
        &self,
        ctx: WsCtx,
        id: Uuid,
        endpoint: Uuid,
        event: Uuid,
        payload: &Value,
        attempts: i32,
    ) -> R<bool> {
        let mut tx = self.db.begin_ws(ctx).await.map_err(internal)?;
        let (url, sealed, prev, prev_until): (String, Vec<u8>, Option<Vec<u8>>, Option<OffsetDateTime>) =
            sqlx::query_as("SELECT url, secret_sealed, secret_prev_sealed, secret_prev_until FROM webhook_endpoints WHERE id = $1")
                .bind(endpoint).fetch_one(tx.conn()).await?;
        let aad = format!("webhook:{endpoint}");
        let mut secrets = vec![self.keys.open(&mut tx, &aad, &sealed).await.map_err(internal)?.to_vec()];
        if let (Some(p), Some(until)) = (prev, prev_until)
            && until > self.clock.now()
        {
            secrets.push(self.keys.open(&mut tx, &aad, &p).await.map_err(internal)?.to_vec());
        }
        tx.commit().await.map_err(internal)?;
        let body = serde_json::to_vec(payload).map_err(internal)?;
        let ts = self.clock.now().unix_timestamp();
        let msg_id = event.to_string();
        let sig = secrets.iter().map(|s| signature(s, &msg_id, ts, &body)).collect::<Vec<_>>().join(" ");
        let headers = vec![
            ("content-type".to_owned(), "application/json".to_owned()),
            ("webhook-id".to_owned(), msg_id.clone()),
            ("webhook-timestamp".to_owned(), ts.to_string()),
            ("webhook-signature".to_owned(), sig),
        ];
        let outcome = match egress::parse(&url) {
            Err(e) => Err(e),
            Ok(t) => match egress::resolve(&t, &self.policy).await {
                Err(e) => Err(e),
                Ok(addr) => egress::post(&t, addr, &headers, body).await,
            },
        };
        let mut tx = self.db.begin_ws(ctx).await.map_err(internal)?;
        let now = self.clock.now();
        let tries = attempts + 1;
        let (ok, status, error, millis, excerpt, retry_after) = match &outcome {
            Ok(r) => (
                (200..300).contains(&r.status),
                Some(i32::from(r.status)),
                None,
                Some(i32::try_from(r.millis).unwrap_or(i32::MAX)),
                Some(r.body.clone()),
                r.retry_after,
            ),
            Err(e) => (false, None, Some(e.to_string()), None, None, None),
        };
        let gone = status == Some(410);
        let refused = matches!(outcome, Err(egress::EgressError::Refused(_)));
        let next =
            if ok || gone || refused || usize::try_from(attempts).unwrap_or(usize::MAX) >= BACKOFF.len() {
                None
            } else {
                let base = BACKOFF[usize::try_from(attempts).unwrap_or(0)];
                let jitter = i64::from(procera_crypto::random_bytes::<1>()[0]) * base / 2550; // up to +10%
                let wait =
                    retry_after.map_or(base + jitter, |r| i64::try_from(r).unwrap_or(3600).clamp(base, 3600));
                Some(now + Duration::seconds(wait))
            };
        let state = if ok {
            "delivered"
        } else if next.is_some() {
            "pending"
        } else {
            "failed"
        };
        sqlx::query("UPDATE webhook_deliveries SET state = $2, attempts = $3, next_attempt_at = $4, last_status = $5, last_error = $6, last_duration_ms = $7, response_excerpt = $8, delivered_at = CASE WHEN $2 = 'delivered' THEN $9 ELSE delivered_at END WHERE id = $1")
            .bind(id).bind(state).bind(tries).bind(next).bind(status).bind(&error).bind(millis).bind(excerpt.map(|e| e.chars().take(500).collect::<String>())).bind(now)
            .execute(tx.conn()).await?;
        if ok {
            sqlx::query("UPDATE webhook_endpoints SET abandoned_streak = 0 WHERE id = $1")
                .bind(endpoint)
                .execute(tx.conn())
                .await?;
        } else if gone || refused {
            let why = if gone {
                "The receiver answered 410 Gone."
            } else {
                "The URL now points inside a private network."
            };
            self.disable(&mut tx, endpoint, why).await?;
        } else if state == "failed" {
            let streak: i32 = sqlx::query_scalar("UPDATE webhook_endpoints SET abandoned_streak = abandoned_streak + 1 WHERE id = $1 RETURNING abandoned_streak")
                .bind(endpoint).fetch_one(tx.conn()).await?;
            if streak >= MAX_ABANDONED {
                self.disable(
                    &mut tx,
                    endpoint,
                    "Three events in a row couldn't be delivered after a day of retries.",
                )
                .await?;
            }
        }
        tx.commit().await.map_err(internal)?;
        Ok(ok)
    }

    /// Turns an endpoint off, records why, and tells whoever created it.
    async fn disable(&self, tx: &mut WsTx<'_>, endpoint: Uuid, why: &str) -> R<()> {
        let row: Option<(Uuid, String)> = sqlx::query_as("UPDATE webhook_endpoints SET enabled = false, disabled_reason = $2, revision = revision + 1 WHERE id = $1 AND enabled RETURNING created_by, url")
            .bind(endpoint).bind(why).fetch_optional(tx.conn()).await?;
        let Some((by, url)) = row else { return Ok(()) };
        self.audit(tx, None, "webhook.disabled", endpoint, json!({ "reason": why })).await?;
        let link = "/admin/webhooks";
        self.notify
            .notify_member(
                tx,
                by,
                "webhook_disabled",
                &json!({ "url": url, "reason": why }),
                Some(link),
                Some(("webhook_endpoint", endpoint)),
                &format!("webhook:{endpoint}:disabled:{}", self.clock.now().unix_timestamp()),
            )
            .await
            .map_err(internal)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_webhooks_signature() {
        // Test vector from the Standard Webhooks spec (secret whsec_MfKQ9r8GKYqrTwjUPD8ILPZIo2LaLaSw).
        let secret =
            base64::engine::general_purpose::STANDARD.decode("MfKQ9r8GKYqrTwjUPD8ILPZIo2LaLaSw").unwrap();
        let s = signature(&secret, "msg_p5jXN8AQM9LWM0D4loKWxJek", 1_614_265_330, br#"{"test": 2432232314}"#);
        assert_eq!(s, "v1,g0hM9SsE+OTPJTGt/tmIKtSyZlE3uFJELVlNIOLJ1OE=");
    }

    #[test]
    fn thin_payloads_keep_only_ids_and_states() {
        let p = json!({ "membership_id": "m1", "status": "approved", "title": "Leave: Ann", "input": { "reason": "secret" }, "version_no": 2, "ids_list_ids": ["a"] });
        assert_eq!(
            thin(&p),
            json!({ "membership_id": "m1", "status": "approved", "version_no": 2, "ids_list_ids": ["a"] })
        );
    }

    #[test]
    fn catalogue() {
        assert_eq!(public_type("instance.completed.v1"), Some("request.completed"));
        assert_eq!(public_type("settings.changed.v1"), None);
        assert!(known_type("*") && known_type("signature.completed") && !known_type("nope"));
    }
}
