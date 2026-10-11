//! Layered settings (BPMS-027, docs/20 §1): code default → platform floor → profile/workspace value.
//! Department/collection overrides (layer 4) arrive with the directory; process-level values live
//! in release bundles (ADR-0012).

pub mod catalogue;

use std::collections::BTreeSet;
use std::sync::Arc;

use catalogue::{CATALOGUE, Comparator, Def, Kind};
use procera_audit::{ActorKind, AuditEvent, Category, Outcome, append};
use procera_db::{DbError, SYSTEM_WORKSPACE, WsTx};
use procera_kernel::{Clock, IdGen};
use serde::Serialize;
use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("unknown setting {0}")]
    UnknownKey(String),
    #[error("invalid value for {key}: {message}")]
    Invalid { key: String, message: String },
    #[error("{key} is looser than the platform floor")]
    BelowFloor { key: String, floor: Value },
    #[error("{0} is locked by the platform")]
    Locked(String),
    #[error("a reason is required to change {0}")]
    ReasonRequired(String),
    #[error("the setting changed (revision {current})")]
    StaleRevision { current: i64 },
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<DbError> for SettingsError {
    fn from(e: DbError) -> Self {
        Self::Internal(e.to_string())
    }
}
impl From<sqlx::Error> for SettingsError {
    fn from(e: sqlx::Error) -> Self {
        Self::Internal(e.to_string())
    }
}
impl From<procera_audit::AuditError> for SettingsError {
    fn from(e: procera_audit::AuditError) -> Self {
        Self::Internal(e.to_string())
    }
}

fn invalid(d: &Def, message: impl Into<String>) -> SettingsError {
    SettingsError::Invalid { key: d.key.to_owned(), message: message.into() }
}

/// Validates and normalizes a value for `d` (sets are sorted and de-duplicated).
pub fn validate(d: &Def, v: &Value) -> Result<Value, SettingsError> {
    let int = |min: i64, max: i64, unit: &str| -> Result<Value, SettingsError> {
        let n = v.as_i64().ok_or_else(|| invalid(d, format!("expected an integer{unit}")))?;
        if n < min || n > max {
            return Err(invalid(d, format!("must be between {min} and {max}{unit}")));
        }
        Ok(json!(n))
    };
    match d.kind {
        Kind::Bool => v.as_bool().map(Value::Bool).ok_or_else(|| invalid(d, "expected true or false")),
        Kind::Int { min, max } => int(min, max, ""),
        Kind::Duration { min, max } => int(min, max, " seconds"),
        Kind::Size { min, max } => int(min, max, " bytes"),
        Kind::Enum { values } => match v.as_str() {
            Some(s) if values.contains(&s) => Ok(json!(s)),
            _ => Err(invalid(d, format!("expected one of {}", values.join(", ")))),
        },
        Kind::Set { values, allow_all } => {
            let items = v.as_array().ok_or_else(|| invalid(d, "expected a list"))?;
            let mut set = BTreeSet::new();
            for i in items {
                match i.as_str() {
                    Some("*") if allow_all => {
                        return Ok(json!(["*"]));
                    }
                    Some(s) if values.contains(&s) => {
                        set.insert(s.to_owned());
                    }
                    _ => return Err(invalid(d, format!("items must be from {}", values.join(", ")))),
                }
            }
            Ok(json!(set))
        }
        Kind::List { max_items, max_len } => {
            let items = v.as_array().ok_or_else(|| invalid(d, "expected a list"))?;
            if items.len() > max_items {
                return Err(invalid(d, format!("at most {max_items} items")));
            }
            let mut set = BTreeSet::new();
            for i in items {
                let s = i
                    .as_str()
                    .map(|s| s.trim().to_lowercase())
                    .filter(|s| !s.is_empty() && s.len() <= max_len);
                set.insert(s.ok_or_else(|| {
                    invalid(d, format!("items must be non-empty text up to {max_len} characters"))
                })?);
            }
            Ok(json!(set))
        }
        Kind::Text { max_len } => match v.as_str() {
            Some(s) if s.chars().count() <= max_len && !s.chars().any(char::is_control) => {
                Ok(json!(s.trim()))
            }
            _ => Err(invalid(d, format!("expected text up to {max_len} characters"))),
        },
        Kind::Colour => match v.as_str() {
            Some(s)
                if s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit()) =>
            {
                Ok(json!(s.to_uppercase()))
            }
            _ => Err(invalid(d, "expected a colour like #4F46E5")),
        },
        Kind::DurationList { min, max, max_items } => {
            let items = v.as_array().ok_or_else(|| invalid(d, "expected a list of seconds"))?;
            if items.len() > max_items {
                return Err(invalid(d, format!("at most {max_items} items")));
            }
            let mut set = BTreeSet::new();
            for i in items {
                match i.as_i64() {
                    Some(n) if (min..=max).contains(&n) => {
                        set.insert(n);
                    }
                    _ => return Err(invalid(d, format!("items must be between {min} and {max} seconds"))),
                }
            }
            Ok(json!(set))
        }
    }
}

fn str_set(v: &Value) -> BTreeSet<&str> {
    v.as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default()
}

/// `true` when `candidate` is at least as strict as `floor` under `cmp`.
pub fn at_least_as_strict(cmp: Comparator, candidate: &Value, floor: &Value) -> bool {
    match cmp {
        Comparator::Smaller => matches!((candidate.as_i64(), floor.as_i64()), (Some(c), Some(f)) if c <= f),
        Comparator::Larger => matches!((candidate.as_i64(), floor.as_i64()), (Some(c), Some(f)) if c >= f),
        Comparator::True => !floor.as_bool().unwrap_or(false) || candidate.as_bool() == Some(true),
        Comparator::Superset => {
            let (c, f) = (str_set(candidate), str_set(floor));
            c.contains("*") || (!f.contains("*") && f.is_subset(&c))
        }
        Comparator::Subset => str_set(candidate).is_subset(&str_set(floor)),
    }
}

/// One contributing layer in the explainer.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Layer {
    pub layer: &'static str,
    pub value: Value,
}

/// The effective value of a key and how it was reached (docs/20 §1 explainer).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Effective {
    pub key: &'static str,
    /// Catalogue group (docs/20 §2 section), for grouping in the UI.
    pub group: &'static str,
    pub value: Value,
    /// Layer the value came from: `default`, `profile`, `workspace` or `platform_floor` (when clamped).
    pub source: &'static str,
    /// The stored workspace value is looser than a floor raised after it was saved (kept, not applied).
    pub clamped: bool,
    pub layers: Vec<Layer>,
    pub editable_with: Option<&'static str>,
    pub immediate: bool,
    pub step_up: bool,
    pub kind: Kind,
    pub revision: i64,
}

/// Pure resolution: default, optional floor, optional stored workspace value (with its origin).
pub fn resolve(
    d: &'static Def,
    floor: Option<&Value>,
    stored: Option<(&Value, &'static str, i64)>,
) -> Effective {
    let default = (d.default)();
    let mut layers = vec![Layer { layer: "default", value: default.clone() }];
    let floor = floor.filter(|_| d.floor.is_some());
    if let Some(f) = floor {
        layers.push(Layer { layer: "platform_floor", value: f.clone() });
    }
    let (mut value, mut source, revision) = match stored {
        Some((v, origin, rev)) => {
            layers.push(Layer { layer: origin, value: v.clone() });
            (v.clone(), origin, rev)
        }
        None => (default, "default", 0),
    };
    let mut clamped = false;
    if let (Some(cmp), Some(f)) = (d.floor, floor)
        && !at_least_as_strict(cmp, &value, f)
    {
        clamped = source != "default";
        value = f.clone();
        source = "platform_floor";
    }
    Effective {
        key: d.key,
        group: d.group,
        value,
        source,
        clamped,
        layers,
        editable_with: d.perm,
        immediate: d.immediate,
        step_up: d.step_up,
        kind: d.kind,
        revision,
    }
}

/// A platform-plane value or floor.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlatformValue {
    pub key: &'static str,
    pub value: Value,
    pub is_floor: bool,
    pub revision: i64,
    pub comparator: Option<Comparator>,
}

#[derive(Clone)]
pub struct Settings {
    clock: Arc<dyn Clock>,
    ids: Arc<dyn IdGen>,
}

impl std::fmt::Debug for Settings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Settings")
    }
}

pub fn def(key: &str) -> Result<&'static Def, SettingsError> {
    catalogue::get(key).ok_or_else(|| SettingsError::UnknownKey(key.to_owned()))
}

impl Settings {
    pub fn new(clock: Arc<dyn Clock>, ids: Arc<dyn IdGen>) -> Self {
        Self { clock, ids }
    }

    async fn floors(tx: &mut WsTx<'_>) -> Result<Vec<(String, Value)>, SettingsError> {
        Ok(sqlx::query_as("SELECT key, value FROM platform_settings WHERE is_floor")
            .fetch_all(tx.conn())
            .await?)
    }

    #[allow(clippy::too_many_arguments)]
    async fn record(
        &self,
        tx: &mut WsTx<'_>,
        layer: &str,
        key: &str,
        before: Option<&Value>,
        after: &Value,
        reason: Option<&str>,
        author: Option<Uuid>,
    ) -> Result<Uuid, SettingsError> {
        let id = self.ids.new_id();
        sqlx::query("INSERT INTO settings_revisions (workspace_id, id, layer, key, before, after, reason, author_id, created_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)")
            .bind(tx.ctx.workspace_id).bind(id).bind(layer).bind(key).bind(before).bind(after).bind(reason).bind(author).bind(self.clock.now())
            .execute(tx.conn()).await?;
        Ok(id)
    }

    #[allow(clippy::too_many_arguments)]
    async fn audit(
        &self,
        tx: &mut WsTx<'_>,
        actor_subject: Option<Uuid>,
        action: &str,
        revision_id: Uuid,
        diff: Value,
        reason: Option<&str>,
        client: (Option<String>, Option<String>),
    ) -> Result<(), SettingsError> {
        let kind = if actor_subject.is_some() { ActorKind::User } else { ActorKind::System };
        let mut ev = AuditEvent::new(actor_subject, kind, Category::Config, action, Outcome::Allowed)
            .object("settings_revision", revision_id);
        ev.diff = Some(diff);
        ev.reason = reason.map(str::to_owned);
        (ev.ip, ev.user_agent) = client;
        append(tx, self.clock.as_ref(), self.ids.as_ref(), &ev).await?;
        Ok(())
    }

    /// Writes a configuration profile's values into a new workspace (docs/20 §0), in its transaction.
    pub async fn apply_profile(&self, tx: &mut WsTx<'_>, profile: &str) -> Result<(), SettingsError> {
        let values = catalogue::profile(profile).ok_or_else(|| SettingsError::Invalid {
            key: "general.profile".into(),
            message: format!("unknown profile {profile}"),
        })?;
        for (key, v) in values {
            let v = validate(def(key)?, &v)?;
            sqlx::query("INSERT INTO workspace_settings (workspace_id, key, value, updated_at) VALUES ($1, $2, $3, $4) ON CONFLICT (workspace_id, key) DO NOTHING")
                .bind(tx.ctx.workspace_id).bind(key).bind(&v).bind(self.clock.now()).execute(tx.conn()).await?;
            self.record(tx, "profile", key, None, &v, Some(profile), None).await?;
        }
        Ok(())
    }

    /// Every catalogue key resolved for the transaction's workspace.
    pub async fn effective_all(&self, tx: &mut WsTx<'_>) -> Result<Vec<Effective>, SettingsError> {
        let floors = Self::floors(tx).await?;
        let stored: Vec<(String, Value, Option<Uuid>, i64)> =
            sqlx::query_as("SELECT key, value, updated_by, revision FROM workspace_settings")
                .fetch_all(tx.conn())
                .await?;
        Ok(CATALOGUE
            .iter()
            .map(|d| {
                let floor = floors.iter().find(|(k, _)| k == d.key).map(|(_, v)| v);
                let s = stored
                    .iter()
                    .find(|(k, ..)| k == d.key)
                    .map(|(_, v, by, rev)| (v, if by.is_some() { "workspace" } else { "profile" }, *rev));
                resolve(d, floor, s)
            })
            .collect())
    }

    pub async fn effective(&self, tx: &mut WsTx<'_>, key: &str) -> Result<Effective, SettingsError> {
        let d = def(key)?;
        let floor: Option<Value> =
            sqlx::query_scalar("SELECT value FROM platform_settings WHERE key = $1 AND is_floor")
                .bind(key)
                .fetch_optional(tx.conn())
                .await?;
        let stored: Option<(Value, Option<Uuid>, i64)> =
            sqlx::query_as("SELECT value, updated_by, revision FROM workspace_settings WHERE key = $1")
                .bind(key)
                .fetch_optional(tx.conn())
                .await?;
        Ok(resolve(
            d,
            floor.as_ref(),
            stored.as_ref().map(|(v, by, rev)| (v, if by.is_some() { "workspace" } else { "profile" }, *rev)),
        ))
    }

    /// Sets a workspace value. Permission and step-up are checked by the caller; this enforces the
    /// catalogue, the floor (`BelowFloor`), reasons for ⚡ keys and optimistic concurrency.
    #[allow(clippy::too_many_arguments)]
    pub async fn set_workspace(
        &self,
        tx: &mut WsTx<'_>,
        actor: (Uuid, Uuid),
        key: &str,
        value: &Value,
        reason: Option<&str>,
        expected_revision: Option<i64>,
        client: (Option<String>, Option<String>),
    ) -> Result<Effective, SettingsError> {
        let (user_id, subject_ref) = actor;
        let d = def(key)?;
        if d.perm.is_none() {
            return Err(SettingsError::Locked(key.to_owned()));
        }
        let reason = reason.map(str::trim).filter(|r| !r.is_empty());
        if d.immediate && reason.is_none() {
            return Err(SettingsError::ReasonRequired(key.to_owned()));
        }
        let v = validate(d, value)?;
        if let Some(cmp) = d.floor {
            let floor: Option<Value> =
                sqlx::query_scalar("SELECT value FROM platform_settings WHERE key = $1 AND is_floor")
                    .bind(key)
                    .fetch_optional(tx.conn())
                    .await?;
            if let Some(f) = floor
                && !at_least_as_strict(cmp, &v, &f)
            {
                return Err(SettingsError::BelowFloor { key: key.to_owned(), floor: f });
            }
        }
        let before: Option<(Value, i64)> =
            sqlx::query_as("SELECT value, revision FROM workspace_settings WHERE key = $1 FOR UPDATE")
                .bind(key)
                .fetch_optional(tx.conn())
                .await?;
        let current = before.as_ref().map_or(0, |(_, r)| *r);
        if let Some(exp) = expected_revision
            && exp != current
        {
            return Err(SettingsError::StaleRevision { current });
        }
        let before_value = before.map(|(v, _)| v).unwrap_or_else(|| (d.default)());
        sqlx::query(
            "INSERT INTO workspace_settings (workspace_id, key, value, updated_by, updated_at) VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (workspace_id, key) DO UPDATE SET value = EXCLUDED.value, updated_by = EXCLUDED.updated_by, updated_at = EXCLUDED.updated_at, revision = workspace_settings.revision + 1",
        )
        .bind(tx.ctx.workspace_id).bind(key).bind(&v).bind(user_id).bind(self.clock.now()).execute(tx.conn()).await?;
        let rev = self.record(tx, "workspace", key, Some(&before_value), &v, reason, Some(user_id)).await?;
        self.audit(
            tx,
            Some(subject_ref),
            "setting.changed",
            rev,
            json!({"key": key, "before": before_value, "after": v}),
            reason,
            client,
        )
        .await?;
        self.effective(tx, key).await
    }

    /// Platform values and floors (system plane).
    pub async fn platform_values(&self, tx: &mut WsTx<'_>) -> Result<Vec<PlatformValue>, SettingsError> {
        let rows: Vec<(String, Value, bool, i64)> =
            sqlx::query_as("SELECT key, value, is_floor, revision FROM platform_settings ORDER BY key")
                .fetch_all(tx.conn())
                .await?;
        Ok(rows
            .into_iter()
            .filter_map(|(k, value, is_floor, revision)| {
                catalogue::get(&k).map(|d| PlatformValue {
                    key: d.key,
                    value,
                    is_floor,
                    revision,
                    comparator: d.floor,
                })
            })
            .collect())
    }

    /// Sets a platform floor (system plane, system-workspace transaction). Raising a floor never
    /// rewrites workspace values: looser ones resolve as `clamped` (docs/20 §1 rules 4-5).
    #[allow(clippy::too_many_arguments)]
    pub async fn set_floor(
        &self,
        tx: &mut WsTx<'_>,
        actor_subject: Uuid,
        actor_user: Uuid,
        key: &str,
        value: &Value,
        reason: &str,
        client: (Option<String>, Option<String>),
    ) -> Result<PlatformValue, SettingsError> {
        debug_assert_eq!(tx.ctx.workspace_id, SYSTEM_WORKSPACE);
        let d = def(key)?;
        let Some(cmp) = d.floor else {
            return Err(invalid(d, "this setting has no platform floor"));
        };
        if reason.trim().is_empty() {
            return Err(SettingsError::ReasonRequired(key.to_owned()));
        }
        let v = validate(d, value)?;
        let before: Option<Value> =
            sqlx::query_scalar("SELECT value FROM platform_settings WHERE key = $1 FOR UPDATE")
                .bind(key)
                .fetch_optional(tx.conn())
                .await?;
        let revision: i64 = sqlx::query_scalar(
            "INSERT INTO platform_settings (key, value, is_floor, updated_by, updated_at) VALUES ($1, $2, true, $3, $4)
             ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, is_floor = true, updated_by = EXCLUDED.updated_by, updated_at = EXCLUDED.updated_at, revision = platform_settings.revision + 1
             RETURNING revision",
        )
        .bind(key).bind(&v).bind(actor_user).bind(self.clock.now()).fetch_one(tx.conn()).await?;
        let rev = self
            .record(tx, "platform_floor", key, before.as_ref(), &v, Some(reason), Some(actor_user))
            .await?;
        self.audit(
            tx,
            Some(actor_subject),
            "platform.floor_changed",
            rev,
            json!({"key": key, "before": before, "after": v}),
            Some(reason),
            client,
        )
        .await?;
        Ok(PlatformValue { key: d.key, value: v, is_floor: true, revision, comparator: Some(cmp) })
    }

    /// Change history for one key in the transaction's workspace, newest first.
    pub async fn history(&self, tx: &mut WsTx<'_>, key: &str) -> Result<Vec<Revision>, SettingsError> {
        def(key)?;
        let rows: Vec<(Uuid, String, Option<Value>, Value, Option<String>, Option<Uuid>, OffsetDateTime)> = sqlx::query_as(
            "SELECT id, layer, before, after, reason, author_id, created_at FROM settings_revisions WHERE key = $1 ORDER BY created_at DESC, id DESC LIMIT 100",
        ).bind(key).fetch_all(tx.conn()).await?;
        Ok(rows
            .into_iter()
            .map(|(id, layer, before, after, reason, author_id, created_at)| Revision {
                id,
                layer,
                before,
                after,
                reason,
                author_id,
                created_at,
            })
            .collect())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Revision {
    pub id: Uuid,
    pub layer: String,
    pub before: Option<Value>,
    pub after: Value,
    pub reason: Option<String>,
    pub author_id: Option<Uuid>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}
