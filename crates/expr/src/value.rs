//! Runtime values, typed marshalling from JSON (SP-08 C08) and the temporal text formats.

use crate::types::Type;
use rust_decimal::Decimal;
use std::collections::BTreeMap;
use std::sync::Arc;
use time::format_description::well_known::Rfc3339;
use time::{Date, Duration, Month, OffsetDateTime, UtcOffset};

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Decimal(Decimal),
    /// Strings and enum values.
    String(Arc<str>),
    Date(Date),
    /// Always normalised to UTC.
    DateTime(OffsetDateTime),
    Duration(Duration),
    Money {
        amount: Decimal,
        currency: Arc<str>,
    },
    List(Arc<Vec<Value>>),
    /// Inline records and the built-in principal/group/file/instance shapes.
    Record(Arc<BTreeMap<String, Value>>),
}

impl Value {
    pub fn string(s: impl Into<Arc<str>>) -> Value {
        Value::String(s.into())
    }

    pub fn list(items: Vec<Value>) -> Value {
        Value::List(Arc::new(items))
    }

    pub fn record<I: IntoIterator<Item = (S, Value)>, S: Into<String>>(fields: I) -> Value {
        Value::Record(Arc::new(fields.into_iter().map(|(k, v)| (k.into(), v)).collect()))
    }

    pub fn datetime(t: OffsetDateTime) -> Value {
        Value::DateTime(t.to_offset(UtcOffset::UTC))
    }

    /// Whether this value inhabits `ty`. Bindings are checked with this, which is what makes
    /// "well-typed expressions don't go wrong" hold at run time.
    pub fn conforms(&self, ty: &Type) -> bool {
        self.inhabits(ty, false)
    }

    /// Like [`Value::conforms`], but an integer does not count as a decimal. Every expression the
    /// checker types as `decimal` evaluates to a decimal value; property tests assert this.
    pub fn has_exact_type(&self, ty: &Type) -> bool {
        self.inhabits(ty, true)
    }

    fn inhabits(&self, ty: &Type, exact: bool) -> bool {
        match (self, ty) {
            (Value::Null, t) => t.is_nullable(),
            (v, Type::Optional(t)) => v.inhabits(t, exact),
            (Value::Bool(_), Type::Bool) => true,
            (Value::Int(_), Type::Int) => true,
            (Value::Int(_), Type::Decimal) => !exact,
            (Value::Decimal(_), Type::Decimal) => true,
            (Value::String(_), Type::String) => true,
            (Value::String(s), Type::Enum(vals)) => vals.iter().any(|v| v.as_str() == &**s),
            (Value::Date(_), Type::Date) => true,
            (Value::DateTime(_), Type::DateTime) => true,
            (Value::Duration(_), Type::Duration) => true,
            (Value::Money { currency, .. }, Type::Money(c)) => &**currency == c.as_str(),
            (Value::List(items), Type::List(t)) => items.iter().all(|v| v.inhabits(t, exact)),
            (Value::Record(fields), t) => match t.fields() {
                Some(decl) if !matches!(t, Type::Money(_)) => {
                    fields.keys().all(|k| decl.contains_key(k))
                        && decl
                            .iter()
                            .all(|(k, ft)| fields.get(k).unwrap_or(&Value::Null).inhabits(ft, exact))
                }
                _ => false,
            },
            _ => false,
        }
    }

    /// Build a value of `ty` from JSON. Integers stay integers, decimals and money are exact
    /// (a JSON number is read from its text, never through a float), dates/datetimes/durations
    /// are parsed from their text formats, and records are checked field by field.
    pub fn from_json(ty: &Type, json: &serde_json::Value) -> Result<Value, String> {
        use serde_json::Value as J;
        let mismatch = || format!("expected {ty}, got {}", short(json));
        Ok(match (ty, json) {
            (t, J::Null) if t.is_nullable() => Value::Null,
            (Type::Optional(t), j) => Value::from_json(t, j)?,
            (Type::Bool, J::Bool(b)) => Value::Bool(*b),
            (Type::Int, J::Number(n)) => Value::Int(n.as_i64().ok_or_else(mismatch)?),
            (Type::Decimal, J::Number(n)) => Value::Decimal(decimal_from_json(n).ok_or_else(mismatch)?),
            (Type::Decimal, J::String(s)) => {
                Value::Decimal(Decimal::from_str_exact(s.trim()).map_err(|_| mismatch())?)
            }
            (Type::String, J::String(s)) => Value::string(s.as_str()),
            (Type::Enum(vals), J::String(s)) if vals.contains(s) => Value::string(s.as_str()),
            (Type::Date, J::String(s)) => Value::Date(parse_date(s)?),
            (Type::DateTime, J::String(s)) => Value::DateTime(parse_datetime(s)?),
            (Type::Duration, J::String(s)) => Value::Duration(parse_duration(s)?),
            (Type::Money(cur), J::Object(o)) => {
                let currency = o.get("currency").and_then(J::as_str).ok_or_else(mismatch)?;
                if currency != cur {
                    return Err(format!("expected currency {cur}, got {currency}"));
                }
                let amount = match o.get("amount") {
                    Some(J::Number(n)) => decimal_from_json(n),
                    Some(J::String(s)) => Decimal::from_str_exact(s.trim()).ok(),
                    _ => None,
                }
                .ok_or_else(mismatch)?;
                Value::Money { amount, currency: Arc::from(cur.as_str()) }
            }
            (Type::List(t), J::Array(items)) => {
                Value::list(items.iter().map(|j| Value::from_json(t, j)).collect::<Result<_, _>>()?)
            }
            (t, J::Object(o)) if t.fields().is_some() && !matches!(t, Type::Money(_)) => {
                let decl = t.fields().unwrap_or_default();
                if let Some(extra) = o.keys().find(|k| !decl.contains_key(*k)) {
                    return Err(format!("unknown field `{extra}` for {t}"));
                }
                let mut out = BTreeMap::new();
                for (k, ft) in &decl {
                    let v = Value::from_json(ft, o.get(k).unwrap_or(&J::Null))
                        .map_err(|e| format!("field `{k}`: {e}"))?;
                    out.insert(k.clone(), v);
                }
                Value::Record(Arc::new(out))
            }
            _ => return Err(mismatch()),
        })
    }

    /// JSON form of a value: decimals and money amounts as strings (exact), temporal values
    /// in their text formats.
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::Value as J;
        match self {
            Value::Null => J::Null,
            Value::Bool(b) => J::Bool(*b),
            Value::Int(i) => J::from(*i),
            Value::Decimal(d) => J::String(d.to_string()),
            Value::String(s) => J::String(s.to_string()),
            Value::Date(d) => J::String(format_date(*d)),
            Value::DateTime(t) => J::String(format_datetime(*t)),
            Value::Duration(d) => J::String(format_duration(*d)),
            Value::Money { amount, currency } => {
                serde_json::json!({ "amount": amount.to_string(), "currency": currency.to_string() })
            }
            Value::List(items) => J::Array(items.iter().map(Value::to_json).collect()),
            Value::Record(fields) => {
                J::Object(fields.iter().map(|(k, v)| (k.clone(), v.to_json())).collect())
            }
        }
    }
}

fn short(j: &serde_json::Value) -> &'static str {
    match j {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "a boolean",
        serde_json::Value::Number(_) => "a number",
        serde_json::Value::String(_) => "a string",
        serde_json::Value::Array(_) => "a list",
        serde_json::Value::Object(_) => "an object",
    }
}

fn decimal_from_json(n: &serde_json::Number) -> Option<Decimal> {
    let text = n.to_string();
    Decimal::from_str_exact(&text).ok().or_else(|| Decimal::from_scientific(&text).ok())
}

/// `YYYY-MM-DD`.
pub fn parse_date(s: &str) -> Result<Date, String> {
    let bad = || format!("`{s}` is not a date (YYYY-MM-DD)");
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return Err(bad());
    }
    let num = |r: std::ops::Range<usize>| s.get(r).filter(|t| t.bytes().all(|c| c.is_ascii_digit()));
    let y: i32 = num(0..4).and_then(|t| t.parse().ok()).ok_or_else(bad)?;
    let m: u8 = num(5..7).and_then(|t| t.parse().ok()).ok_or_else(bad)?;
    let d: u8 = num(8..10).and_then(|t| t.parse().ok()).ok_or_else(bad)?;
    let month = Month::try_from(m).map_err(|_| bad())?;
    Date::from_calendar_date(y, month, d).map_err(|_| bad())
}

/// RFC 3339 with an explicit offset (docs/11 §5: datetimes always carry one). Result is UTC.
pub fn parse_datetime(s: &str) -> Result<OffsetDateTime, String> {
    OffsetDateTime::parse(s, &Rfc3339)
        .map(|t| t.to_offset(UtcOffset::UTC))
        .map_err(|_| format!("`{s}` is not an RFC 3339 datetime with an offset"))
}

/// ISO-8601 duration subset of docs/11 §13.9: `[-]P[nW][nD][T[nH][nM][nS]]`. Months and
/// years are rejected because they have no fixed length.
pub fn parse_duration(s: &str) -> Result<Duration, String> {
    let bad = |why: &str| format!("`{s}` is not a duration: {why}");
    let (neg, rest) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s),
    };
    let body = rest.strip_prefix('P').ok_or_else(|| bad("must start with P"))?;
    let mut secs: i64 = 0;
    let mut in_time = false;
    let mut num = String::new();
    let mut parts = 0;
    for ch in body.chars() {
        if ch.is_ascii_digit() {
            num.push(ch);
            continue;
        }
        if ch == 'T' {
            if in_time || !num.is_empty() {
                return Err(bad("misplaced T"));
            }
            in_time = true;
            continue;
        }
        if num.is_empty() {
            return Err(bad("a number must precede each unit"));
        }
        let n: i64 = num.parse().map_err(|_| bad("number too large"))?;
        num.clear();
        let unit: i64 = match (in_time, ch) {
            (false, 'W') => 7 * 86_400,
            (false, 'D') => 86_400,
            (true, 'H') => 3_600,
            (true, 'M') => 60,
            (true, 'S') => 1,
            (false, 'Y' | 'M') => return Err(bad("months and years are not allowed (no fixed length)")),
            _ => return Err(bad("unknown unit")),
        };
        secs = n.checked_mul(unit).and_then(|v| secs.checked_add(v)).ok_or_else(|| bad("too large"))?;
        parts += 1;
    }
    if !num.is_empty() || parts == 0 {
        return Err(bad("incomplete"));
    }
    let d = Duration::seconds(secs);
    Ok(if neg { -d } else { d })
}

pub fn format_date(d: Date) -> String {
    format!("{:04}-{:02}-{:02}", d.year(), u8::from(d.month()), d.day())
}

pub fn format_datetime(t: OffsetDateTime) -> String {
    let t = t.to_offset(UtcOffset::UTC);
    let ms = t.millisecond();
    let frac = if t.nanosecond() == 0 {
        String::new()
    } else if t.nanosecond().is_multiple_of(1_000_000) {
        format!(".{ms:03}")
    } else {
        format!(".{:09}", t.nanosecond())
    };
    format!("{}T{:02}:{:02}:{:02}{frac}Z", format_date(t.date()), t.hour(), t.minute(), t.second())
}

/// Canonical form `[-]P[nD][T[nH][nM][nS]]`, or `PT0S`. Sub-second parts are dropped.
pub fn format_duration(d: Duration) -> String {
    let neg = d.is_negative();
    let total = d.whole_seconds().unsigned_abs();
    let (days, rem) = (total / 86_400, total % 86_400);
    let (h, m, s) = (rem / 3_600, rem % 3_600 / 60, rem % 60);
    let mut out = String::from(if neg { "-P" } else { "P" });
    if days > 0 {
        out.push_str(&format!("{days}D"));
    }
    if h > 0 || m > 0 || s > 0 || days == 0 {
        out.push('T');
        if h > 0 {
            out.push_str(&format!("{h}H"));
        }
        if m > 0 {
            out.push_str(&format!("{m}M"));
        }
        if s > 0 || (h == 0 && m == 0) {
            out.push_str(&format!("{s}S"));
        }
    }
    out
}
