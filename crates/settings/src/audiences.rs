//! Per-item audiences (`Kind::Audiences`, e.g. `requests.types.audience`): `{item: audience}` where
//! an audience has the shape campaigns and announcements use (`procera_members::audience::Audience`:
//! `all`, `departments`, `groups`, `roles`, `locations`). An item missing from the map is open to
//! everyone. This checks the shape only; the HTTP layer confirms departments and groups exist.

use std::collections::BTreeSet;

use serde_json::{Map, Value, json};

const MAX_IDS: usize = 200;
const MAX_LEN: usize = 100;

fn ids(o: &Map<String, Value>, field: &str) -> Result<Vec<String>, String> {
    let Some(v) = o.get(field) else { return Ok(vec![]) };
    let items = v.as_array().ok_or_else(|| format!("{field} must be a list of ids"))?;
    if items.len() > MAX_IDS {
        return Err(format!("{field}: at most {MAX_IDS} items"));
    }
    let mut set = BTreeSet::new();
    for i in items {
        let id = i
            .as_str()
            .and_then(|s| uuid::Uuid::parse_str(s).ok())
            .ok_or_else(|| format!("{field} must be a list of ids"))?;
        set.insert(id.to_string());
    }
    Ok(set.into_iter().collect())
}

fn words(o: &Map<String, Value>, field: &str) -> Result<Vec<String>, String> {
    let Some(v) = o.get(field) else { return Ok(vec![]) };
    let items = v.as_array().ok_or_else(|| format!("{field} must be a list"))?;
    if items.len() > MAX_IDS {
        return Err(format!("{field}: at most {MAX_IDS} items"));
    }
    let mut set = BTreeSet::new();
    for i in items {
        let s = i
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty() && s.chars().count() <= MAX_LEN && !s.chars().any(char::is_control))
            .ok_or_else(|| format!("{field} items must be text up to {MAX_LEN} characters"))?;
        set.insert(s.to_owned());
    }
    Ok(set.into_iter().collect())
}

/// One audience, normalised (sorted, de-duplicated; `all` drops the other lists).
fn audience(item: &str, v: &Value) -> Result<Value, String> {
    let o = v.as_object().ok_or_else(|| format!("{item}: expected an audience object"))?;
    if let Some(k) =
        o.keys().find(|k| !matches!(k.as_str(), "all" | "departments" | "groups" | "roles" | "locations"))
    {
        return Err(format!("{item}: unknown audience field {k}"));
    }
    let all = match o.get("all") {
        None => false,
        Some(b) => b.as_bool().ok_or_else(|| format!("{item}: all must be true or false"))?,
    };
    if all {
        return Ok(json!({ "all": true }));
    }
    let (departments, groups) = (ids(o, "departments")?, ids(o, "groups")?);
    let (roles, locations) = (words(o, "roles")?, words(o, "locations")?);
    if departments.is_empty() && groups.is_empty() && roles.is_empty() && locations.is_empty() {
        return Err(format!("{item}: choose everyone, or at least one department, group or role"));
    }
    let mut out = json!({ "all": false, "departments": departments, "groups": groups, "roles": roles });
    if !locations.is_empty() {
        out["locations"] = json!(locations);
    }
    Ok(out)
}

/// Validates and normalises `{item: audience}`; every key must be one of `items`.
pub fn validate(items: &[&str], v: &Value) -> Result<Value, String> {
    let o = v.as_object().ok_or("expected an object of audiences")?;
    let mut out = Map::new();
    for (k, a) in o {
        if !items.contains(&k.as_str()) {
            return Err(format!("unknown item {k}; expected one of {}", items.join(", ")));
        }
        out.insert(k.clone(), audience(k, a)?);
    }
    Ok(Value::Object(out))
}

/// Department and group ids named anywhere in a valid value (for existence checks).
pub fn referenced(v: &Value) -> (Vec<uuid::Uuid>, Vec<uuid::Uuid>) {
    let pick = |field: &str| -> Vec<uuid::Uuid> {
        let set: BTreeSet<uuid::Uuid> = v
            .as_object()
            .into_iter()
            .flat_map(|o| o.values())
            .filter_map(|a| a.get(field).and_then(Value::as_array))
            .flatten()
            .filter_map(|i| i.as_str().and_then(|s| uuid::Uuid::parse_str(s).ok()))
            .collect();
        set.into_iter().collect()
    };
    (pick("departments"), pick("groups"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ITEMS: &[&str] = &["a.one", "b.two"];

    #[test]
    fn audiences_normalise_and_reject_bad_shapes() {
        let d = "0190f000-0000-7000-8000-000000000001";
        let v = validate(
            ITEMS,
            &json!({"a.one": {"roles": ["x", "x", " y "], "departments": [d, d]}, "b.two": {"all": true, "roles": ["z"]}}),
        );
        assert_eq!(
            v.ok(),
            Some(
                json!({"a.one": {"all": false, "departments": [d], "groups": [], "roles": ["x", "y"]}, "b.two": {"all": true}})
            )
        );
        assert!(validate(ITEMS, &json!({"c.three": {"all": true}})).is_err(), "unknown item");
        assert!(validate(ITEMS, &json!({"a.one": {}})).is_err(), "empty audience");
        assert!(validate(ITEMS, &json!({"a.one": {"all": false, "departments": []}})).is_err());
        assert!(validate(ITEMS, &json!({"a.one": {"groups": ["nope"]}})).is_err(), "not an id");
        assert!(validate(ITEMS, &json!({"a.one": {"people": []}})).is_err(), "unknown field");
        assert!(validate(ITEMS, &json!([])).is_err());
        assert_eq!(validate(ITEMS, &json!({})).ok(), Some(json!({})));
        let (deps, groups) = referenced(&json!({"a.one": {"departments": [d]}}));
        assert_eq!((deps.len(), groups.len()), (1, 0));
    }
}
