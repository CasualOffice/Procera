//! Leave types, policies and balances (BPMS-223, docs/25 §2.2, §3). A balance is never stored: it
//! is the sum of the person's ledger for a type and leave year. Procera writes the system entries
//! (grants, monthly accruals, carry-over, lapses) on demand and idempotently, each with a `period`
//! key, so reading a balance twice, or on two servers at once, never grants twice.

use procera_db::WsTx;
use procera_members::audience::Audience;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::{Date, Month, OffsetDateTime};
use uuid::Uuid;

use crate::{Hr, HrError, R, dates, name_of};

pub const COLOURS: &[&str] = &["gray", "brown", "orange", "yellow", "green", "blue", "purple", "pink", "red"];

/// Days (multiples of 0.5) to half-days.
pub fn halves(days: f64, what: &str) -> R<i32> {
    let h = (days * 2.0).round();
    if (h - days * 2.0).abs() > 1e-9 || !(-730.0..=730.0).contains(&h) {
        return Err(HrError::Invalid(format!("{what} must be whole or half days, up to 365.")));
    }
    Ok(h as i32)
}

pub fn days(halves: i32) -> f64 {
    f64::from(halves) / 2.0
}

fn first_of(y: i32, m: u8) -> Date {
    let month = Month::try_from(m).unwrap_or(Month::January);
    Date::from_calendar_date(y, month, 1).unwrap_or(Date::MIN)
}

/// The first day of the month `n` months after `d`'s month.
pub fn add_months(d: Date, n: i32) -> Date {
    let total = d.year() * 12 + i32::from(u8::from(d.month())) - 1 + n;
    first_of(total.div_euclid(12), (total.rem_euclid(12) + 1) as u8)
}

/// Whole months from the month of `a` to the month of `b`.
fn months_between(a: Date, b: Date) -> i32 {
    (b.year() - a.year()) * 12 + i32::from(u8::from(b.month())) - i32::from(u8::from(a.month()))
}

/// The first day of the leave year containing `d` for a year starting in `start_month`.
pub fn year_start(d: Date, start_month: i16) -> Date {
    let sm = start_month.clamp(1, 12) as u8;
    let y = if u8::from(d.month()) >= sm { d.year() } else { d.year() - 1 };
    first_of(y, sm)
}

/// Half-days a rule grants by month `k` (1-12) of a year: cumulative and floored, so twelve months
/// add up to exactly the yearly amount.
pub fn accrued_by(yearly_halves: i32, k: i32) -> i32 {
    yearly_halves * k.clamp(0, 12) / 12
}

#[derive(Debug, Clone, Serialize)]
pub struct LeaveType {
    pub id: Uuid,
    pub key: String,
    pub name: String,
    pub colour: String,
    pub paid: bool,
    pub limited: bool,
    pub sensitive: bool,
    pub half_days: bool,
    pub needs_attachment_after_days: Option<i16>,
    pub min_notice_days: i16,
    pub max_consecutive_days: Option<i16>,
    pub negative_limit_days: f64,
    pub counts_non_working_days: bool,
    pub optional_holidays_only: bool,
    pub archived: bool,
    pub revision: i32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeaveTypeInput {
    #[serde(default)]
    pub key: Option<String>,
    pub name: String,
    #[serde(default)]
    pub colour: Option<String>,
    #[serde(default = "yes")]
    pub paid: bool,
    #[serde(default = "yes")]
    pub limited: bool,
    #[serde(default)]
    pub sensitive: bool,
    #[serde(default = "yes")]
    pub half_days: bool,
    #[serde(default)]
    pub needs_attachment_after_days: Option<i16>,
    #[serde(default)]
    pub min_notice_days: i16,
    #[serde(default)]
    pub max_consecutive_days: Option<i16>,
    #[serde(default)]
    pub negative_limit_days: f64,
    #[serde(default)]
    pub counts_non_working_days: bool,
    #[serde(default)]
    pub optional_holidays_only: bool,
    #[serde(default)]
    pub archived: Option<bool>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub leave_type_id: Uuid,
    pub yearly_days: f64,
    #[serde(default = "upfront")]
    pub accrual: String,
    #[serde(default = "yes")]
    pub prorate: bool,
    #[serde(default)]
    pub probation_waits: bool,
    #[serde(default)]
    pub carry_over_max_days: f64,
    #[serde(default)]
    pub carry_over_expiry_months: Option<i16>,
}

fn upfront() -> String {
    "upfront".into()
}

#[derive(Debug, Clone, Serialize)]
pub struct Policy {
    pub id: Uuid,
    pub name: String,
    pub audience: Value,
    pub priority: i32,
    pub year_start_month: i16,
    pub rules: Vec<Rule>,
    pub members: i64,
    pub archived: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    pub revision: i32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyInput {
    pub name: String,
    pub audience: Value,
    #[serde(default)]
    pub priority: Option<i32>,
    #[serde(default = "january")]
    pub year_start_month: i16,
    #[serde(default)]
    pub rules: Vec<Rule>,
    #[serde(default)]
    pub archived: Option<bool>,
}

fn january() -> i16 {
    1
}

/// One ledger entry as people see it.
#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    pub id: Uuid,
    pub kind: String,
    pub days: f64,
    pub reason: Option<String>,
    pub by: Option<String>,
    pub request_id: Option<Uuid>,
    #[serde(with = "time::serde::rfc3339")]
    pub at: OffsetDateTime,
}

/// A person's balance for one type in one leave year, with the entries that make it.
#[derive(Debug, Clone, Serialize)]
pub struct Balance {
    pub leave_type: Value,
    #[serde(with = "dates::iso")]
    pub year_start: Date,
    #[serde(with = "dates::iso")]
    pub year_end: Date,
    /// The policy's yearly amount (`None` when the policy has no rule for the type).
    pub yearly_days: Option<f64>,
    pub accrual: Option<String>,
    pub granted: f64,
    pub carried: f64,
    pub taken: f64,
    pub pending: f64,
    pub adjusted: f64,
    pub expired: f64,
    pub available: f64,
    pub entries: Vec<Entry>,
}

/// Which policy applies to someone, and why.
#[derive(Debug, Clone, Serialize)]
pub struct PolicyOf {
    pub id: Uuid,
    pub name: String,
    /// `assigned` (their HR record names it) or `audience` (the first policy whose audience includes them).
    pub via: &'static str,
    pub year_start_month: i16,
}

const TYPE_COLS: &str = "id, key, name, colour, paid, limited, sensitive, half_days, needs_attachment_after_days, min_notice_days, max_consecutive_days, negative_limit_halves, counts_non_working_days, optional_holidays_only, archived_at IS NOT NULL, revision";

type TypeRow = (
    Uuid,
    String,
    String,
    String,
    bool,
    bool,
    bool,
    bool,
    Option<i16>,
    i16,
    Option<i16>,
    i32,
    bool,
    bool,
    bool,
    i32,
);

fn leave_type(r: TypeRow) -> LeaveType {
    LeaveType {
        id: r.0,
        key: r.1,
        name: r.2,
        colour: r.3,
        paid: r.4,
        limited: r.5,
        sensitive: r.6,
        half_days: r.7,
        needs_attachment_after_days: r.8,
        min_notice_days: r.9,
        max_consecutive_days: r.10,
        negative_limit_days: days(r.11),
        counts_non_working_days: r.12,
        optional_holidays_only: r.13,
        archived: r.14,
        revision: r.15,
    }
}

fn key_from(name: &str) -> String {
    let mut k: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .split('_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    if !k.starts_with(|c: char| c.is_ascii_lowercase()) {
        k = format!("t_{k}");
    }
    k.chars().take(40).collect()
}

struct Member {
    joined: Date,
    probation_end: Option<Date>,
}

impl Hr {
    // ---------------------------------------------------------------- types

    pub async fn leave_types(&self, tx: &mut WsTx<'_>, include_archived: bool) -> R<Vec<LeaveType>> {
        let rows: Vec<TypeRow> = sqlx::query_as(&format!(
            "SELECT {TYPE_COLS} FROM leave_types WHERE $1 OR archived_at IS NULL ORDER BY archived_at NULLS FIRST, created_at"
        ))
        .bind(include_archived)
        .fetch_all(tx.conn())
        .await?;
        Ok(rows.into_iter().map(leave_type).collect())
    }

    pub async fn leave_type(&self, tx: &mut WsTx<'_>, id: Uuid) -> R<LeaveType> {
        let r: TypeRow = sqlx::query_as(&format!("SELECT {TYPE_COLS} FROM leave_types WHERE id = $1"))
            .bind(id)
            .fetch_optional(tx.conn())
            .await?
            .ok_or(HrError::NotFound)?;
        Ok(leave_type(r))
    }

    fn check_type(i: &LeaveTypeInput) -> R<(String, String, i32)> {
        let name = name_of(&i.name, "A leave type")?;
        let colour = i.colour.clone().unwrap_or_else(|| "blue".into());
        if !COLOURS.contains(&colour.as_str()) {
            return Err(HrError::Invalid(format!("Colour is one of {}.", COLOURS.join(", "))));
        }
        if !(0..=365).contains(&i.min_notice_days) {
            return Err(HrError::Invalid("Notice is between 0 and 365 days.".into()));
        }
        if i.max_consecutive_days.is_some_and(|d| !(1..=366).contains(&d))
            || i.needs_attachment_after_days.is_some_and(|d| !(1..=365).contains(&d))
        {
            return Err(HrError::Invalid("Day limits are between 1 and 365.".into()));
        }
        let neg = halves(i.negative_limit_days, "The negative limit")?;
        if neg < 0 {
            return Err(HrError::Invalid(
                "The negative limit is a number of days people may go below zero.".into(),
            ));
        }
        Ok((name, colour, neg))
    }

    pub async fn create_leave_type(&self, tx: &mut WsTx<'_>, i: &LeaveTypeInput) -> R<LeaveType> {
        let (name, colour, neg) = Self::check_type(i)?;
        let key = i.key.clone().unwrap_or_else(|| key_from(&name));
        if !key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') || key.len() < 2 {
            return Err(HrError::Invalid("A key is 2-40 lower-case letters, digits or _.".into()));
        }
        let id = self.ids.new_id();
        sqlx::query(
            "INSERT INTO leave_types (workspace_id, id, key, name, colour, paid, limited, sensitive, half_days, needs_attachment_after_days,
                    min_notice_days, max_consecutive_days, negative_limit_halves, counts_non_working_days, optional_holidays_only, created_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)",
        )
        .bind(tx.ctx.workspace_id)
        .bind(id)
        .bind(&key)
        .bind(&name)
        .bind(&colour)
        .bind(i.paid)
        .bind(i.limited)
        .bind(i.sensitive)
        .bind(i.half_days)
        .bind(i.needs_attachment_after_days)
        .bind(i.min_notice_days)
        .bind(i.max_consecutive_days)
        .bind(neg)
        .bind(i.counts_non_working_days)
        .bind(i.optional_holidays_only)
        .bind(self.clock.now())
        .execute(tx.conn())
        .await
        .map_err(|e| match HrError::from(e) {
            HrError::Conflict(_) => HrError::Conflict("A leave type with that name or key already exists.".into()),
            o => o,
        })?;
        self.leave_type(tx, id).await
    }

    pub async fn update_leave_type(
        &self,
        tx: &mut WsTx<'_>,
        id: Uuid,
        rev: Option<i32>,
        i: &LeaveTypeInput,
    ) -> R<LeaveType> {
        let before = self.leave_type(tx, id).await?;
        if rev.is_some_and(|r| r != before.revision) {
            return Err(HrError::Conflict("stale".into()));
        }
        let (name, colour, neg) = Self::check_type(i)?;
        let archived = i.archived.unwrap_or(before.archived);
        sqlx::query(
            "UPDATE leave_types SET name = $2, colour = $3, paid = $4, limited = $5, sensitive = $6, half_days = $7,
                    needs_attachment_after_days = $8, min_notice_days = $9, max_consecutive_days = $10, negative_limit_halves = $11,
                    counts_non_working_days = $12, optional_holidays_only = $13,
                    archived_at = CASE WHEN $14 THEN coalesce(archived_at, $15) END, revision = revision + 1
              WHERE id = $1",
        )
        .bind(id)
        .bind(&name)
        .bind(&colour)
        .bind(i.paid)
        .bind(i.limited)
        .bind(i.sensitive)
        .bind(i.half_days)
        .bind(i.needs_attachment_after_days)
        .bind(i.min_notice_days)
        .bind(i.max_consecutive_days)
        .bind(neg)
        .bind(i.counts_non_working_days)
        .bind(i.optional_holidays_only)
        .bind(archived)
        .bind(self.clock.now())
        .execute(tx.conn())
        .await
        .map_err(|e| match HrError::from(e) {
            HrError::Conflict(_) => HrError::Conflict("A leave type with that name already exists.".into()),
            o => o,
        })?;
        self.leave_type(tx, id).await
    }

    // ---------------------------------------------------------------- policies

    pub async fn policies(&self, tx: &mut WsTx<'_>, include_archived: bool) -> R<Vec<Policy>> {
        let rows: Vec<(Uuid, String, Value, i32, i16, bool, OffsetDateTime, i32, i64)> = sqlx::query_as(
            "SELECT p.id, p.name, p.audience, p.priority, p.year_start_month, p.archived_at IS NOT NULL, p.created_at, p.revision,
                    (SELECT count(*) FROM memberships m WHERE m.status = 'active' AND procera_leave_policy_of(m.id) = p.id)
               FROM leave_policies p WHERE $1 OR p.archived_at IS NULL ORDER BY p.archived_at NULLS FIRST, p.priority, p.created_at",
        )
        .bind(include_archived)
        .fetch_all(tx.conn())
        .await?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let rules = self.rules(tx, r.0).await?;
            out.push(Policy {
                id: r.0,
                name: r.1,
                audience: r.2,
                priority: r.3,
                year_start_month: r.4,
                rules,
                members: r.8,
                archived: r.5,
                created_at: r.6,
                revision: r.7,
            });
        }
        Ok(out)
    }

    pub async fn policy(&self, tx: &mut WsTx<'_>, id: Uuid) -> R<Policy> {
        self.policies(tx, true).await?.into_iter().find(|p| p.id == id).ok_or(HrError::NotFound)
    }

    async fn rules(&self, tx: &mut WsTx<'_>, policy: Uuid) -> R<Vec<Rule>> {
        let rows: Vec<(Uuid, i32, String, bool, bool, i32, Option<i16>)> = sqlx::query_as(
            "SELECT r.leave_type_id, r.yearly_halves, r.accrual, r.prorate, r.probation_waits, r.carry_over_max_halves, r.carry_over_expiry_months
               FROM leave_policy_rules r JOIN leave_types t ON t.workspace_id = r.workspace_id AND t.id = r.leave_type_id
              WHERE r.policy_id = $1 ORDER BY t.created_at",
        )
        .bind(policy)
        .fetch_all(tx.conn())
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| Rule {
                leave_type_id: r.0,
                yearly_days: days(r.1),
                accrual: r.2,
                prorate: r.3,
                probation_waits: r.4,
                carry_over_max_days: days(r.5),
                carry_over_expiry_months: r.6,
            })
            .collect())
    }

    async fn check_policy(&self, tx: &mut WsTx<'_>, i: &PolicyInput) -> R<(String, Vec<(Rule, i32, i32)>)> {
        let name = name_of(&i.name, "A leave policy")?;
        let aud: Audience = serde_json::from_value(i.audience.clone())
            .map_err(|_| HrError::Invalid("The audience isn't valid.".into()))?;
        if aud.is_empty() {
            return Err(HrError::Invalid("Choose who the policy applies to.".into()));
        }
        if !(1..=12).contains(&i.year_start_month) {
            return Err(HrError::Invalid("The leave year starts in a month from 1 to 12.".into()));
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut out = Vec::new();
        for r in &i.rules {
            if !seen.insert(r.leave_type_id) {
                return Err(HrError::Invalid("Each leave type can appear once in a policy.".into()));
            }
            let t = self.leave_type(tx, r.leave_type_id).await.map_err(|e| match e {
                HrError::NotFound => HrError::Invalid("A rule names a leave type that doesn't exist.".into()),
                o => o,
            })?;
            if t.archived {
                return Err(HrError::Invalid(format!("{} is archived.", t.name)));
            }
            if r.accrual != "upfront" && r.accrual != "monthly" {
                return Err(HrError::Invalid("Accrual is upfront or monthly.".into()));
            }
            if r.carry_over_expiry_months.is_some_and(|m| !(1..=12).contains(&m)) {
                return Err(HrError::Invalid("Carried-over days expire after 1 to 12 months.".into()));
            }
            let yh = halves(r.yearly_days, &format!("{}'s yearly allowance", t.name))?;
            let ch = halves(r.carry_over_max_days, &format!("{}'s carry-over", t.name))?;
            if yh < 0 || ch < 0 {
                return Err(HrError::Invalid("Allowances and carry-over can't be negative.".into()));
            }
            out.push((r.clone(), yh, ch));
        }
        Ok((name, out))
    }

    async fn write_rules(&self, tx: &mut WsTx<'_>, policy: Uuid, rules: &[(Rule, i32, i32)]) -> R<()> {
        sqlx::query("DELETE FROM leave_policy_rules WHERE policy_id = $1")
            .bind(policy)
            .execute(tx.conn())
            .await?;
        for (r, yh, ch) in rules {
            sqlx::query(
                "INSERT INTO leave_policy_rules (workspace_id, policy_id, leave_type_id, yearly_halves, accrual, prorate, probation_waits,
                        carry_over_max_halves, carry_over_expiry_months) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
            )
            .bind(tx.ctx.workspace_id)
            .bind(policy)
            .bind(r.leave_type_id)
            .bind(yh)
            .bind(&r.accrual)
            .bind(r.prorate)
            .bind(r.probation_waits)
            .bind(ch)
            .bind(r.carry_over_expiry_months.filter(|_| *ch > 0))
            .execute(tx.conn())
            .await?;
        }
        Ok(())
    }

    pub async fn create_policy(&self, tx: &mut WsTx<'_>, i: &PolicyInput) -> R<Policy> {
        let (name, rules) = self.check_policy(tx, i).await?;
        let priority = match i.priority {
            Some(p) => p,
            None => sqlx::query_scalar::<_, Option<i32>>("SELECT max(priority) FROM leave_policies")
                .fetch_one(tx.conn())
                .await?
                .map_or(10, |m| m + 10),
        };
        let id = self.ids.new_id();
        sqlx::query(
            "INSERT INTO leave_policies (workspace_id, id, name, audience, priority, year_start_month, created_at) VALUES ($1,$2,$3,$4,$5,$6,$7)",
        )
        .bind(tx.ctx.workspace_id)
        .bind(id)
        .bind(&name)
        .bind(&i.audience)
        .bind(priority)
        .bind(i.year_start_month)
        .bind(self.clock.now())
        .execute(tx.conn())
        .await
        .map_err(|e| match HrError::from(e) {
            HrError::Conflict(_) => HrError::Conflict("A leave policy with that name already exists.".into()),
            o => o,
        })?;
        self.write_rules(tx, id, &rules).await?;
        self.policy(tx, id).await
    }

    /// Changes a policy. New rules apply from the next grant or accrual; entries already in the
    /// ledger stay (adjust them if needed).
    pub async fn update_policy(
        &self,
        tx: &mut WsTx<'_>,
        id: Uuid,
        rev: Option<i32>,
        i: &PolicyInput,
    ) -> R<Policy> {
        let before = self.policy(tx, id).await?;
        if rev.is_some_and(|r| r != before.revision) {
            return Err(HrError::Conflict("stale".into()));
        }
        let (name, rules) = self.check_policy(tx, i).await?;
        sqlx::query(
            "UPDATE leave_policies SET name = $2, audience = $3, priority = $4, year_start_month = $5,
                    archived_at = CASE WHEN $6 THEN coalesce(archived_at, $7) END, revision = revision + 1 WHERE id = $1",
        )
        .bind(id)
        .bind(&name)
        .bind(&i.audience)
        .bind(i.priority.unwrap_or(before.priority))
        .bind(i.year_start_month)
        .bind(i.archived.unwrap_or(before.archived))
        .bind(self.clock.now())
        .execute(tx.conn())
        .await
        .map_err(|e| match HrError::from(e) {
            HrError::Conflict(_) => HrError::Conflict("A leave policy with that name already exists.".into()),
            o => o,
        })?;
        self.write_rules(tx, id, &rules).await?;
        self.policy(tx, id).await
    }

    /// Who a policy applies to now: the count and up to 25 names.
    pub async fn policy_members(&self, tx: &mut WsTx<'_>, id: Uuid) -> R<(i64, Vec<(Uuid, String)>)> {
        self.policy(tx, id).await?;
        let rows: Vec<(Uuid, String)> = sqlx::query_as(
            "SELECT m.id, coalesce(u.display_name, u.email::text) FROM memberships m JOIN users u ON u.id = m.user_id
              WHERE m.status = 'active' AND procera_leave_policy_of(m.id) = $1 ORDER BY lower(coalesce(u.display_name, u.email::text))",
        )
        .bind(id)
        .fetch_all(tx.conn())
        .await?;
        let n = rows.len() as i64;
        Ok((n, rows.into_iter().take(25).collect()))
    }

    /// The policy that applies to a member, and why.
    pub async fn policy_of(&self, tx: &mut WsTx<'_>, membership: Uuid) -> R<Option<PolicyOf>> {
        let row: Option<(Uuid, String, i16, bool)> = sqlx::query_as(
            "SELECT p.id, p.name, p.year_start_month,
                    EXISTS (SELECT 1 FROM membership_hr h WHERE h.membership_id = $1 AND h.leave_policy_id = p.id)
               FROM leave_policies p WHERE p.id = procera_leave_policy_of($1)",
        )
        .bind(membership)
        .fetch_optional(tx.conn())
        .await?;
        Ok(row.map(|r| PolicyOf {
            id: r.0,
            name: r.1,
            year_start_month: r.2,
            via: if r.3 { "assigned" } else { "audience" },
        }))
    }

    /// Assigns a policy to one member, or (`None`) goes back to audiences.
    pub async fn assign_policy(
        &self,
        tx: &mut WsTx<'_>,
        membership: Uuid,
        policy: Option<Uuid>,
    ) -> R<Option<PolicyOf>> {
        self.member_hr(tx, membership).await?;
        if let Some(p) = policy {
            let pol = self.policy(tx, p).await?;
            if pol.archived {
                return Err(HrError::Invalid(format!("{} is archived.", pol.name)));
            }
        }
        sqlx::query(
            "INSERT INTO membership_hr (workspace_id, membership_id, leave_policy_id, updated_at) VALUES ($1, $2, $3, $4)
             ON CONFLICT (workspace_id, membership_id) DO UPDATE SET leave_policy_id = $3, updated_at = $4",
        )
        .bind(tx.ctx.workspace_id)
        .bind(membership)
        .bind(policy)
        .bind(self.clock.now())
        .execute(tx.conn())
        .await?;
        self.policy_of(tx, membership).await
    }

    // ---------------------------------------------------------------- ledger

    async fn member_dates(&self, tx: &mut WsTx<'_>, membership: Uuid) -> R<Member> {
        let (joined, probation_end): (Date, Option<Date>) = sqlx::query_as(
            "SELECT coalesce(h.start_date, m.joined_at::date), h.probation_end
               FROM memberships m LEFT JOIN membership_hr h ON h.workspace_id = m.workspace_id AND h.membership_id = m.id
              WHERE m.id = $1",
        )
        .bind(membership)
        .fetch_optional(tx.conn())
        .await?
        .ok_or(HrError::NotFound)?;
        Ok(Member { joined, probation_end })
    }

    #[allow(clippy::too_many_arguments)]
    async fn system_entry(
        &self,
        tx: &mut WsTx<'_>,
        m: Uuid,
        t: Uuid,
        year: Date,
        kind: &str,
        h: i32,
        period: &str,
    ) -> R<()> {
        if h == 0 {
            return Ok(());
        }
        sqlx::query(
            "INSERT INTO leave_ledger (workspace_id, id, membership_id, leave_type_id, leave_year, kind, halves, period, created_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
             ON CONFLICT (workspace_id, membership_id, leave_type_id, period) WHERE period IS NOT NULL DO NOTHING",
        )
        .bind(tx.ctx.workspace_id)
        .bind(self.ids.new_id())
        .bind(m)
        .bind(t)
        .bind(year)
        .bind(kind)
        .bind(h)
        .bind(period)
        .bind(self.clock.now())
        .execute(tx.conn())
        .await?;
        Ok(())
    }

    async fn sum(&self, tx: &mut WsTx<'_>, m: Uuid, t: Uuid, year: Date, kinds: &[&str]) -> R<i32> {
        let s: Option<i64> = sqlx::query_scalar(
            "SELECT sum(halves) FROM leave_ledger WHERE membership_id = $1 AND leave_type_id = $2 AND leave_year = $3 AND kind = ANY($4)",
        )
        .bind(m)
        .bind(t)
        .bind(year)
        .bind(kinds)
        .fetch_one(tx.conn())
        .await?;
        Ok(s.unwrap_or(0) as i32)
    }

    /// Grants and accruals of one rule for the leave year starting `year`, as of `as_of`. Entries
    /// top the year up to the rule's cumulative target (month by month for monthly accrual), so a
    /// person who moves to another policy mid-year is never granted twice; nothing is taken back
    /// when the target is lower (HR adjusts if needed).
    #[allow(clippy::too_many_arguments)]
    async fn grant_year(
        &self,
        tx: &mut WsTx<'_>,
        m: Uuid,
        rule: &Rule,
        who: &Member,
        year: Date,
        as_of: Date,
        whole: bool,
    ) -> R<()> {
        let yh = halves(rule.yearly_days, "allowance")?;
        let eff = match (rule.probation_waits, who.probation_end) {
            (true, Some(p)) if p > who.joined => p,
            _ => who.joined,
        };
        if eff > as_of || yh == 0 {
            return Ok(());
        }
        let t = rule.leave_type_id;
        let mut have = self.sum(tx, m, t, year, &["grant", "accrual"]).await?;
        if rule.accrual == "monthly" {
            let started = (months_between(year, as_of) + 1).clamp(0, 12);
            let mut target = 0;
            for k in 1..=started {
                let month = add_months(year, k - 1);
                if rule.prorate && month < first_of(eff.year(), u8::from(eff.month())) {
                    continue;
                }
                target += accrued_by(yh, k) - accrued_by(yh, k - 1);
                // Whole-day types only ever grant whole days.
                let due = if whole { target - target % 2 } else { target };
                if due > have {
                    self.system_entry(tx, m, t, year, "accrual", due - have, &format!("accrual:{year}:{k}"))
                        .await?;
                    have = due;
                }
            }
        } else {
            let target = if rule.prorate && eff > year {
                yh * (12 - months_between(year, eff)).clamp(0, 12) / 12
            } else {
                yh
            };
            let target = if whole { target - target % 2 } else { target };
            if target > have {
                self.system_entry(tx, m, t, year, "grant", target - have, &format!("grant:{year}")).await?;
            }
        }
        Ok(())
    }

    /// Writes whatever system entries are due for a member up to `today`: grants and accruals for
    /// the previous and current leave year (not before the policy existed), then the year-end
    /// carry-over (up to the cap; the rest lapses; a negative balance is carried whole) and the
    /// expiry of carried days (the days carried, less what has been used since; used days come from
    /// carried days first).
    pub async fn ensure(&self, tx: &mut WsTx<'_>, m: Uuid, today: Date) -> R<()> {
        let Some(p) = self.policy_of(tx, m).await? else { return Ok(()) };
        let policy = self.policy(tx, p.id).await?;
        let who = self.member_dates(tx, m).await?;
        let cur = year_start(today, policy.year_start_month);
        let prev = add_months(cur, -12);
        let first = year_start(policy.created_at.date(), policy.year_start_month);
        for rule in &policy.rules {
            let t = self.leave_type(tx, rule.leave_type_id).await?;
            if t.archived || !t.limited {
                continue;
            }
            if prev >= first {
                self.grant_year(tx, m, rule, &who, prev, cur.previous_day().unwrap_or(prev), !t.half_days)
                    .await?;
            }
            self.grant_year(tx, m, rule, &who, cur, today, !t.half_days).await?;
            // Year-end rollover from the previous year.
            let bal = self
                .sum(
                    tx,
                    m,
                    t.id,
                    prev,
                    &[
                        "grant",
                        "accrual",
                        "hold",
                        "release",
                        "taken",
                        "cancelled",
                        "adjustment",
                        "carry_over",
                        "expiry",
                    ],
                )
                .await?;
            let cap = halves(rule.carry_over_max_days, "carry-over")?;
            if bal != 0 {
                let carry = if bal > 0 { bal.min(cap) } else { bal };
                let lapse = if bal > 0 { bal - carry } else { 0 };
                self.system_entry(tx, m, t.id, prev, "expiry", -lapse, &format!("lapse:{prev}")).await?;
                self.system_entry(tx, m, t.id, prev, "carry_over", -carry, &format!("carry_out:{prev}"))
                    .await?;
                self.system_entry(tx, m, t.id, cur, "carry_over", carry, &format!("carry:{cur}")).await?;
            }
            // Carried days expire after the policy's months, less what has been used since.
            if let Some(months) = rule.carry_over_expiry_months
                && today >= add_months(cur, i32::from(months))
            {
                let carried = self.sum(tx, m, t.id, cur, &["carry_over"]).await?.max(0);
                let used =
                    (-self.sum(tx, m, t.id, cur, &["hold", "release", "taken", "cancelled"]).await?).max(0);
                let expire = (carried - used).max(0);
                self.system_entry(tx, m, t.id, cur, "expiry", -expire, &format!("carry_expiry:{cur}"))
                    .await?;
            }
        }
        Ok(())
    }

    /// A member's balances for the leave year containing `on` (written up to date first).
    pub async fn balances(
        &self,
        tx: &mut WsTx<'_>,
        m: Uuid,
        today: Date,
        on: Option<Date>,
    ) -> R<(Option<PolicyOf>, Vec<Balance>)> {
        self.member_dates(tx, m).await?; // 404 for someone outside the workspace
        self.ensure(tx, m, today).await?;
        let pol = self.policy_of(tx, m).await?;
        let ysm = pol.as_ref().map_or(1, |p| p.year_start_month);
        let year = year_start(on.unwrap_or(today), ysm);
        let rules = match &pol {
            Some(p) => self.rules(tx, p.id).await?,
            None => vec![],
        };
        let mut type_ids: Vec<Uuid> = rules.iter().map(|r| r.leave_type_id).collect();
        let extra: Vec<Uuid> = sqlx::query_scalar(
            "SELECT DISTINCT leave_type_id FROM leave_ledger WHERE membership_id = $1 AND leave_year = $2",
        )
        .bind(m)
        .bind(year)
        .fetch_all(tx.conn())
        .await?;
        for t in extra {
            if !type_ids.contains(&t) {
                type_ids.push(t);
            }
        }
        let mut out = Vec::new();
        for tid in type_ids {
            let t = self.leave_type(tx, tid).await?;
            let rule = rules.iter().find(|r| r.leave_type_id == tid);
            if t.archived && rule.is_none() {
                continue;
            }
            let rows: Vec<(Uuid, String, i32, Option<String>, Option<String>, Option<Uuid>, OffsetDateTime)> = sqlx::query_as(
                "SELECT l.id, l.kind, l.halves, l.reason, coalesce(u.display_name, u.email::text), l.request_id, l.created_at
                   FROM leave_ledger l
                   LEFT JOIN memberships bm ON bm.workspace_id = l.workspace_id AND bm.id = l.created_by
                   LEFT JOIN users u ON u.id = bm.user_id
                  WHERE l.membership_id = $1 AND l.leave_type_id = $2 AND l.leave_year = $3 ORDER BY l.created_at, l.id",
            )
            .bind(m)
            .bind(tid)
            .bind(year)
            .fetch_all(tx.conn())
            .await?;
            let total = |kinds: &[&str]| {
                rows.iter().filter(|r| kinds.contains(&r.1.as_str())).map(|r| r.2).sum::<i32>()
            };
            let available = rows.iter().map(|r| r.2).sum::<i32>();
            out.push(Balance {
                leave_type: serde_json::json!({ "id": t.id, "key": t.key, "name": t.name, "colour": t.colour, "limited": t.limited, "sensitive": t.sensitive, "paid": t.paid }),
                year_start: year,
                year_end: add_months(year, 12).previous_day().unwrap_or(year),
                yearly_days: rule.map(|r| r.yearly_days),
                accrual: rule.map(|r| r.accrual.clone()),
                granted: days(total(&["grant", "accrual"])),
                carried: days(total(&["carry_over"])),
                taken: days(-total(&["taken", "cancelled"])),
                pending: days(-total(&["hold", "release"])),
                adjusted: days(total(&["adjustment"])),
                expired: days(-total(&["expiry"])),
                available: days(available),
                entries: rows
                    .into_iter()
                    .map(|r| Entry { id: r.0, kind: r.1, days: days(r.2), reason: r.3, by: r.4, request_id: r.5, at: r.6 })
                    .collect(),
            });
        }
        Ok((pol, out))
    }

    /// An admin's correction: `days` (positive or negative, whole or half) with a reason, in the
    /// current leave year.
    #[allow(clippy::too_many_arguments)]
    pub async fn adjust(
        &self,
        tx: &mut WsTx<'_>,
        m: Uuid,
        leave_type: Uuid,
        d: f64,
        reason: &str,
        by: Uuid,
        today: Date,
    ) -> R<Entry> {
        let h = halves(d, "An adjustment")?;
        if h == 0 {
            return Err(HrError::Invalid("Adjust by at least half a day.".into()));
        }
        let reason = reason.trim();
        if reason.is_empty() || reason.chars().count() > 500 {
            return Err(HrError::Invalid("Say why (up to 500 characters).".into()));
        }
        let t = self.leave_type(tx, leave_type).await?;
        if !t.limited {
            return Err(HrError::Invalid(format!("{} has no balance to adjust.", t.name)));
        }
        self.member_dates(tx, m).await?;
        self.ensure(tx, m, today).await?;
        let ysm = self.policy_of(tx, m).await?.map_or(1, |p| p.year_start_month);
        let id = self.ids.new_id();
        let at = self.clock.now();
        sqlx::query(
            "INSERT INTO leave_ledger (workspace_id, id, membership_id, leave_type_id, leave_year, kind, halves, reason, created_by, created_at)
             VALUES ($1,$2,$3,$4,$5,'adjustment',$6,$7,$8,$9)",
        )
        .bind(tx.ctx.workspace_id)
        .bind(id)
        .bind(m)
        .bind(leave_type)
        .bind(year_start(today, ysm))
        .bind(h)
        .bind(reason)
        .bind(by)
        .bind(at)
        .execute(tx.conn())
        .await?;
        Ok(Entry {
            id,
            kind: "adjustment".into(),
            days: days(h),
            reason: Some(reason.into()),
            by: None,
            request_id: None,
            at,
        })
    }

    /// Everyone's available days by type for the current leave year (HR's balances grid).
    pub async fn all_balances(&self, tx: &mut WsTx<'_>, today: Date) -> R<Vec<Value>> {
        let people: Vec<(Uuid, String, String)> = sqlx::query_as(
            "SELECT m.id, coalesce(u.display_name, u.email::text), u.email::text FROM memberships m JOIN users u ON u.id = m.user_id
              WHERE m.status = 'active' ORDER BY lower(coalesce(u.display_name, u.email::text)) LIMIT 2000",
        )
        .fetch_all(tx.conn())
        .await?;
        let mut out = Vec::with_capacity(people.len());
        for (id, name, email) in people {
            let (pol, bals) = self.balances(tx, id, today, None).await?;
            let by_type: serde_json::Map<String, Value> = bals
                .iter()
                .filter_map(|b| {
                    let id = b.leave_type.get("id")?.as_str()?.to_string();
                    Some((id, serde_json::json!({ "available": b.available, "taken": b.taken, "pending": b.pending })))
                })
                .collect();
            out.push(serde_json::json!({ "id": id, "name": name, "email": email, "policy": pol, "balances": by_type }));
        }
        Ok(out)
    }

    /// Common leave types and a standard policy for everyone, for a workspace that has none.
    pub async fn set_up_defaults(&self, tx: &mut WsTx<'_>) -> R<(Vec<LeaveType>, Policy)> {
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM leave_types").fetch_one(tx.conn()).await?;
        if n > 0 {
            return Err(HrError::Conflict("This workspace already has leave types.".into()));
        }
        let ty = |key: &str, name: &str, colour: &str| LeaveTypeInput {
            key: Some(key.into()),
            name: name.into(),
            colour: Some(colour.into()),
            paid: true,
            limited: true,
            sensitive: false,
            half_days: true,
            needs_attachment_after_days: None,
            min_notice_days: 0,
            max_consecutive_days: None,
            negative_limit_days: 0.0,
            counts_non_working_days: false,
            optional_holidays_only: false,
            archived: None,
        };
        let annual = self
            .create_leave_type(
                tx,
                &LeaveTypeInput { min_notice_days: 3, ..ty("annual", "Annual leave", "blue") },
            )
            .await?;
        let sick = self
            .create_leave_type(
                tx,
                &LeaveTypeInput {
                    sensitive: true,
                    needs_attachment_after_days: Some(3),
                    ..ty("sick", "Sick leave", "pink")
                },
            )
            .await?;
        let casual = self
            .create_leave_type(
                tx,
                &LeaveTypeInput { max_consecutive_days: Some(3), ..ty("casual", "Casual leave", "green") },
            )
            .await?;
        let optional = self
            .create_leave_type(
                tx,
                &LeaveTypeInput {
                    half_days: false,
                    optional_holidays_only: true,
                    ..ty("optional_holiday", "Optional holiday", "purple")
                },
            )
            .await?;
        let unpaid = self
            .create_leave_type(
                tx,
                &LeaveTypeInput { paid: false, limited: false, ..ty("unpaid", "Unpaid leave", "gray") },
            )
            .await?;
        let rule = |t: &LeaveType, d: f64, accrual: &str, carry: f64, expiry: Option<i16>| Rule {
            leave_type_id: t.id,
            yearly_days: d,
            accrual: accrual.into(),
            prorate: true,
            probation_waits: false,
            carry_over_max_days: carry,
            carry_over_expiry_months: expiry,
        };
        let policy = self
            .create_policy(
                tx,
                &PolicyInput {
                    name: "Standard".into(),
                    audience: serde_json::json!({ "all": true, "departments": [], "groups": [], "roles": [] }),
                    priority: Some(100),
                    year_start_month: 1,
                    rules: vec![
                        rule(&annual, 18.0, "monthly", 5.0, Some(3)),
                        rule(&sick, 12.0, "upfront", 0.0, None),
                        rule(&casual, 6.0, "upfront", 0.0, None),
                        rule(&optional, 2.0, "upfront", 0.0, None),
                        Rule { yearly_days: 0.0, ..rule(&unpaid, 0.0, "upfront", 0.0, None) },
                    ],
                    archived: None,
                },
            )
            .await?;
        Ok((vec![annual, sick, casual, optional, unpaid], policy))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use time::macros::date;

    #[test]
    fn leave_years() {
        assert_eq!(year_start(date!(2026 - 03 - 15), 1), date!(2026 - 01 - 01));
        assert_eq!(year_start(date!(2026 - 03 - 15), 4), date!(2025 - 04 - 01));
        assert_eq!(year_start(date!(2026 - 04 - 01), 4), date!(2026 - 04 - 01));
        assert_eq!(add_months(date!(2025 - 11 - 01), 3), date!(2026 - 02 - 01));
        assert_eq!(add_months(date!(2026 - 01 - 01), -12), date!(2025 - 01 - 01));
        assert_eq!(months_between(date!(2025 - 04 - 01), date!(2026 - 03 - 31)), 11);
    }

    #[test]
    fn halves_are_exact() {
        assert_eq!(halves(1.5, "x").unwrap(), 3);
        assert_eq!(halves(-2.0, "x").unwrap(), -4);
        assert!(halves(1.25, "x").is_err());
        assert!(halves(400.0, "x").is_err());
    }

    #[test]
    fn keys_from_names() {
        assert_eq!(key_from("Annual leave"), "annual_leave");
        assert_eq!(key_from("2nd Saturday off!"), "t_2nd_saturday_off");
    }

    proptest! {
        /// Monthly accrual never goes backwards and adds up to exactly the yearly amount.
        #[test]
        fn monthly_accrual_sums_to_the_year(yh in 0i32..730) {
            let mut total = 0;
            for k in 1..=12 {
                let h = accrued_by(yh, k) - accrued_by(yh, k - 1);
                prop_assert!(h >= 0);
                total += h;
            }
            prop_assert_eq!(total, yh);
        }
    }
}
