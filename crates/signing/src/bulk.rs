//! Bulk sending, reminders and expiry (BPMS-107 slice 3, migration 0051).
//!
//! - **Bulk send**: a bulk draft's placeholder signer stands for each person in an audience (the
//!   shared `procera_members::audience`: everyone, departments, groups, roles, locations). Sending
//!   copies the envelope once per active member (same frozen document, the placeholder's fields,
//!   the same copies-to list) into a batch, in one transaction, and tells each signer. The draft
//!   stays as the batch's template and is never sent itself. At most 2,000 people per send.
//! - **Reminders**: the signer whose turn it is gets a reminder every `reminder_days` after they
//!   were told (at most three); a sender can also remind now (at most once an hour per signer).
//! - **Expiry**: a sent envelope past `expires_at` expires and its sender is told.
//!
//! `run_due` does the scheduled part for every workspace with envelopes out (scheduler and
//! `/__test/drain`); reminders and expiries are audited as the system.

use procera_db::{WsCtx, WsTx};
use procera_members::audience::{self, Audience};
use serde::Serialize;
use serde_json::{Value, json};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::{Actor, Envelope, R, Signing, SigningError, day};

/// Most people one bulk send reaches (one transaction).
pub const MAX_BULK: usize = 2000;
const MAX_REMINDERS: i32 = 3;

/// What a scheduled pass did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Due {
    pub reminded: usize,
    pub expired: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct BatchRow {
    pub envelope_id: Uuid,
    pub recipient_id: Uuid,
    pub membership_id: Uuid,
    pub name: String,
    pub email: String,
    pub department: Option<String>,
    /// The envelope's state (`sent`, `completed`, `declined`, `voided`, `expired`).
    pub state: String,
    #[serde(with = "time::serde::rfc3339::option")]
    pub viewed_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub signed_at: Option<OffsetDateTime>,
    pub decline_reason: Option<String>,
    pub reminders_sent: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Batch {
    pub id: Uuid,
    pub title: String,
    pub template_id: Uuid,
    pub audience: Value,
    pub total: i32,
    pub created_by: Uuid,
    pub sender_name: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub expires_at: Option<OffsetDateTime>,
    pub counts: Value,
    pub rows: Vec<BatchRow>,
}

type BatchRowTuple = (
    Uuid,
    Uuid,
    Uuid,
    String,
    String,
    Option<String>,
    String,
    Option<OffsetDateTime>,
    Option<OffsetDateTime>,
    Option<String>,
    i32,
);

impl Signing {
    /// Sends a bulk draft: one copy per active member of `audience`, grouped in a batch.
    pub async fn send_bulk(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        id: Uuid,
        revision: Option<i32>,
        aud: &Audience,
    ) -> R<Batch> {
        let e = self.draft(tx, actor, id, revision).await?;
        if !e.bulk {
            return Err(SigningError::Invalid(
                "Only a document set up for a group can be sent to one.".into(),
            ));
        }
        if aud.is_empty() {
            return Err(SigningError::Invalid("Choose who receives it.".into()));
        }
        let placeholder = e
            .recipients
            .iter()
            .find(|r| r.role == "signer")
            .ok_or_else(|| SigningError::Internal("no placeholder signer".into()))?;
        if !e.fields.iter().any(|f| f.recipient_id == placeholder.id && f.kind == "signature") {
            return Err(SigningError::Invalid("Place a signature field for each person first.".into()));
        }
        let mut people =
            audience::resolve(tx, aud).await.map_err(|e| SigningError::Internal(e.to_string()))?;
        people.sort();
        people.dedup();
        if people.is_empty() {
            return Err(SigningError::Invalid("Nobody active matches who you chose.".into()));
        }
        if people.len() > MAX_BULK {
            return Err(SigningError::Invalid(format!(
                "One send can reach at most {MAX_BULK} people; this reaches {}.",
                people.len()
            )));
        }
        let now = self.clock.now();
        let lifetime = e.expires_at.map_or(Duration::days(30), |x| x - e.created_at);
        let expires = now + lifetime;
        let batch = self.ids.new_id();
        sqlx::query("INSERT INTO envelope_batches (workspace_id, id, template_id, title, audience, total, created_by, created_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)")
            .bind(tx.ctx.workspace_id).bind(batch).bind(id).bind(&e.title).bind(json!(aud))
            .bind(i32::try_from(people.len()).unwrap_or(i32::MAX)).bind(actor.membership).bind(now)
            .execute(tx.conn()).await?;
        let ccs: Vec<Uuid> =
            e.recipients.iter().filter(|r| r.role == "cc").map(|r| r.membership_id).collect();
        let mine: Vec<&crate::Field> = e.fields.iter().filter(|f| f.recipient_id == placeholder.id).collect();
        let doc_hash = hex_decode(&e.document_hash);
        for person in &people {
            let env = self.ids.new_id();
            sqlx::query(
                "INSERT INTO envelopes (workspace_id, id, title, message, source_file_id, document_hash, pages, order_mode, mode, state, created_by, created_at, sent_at, expires_at, batch_id, reminder_days)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,'sequential',$8,'draft',$9,$10,$10,$11,$12,$13)",
            )
            .bind(tx.ctx.workspace_id).bind(env).bind(&e.title).bind(&e.message).bind(e.source_file_id).bind(&doc_hash)
            .bind(&e.pages).bind(&e.mode).bind(actor.membership).bind(now).bind(expires).bind(batch).bind(e.reminder_days)
            .execute(tx.conn()).await?;
            let signer = self.ids.new_id();
            sqlx::query("INSERT INTO envelope_recipients (workspace_id, envelope_id, id, ord, membership_id, role) VALUES ($1,$2,$3,1,$4,'signer')")
                .bind(tx.ctx.workspace_id).bind(env).bind(signer).bind(person).execute(tx.conn()).await?;
            for (i, cc) in ccs.iter().filter(|c| *c != person).enumerate() {
                sqlx::query("INSERT INTO envelope_recipients (workspace_id, envelope_id, id, ord, membership_id, role) VALUES ($1,$2,$3,$4,$5,'cc')")
                    .bind(tx.ctx.workspace_id).bind(env).bind(self.ids.new_id()).bind(i32::try_from(i + 2).unwrap_or(i32::MAX)).bind(cc)
                    .execute(tx.conn()).await?;
            }
            for f in &mine {
                sqlx::query("INSERT INTO envelope_fields (workspace_id, envelope_id, id, recipient_id, kind, page, x, y, w, h, required, label) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)")
                    .bind(tx.ctx.workspace_id).bind(env).bind(self.ids.new_id()).bind(signer).bind(&f.kind).bind(f.page)
                    .bind(f.x).bind(f.y).bind(f.w).bind(f.h).bind(f.required).bind(&f.label)
                    .execute(tx.conn()).await?;
            }
            // The copy holds the document too, so it can't be deleted while any copy needs it.
            sqlx::query("INSERT INTO file_links (workspace_id, file_id, object_type, object_id, linked_by, linked_at) VALUES ($1, $2, 'envelope', $3, $4, $5) ON CONFLICT DO NOTHING")
                .bind(tx.ctx.workspace_id).bind(e.source_file_id).bind(env).bind(actor.membership).bind(now)
                .execute(tx.conn()).await?;
            // Filled in as a draft (a sent envelope's fields are frozen), then sent.
            sqlx::query("UPDATE envelopes SET state = 'sent' WHERE id = $1")
                .bind(env)
                .execute(tx.conn())
                .await?;
            self.emit(
                tx,
                "envelope.sent.v1",
                env,
                json!({ "envelope_id": env, "state": "sent", "batch_id": batch }),
            )
            .await?;
            let copy = self.get(tx, env).await?;
            self.notify_turn(tx, &copy).await?;
        }
        sqlx::query("UPDATE envelopes SET revision = revision + 1 WHERE id = $1")
            .bind(id)
            .execute(tx.conn())
            .await?;
        self.audit(
            tx,
            actor,
            "envelope.bulk_sent",
            id,
            json!({ "batch_id": batch, "people": people.len(), "audience": aud, "expires_at": expires }),
        )
        .await?;
        self.batch(tx, batch).await
    }

    /// A batch with each copy's progress.
    pub async fn batch(&self, tx: &mut WsTx<'_>, id: Uuid) -> R<Batch> {
        let b: Option<(Uuid, String, Uuid, Value, i32, Uuid, String, OffsetDateTime)> = sqlx::query_as(
            "SELECT b.id, b.title, b.template_id, b.audience, b.total, b.created_by, u.display_name, b.created_at
               FROM envelope_batches b JOIN memberships m ON m.workspace_id = b.workspace_id AND m.id = b.created_by JOIN users u ON u.id = m.user_id
              WHERE b.id = $1",
        )
        .bind(id)
        .fetch_optional(tx.conn())
        .await?;
        let b = b.ok_or(SigningError::NotFound)?;
        let rows: Vec<BatchRowTuple> = sqlx::query_as(
            "SELECT e.id, r.id, r.membership_id, u.display_name, u.email::text, d.name, e.state, r.viewed_at, r.signed_at, r.decline_reason, r.reminders_sent
               FROM envelopes e JOIN envelope_recipients r ON r.workspace_id = e.workspace_id AND r.envelope_id = e.id AND r.role = 'signer'
               JOIN memberships m ON m.workspace_id = r.workspace_id AND m.id = r.membership_id JOIN users u ON u.id = m.user_id
               LEFT JOIN departments d ON d.workspace_id = m.workspace_id AND d.id = m.department_id
              WHERE e.batch_id = $1 ORDER BY u.display_name",
        )
        .bind(id)
        .fetch_all(tx.conn())
        .await?;
        let now = self.clock.now();
        let expires: Option<OffsetDateTime> =
            sqlx::query_scalar("SELECT max(expires_at) FROM envelopes WHERE batch_id = $1")
                .bind(id)
                .fetch_one(tx.conn())
                .await?;
        let rows: Vec<BatchRow> = rows
            .into_iter()
            .map(|x| BatchRow {
                envelope_id: x.0,
                recipient_id: x.1,
                membership_id: x.2,
                name: x.3,
                email: x.4,
                department: x.5,
                state: if x.6 == "sent" && expires.is_some_and(|t| t <= now) {
                    "expired".into()
                } else {
                    x.6
                },
                viewed_at: x.7,
                signed_at: x.8,
                decline_reason: x.9,
                reminders_sent: x.10,
            })
            .collect();
        let count = |s: &str| rows.iter().filter(|r| r.state == s).count();
        let counts = json!({
            "total": rows.len(), "signed": count("completed"), "waiting": count("sent"), "viewed": rows.iter().filter(|r| r.state == "sent" && r.viewed_at.is_some()).count(),
            "declined": count("declined"), "cancelled": count("voided"), "expired": count("expired"),
        });
        Ok(Batch {
            id: b.0,
            title: b.1,
            template_id: b.2,
            audience: b.3,
            total: b.4,
            created_by: b.5,
            sender_name: b.6,
            created_at: b.7,
            expires_at: expires,
            counts,
            rows,
        })
    }

    /// Batches I sent (or every batch), newest first, with their counts but no rows.
    pub async fn batches(&self, tx: &mut WsTx<'_>, sender: Option<Uuid>) -> R<Vec<Batch>> {
        let ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM envelope_batches WHERE $1::uuid IS NULL OR created_by = $1 ORDER BY created_at DESC LIMIT 200")
            .bind(sender)
            .fetch_all(tx.conn())
            .await?;
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            let mut b = self.batch(tx, id).await?;
            b.rows.clear();
            out.push(b);
        }
        Ok(out)
    }

    /// Reminds the signer(s) whose turn it is on a sent envelope, now (`by` sends it as a person).
    async fn remind_one(&self, tx: &mut WsTx<'_>, e: &Envelope, by: Option<&Actor>, audit: bool) -> R<usize> {
        if e.state != "sent" || e.expires_at.is_some_and(|x| x <= self.clock.now()) {
            return Ok(0);
        }
        let now = self.clock.now();
        let mut n = 0;
        for r in
            e.recipients.iter().filter(|r| r.role == "signer" && r.state == "pending" && Self::turn(e, r))
        {
            let (sent, last, told): (i32, Option<OffsetDateTime>, Option<OffsetDateTime>) = sqlx::query_as(
                "SELECT reminders_sent, reminded_at, notified_at FROM envelope_recipients WHERE id = $1",
            )
            .bind(r.id)
            .fetch_one(tx.conn())
            .await?;
            let Some(told) = told else { continue };
            let due = match by {
                // A person reminding: not more than once an hour per signer.
                Some(_) => {
                    last.is_none_or(|t| now - t >= Duration::hours(1)) && now - told >= Duration::minutes(1)
                }
                None => {
                    let Some(days) = e.reminder_days else { continue };
                    sent < MAX_REMINDERS && now - last.unwrap_or(told) >= Duration::days(i64::from(days))
                }
            };
            if !due {
                continue;
            }
            sqlx::query("UPDATE envelope_recipients SET reminders_sent = reminders_sent + 1, reminded_at = $2 WHERE id = $1").bind(r.id).bind(now).execute(tx.conn()).await?;
            let link = format!("/sign/{}", r.id);
            let vars = json!({ "title": e.title, "sender": e.sender_name, "due": e.expires_at.map(day), "link": link });
            self.tell(
                tx,
                r.membership_id,
                Some(&r.email),
                "signature_reminder",
                vars,
                &link,
                e.id,
                &format!("sign:{}:reminder:{}", r.id, sent + 1),
            )
            .await?;
            let diff = json!({ "recipient_id": r.id, "reminder": sent + 1 });
            match by {
                _ if !audit => {}
                Some(a) => self.audit(tx, a, "envelope.reminded", e.id, diff).await?,
                None => self.audit_system(tx, "envelope.reminded", e.id, diff).await?,
            }
            n += 1;
        }
        Ok(n)
    }

    /// `Remind now` on one envelope (its sender). Returns how many were reminded (0 when it is too soon).
    pub async fn remind(&self, tx: &mut WsTx<'_>, actor: &Actor, id: Uuid) -> R<usize> {
        let e = self.get(tx, id).await?;
        if e.created_by != actor.membership {
            return Err(SigningError::NotFound);
        }
        if e.state != "sent" {
            return Err(SigningError::Conflict(
                "Only a document waiting for signatures can be reminded.".into(),
            ));
        }
        self.remind_one(tx, &e, Some(actor), true).await
    }

    /// Reminds everyone still to sign in a batch (its sender).
    pub async fn remind_batch(&self, tx: &mut WsTx<'_>, actor: &Actor, id: Uuid) -> R<usize> {
        let by: Option<Uuid> = sqlx::query_scalar("SELECT created_by FROM envelope_batches WHERE id = $1")
            .bind(id)
            .fetch_optional(tx.conn())
            .await?;
        if by != Some(actor.membership) {
            return Err(SigningError::NotFound);
        }
        let ids: Vec<Uuid> =
            sqlx::query_scalar("SELECT id FROM envelopes WHERE batch_id = $1 AND state = 'sent'")
                .bind(id)
                .fetch_all(tx.conn())
                .await?;
        let mut n = 0;
        for eid in ids {
            let e = self.get(tx, eid).await?;
            n += self.remind_one(tx, &e, Some(actor), false).await?;
        }
        if n > 0 {
            self.audit_batch(tx, actor, "envelope.batch_reminded", id, json!({ "reminded": n })).await?;
        }
        Ok(n)
    }

    /// Cancels every copy of a batch still waiting for a signature.
    pub async fn void_batch(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        id: Uuid,
        reason: &str,
        may_any: bool,
    ) -> R<usize> {
        let by: Option<Uuid> = sqlx::query_scalar("SELECT created_by FROM envelope_batches WHERE id = $1")
            .bind(id)
            .fetch_optional(tx.conn())
            .await?;
        let Some(by) = by else { return Err(SigningError::NotFound) };
        if by != actor.membership && !may_any {
            return Err(SigningError::NotFound);
        }
        let ids: Vec<Uuid> =
            sqlx::query_scalar("SELECT id FROM envelopes WHERE batch_id = $1 AND state = 'sent'")
                .bind(id)
                .fetch_all(tx.conn())
                .await?;
        for eid in &ids {
            self.void_inner(tx, actor, *eid, reason, true, false).await?;
        }
        if !ids.is_empty() {
            self.audit_batch(
                tx,
                actor,
                "envelope.batch_voided",
                id,
                json!({ "reason": reason.trim(), "envelopes": ids }),
            )
            .await?;
        }
        Ok(ids.len())
    }

    /// One audit record for an action on a whole batch.
    async fn audit_batch(
        &self,
        tx: &mut WsTx<'_>,
        actor: &Actor,
        action: &str,
        id: Uuid,
        diff: Value,
    ) -> R<()> {
        let mut ev = procera_audit::AuditEvent::new(
            Some(actor.subject_ref),
            procera_audit::ActorKind::User,
            procera_audit::Category::Content,
            action,
            procera_audit::Outcome::Allowed,
        )
        .object("envelope_batch", id);
        ev.ip = actor.ip.clone();
        ev.user_agent = actor.user_agent.clone();
        ev.diff = Some(diff);
        procera_audit::append(tx, self.clock.as_ref(), self.ids.as_ref(), &ev).await?;
        Ok(())
    }

    /// The scheduled pass: expiries and reminders in every workspace with envelopes out.
    pub async fn run_due(&self) -> Result<Due, String> {
        let workspaces: Vec<Uuid> =
            sqlx::query_scalar("SELECT workspace_id FROM discovery.signing_due_workspaces(500)")
                .fetch_all(self.db.pool())
                .await
                .map_err(|e| e.to_string())?;
        let mut due = Due::default();
        for ws in workspaces {
            let mut tx = self.db.begin_ws(WsCtx { workspace_id: ws }).await.map_err(|e| e.to_string())?;
            let d = self.due_in(&mut tx).await.map_err(|e| e.to_string())?;
            tx.commit().await.map_err(|e| e.to_string())?;
            due.reminded += d.reminded;
            due.expired += d.expired;
        }
        Ok(due)
    }

    async fn due_in(&self, tx: &mut WsTx<'_>) -> R<Due> {
        let now = self.clock.now();
        let mut due = Due::default();
        let expired: Vec<(Uuid, String, Uuid)> = sqlx::query_as(
            "UPDATE envelopes SET state = 'expired', ended_reason = 'It expired before everyone signed.', revision = revision + 1
              WHERE state = 'sent' AND expires_at <= $1 RETURNING id, title, created_by",
        )
        .bind(now)
        .fetch_all(tx.conn())
        .await?;
        for (id, title, by) in expired {
            self.audit_system(tx, "envelope.expired", id, json!({ "expired_at": now })).await?;
            self.emit(tx, "envelope.expired.v1", id, json!({ "envelope_id": id, "state": "expired" }))
                .await?;
            let email: Option<String> = sqlx::query_scalar(
                "SELECT u.email::text FROM memberships m JOIN users u ON u.id = m.user_id WHERE m.id = $1",
            )
            .bind(by)
            .fetch_optional(tx.conn())
            .await?;
            let link = format!("/documents/{id}");
            self.tell(
                tx,
                by,
                email.as_deref(),
                "signature_expired",
                json!({ "title": title }),
                &link,
                id,
                &format!("sign:{id}:expired"),
            )
            .await?;
            due.expired += 1;
        }
        let out: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM envelopes WHERE state = 'sent' AND reminder_days IS NOT NULL LIMIT 5000",
        )
        .fetch_all(tx.conn())
        .await?;
        for id in out {
            let e = self.get(tx, id).await?;
            due.reminded += self.remind_one(tx, &e, None, true).await?;
        }
        Ok(due)
    }
}

fn hex_decode(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).filter_map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}
