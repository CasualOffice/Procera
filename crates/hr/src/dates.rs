//! Dates as `YYYY-MM-DD` in JSON (the `time` crate's default is a tuple).

use serde::{Deserialize, Deserializer, Serializer};
use time::{Date, format_description::well_known::Iso8601};

pub fn parse(s: &str) -> Option<Date> {
    Date::parse(s, &Iso8601::DATE).ok().filter(|_| s.len() == 10)
}

pub mod iso {
    use super::*;

    pub fn serialize<S: Serializer>(d: &Date, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&d.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Date, D::Error> {
        let s = String::deserialize(d)?;
        parse(&s).ok_or_else(|| serde::de::Error::custom(format!("{s} isn't a date like 2026-10-02")))
    }
}

pub mod iso_opt {
    use super::*;

    pub fn serialize<S: Serializer>(d: &Option<Date>, s: S) -> Result<S::Ok, S::Error> {
        match d {
            Some(d) => s.serialize_str(&d.to_string()),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Date>, D::Error> {
        match Option::<String>::deserialize(d)? {
            None => Ok(None),
            Some(s) if s.is_empty() => Ok(None),
            Some(s) => parse(&s)
                .map(Some)
                .ok_or_else(|| serde::de::Error::custom(format!("{s} isn't a date like 2026-10-02"))),
        }
    }
}
