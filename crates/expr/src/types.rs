//! Static types (docs/11 §5) and the variable schema expressions are checked against.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// Expression types. `integer` and `decimal` are distinct; an integer is accepted wherever a
/// decimal is (`Int <: Decimal`), and an enum wherever a string is (`Enum <: String`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Type {
    /// The type of the `null` literal.
    Null,
    /// Element type of `[]`; has no values.
    Never,
    Bool,
    Int,
    Decimal,
    /// `string` and `text`.
    String,
    Date,
    /// An instant (stored UTC, docs/11 §5).
    DateTime,
    Duration,
    /// `money{currency}`: the currency is part of the type, so mixing currencies is a compile
    /// error rather than a runtime surprise.
    Money(String),
    /// `enum{values}`: sorted, unique values. Runtime values are strings.
    Enum(Vec<String>),
    /// Built-in principal: `id`, `manager?`, `department?`, `email?` (docs/11 §13.10).
    Principal,
    /// Built-in group reference: `id`, `name?`.
    Group,
    /// Built-in file reference: `name`, `hash`, `size`, `content_type?`.
    File,
    /// Built-in instance context: `id`, `display_id`, `started_at` (docs/11 §13.10).
    Instance,
    List(Box<Type>),
    /// Inline record (`{"record": {field: type}}`), compared structurally.
    Record(BTreeMap<String, Type>),
    /// A value that may be null. Never nested.
    Optional(Box<Type>),
}

impl Type {
    pub fn list(t: Type) -> Type {
        Type::List(Box::new(t))
    }

    pub fn optional(t: Type) -> Type {
        match t {
            Type::Null | Type::Optional(_) => t,
            other => Type::Optional(Box::new(other)),
        }
    }

    pub fn enumeration<I: IntoIterator<Item = S>, S: Into<String>>(values: I) -> Type {
        let mut v: Vec<String> = values.into_iter().map(Into::into).collect();
        v.sort();
        v.dedup();
        Type::Enum(v)
    }

    pub fn is_nullable(&self) -> bool {
        matches!(self, Type::Null | Type::Optional(_))
    }

    /// The type without its nullability.
    pub fn non_null(&self) -> &Type {
        match self {
            Type::Optional(t) => t,
            t => t,
        }
    }

    pub fn is_numeric(&self) -> bool {
        matches!(self, Type::Int | Type::Decimal)
    }

    pub fn is_stringy(&self) -> bool {
        matches!(self, Type::String | Type::Enum(_))
    }

    /// Fields of record-like types, with their types.
    pub fn fields(&self) -> Option<BTreeMap<String, Type>> {
        let s = |n: &str| n.to_string();
        Some(match self {
            Type::Record(f) => f.clone(),
            Type::Principal => BTreeMap::from([
                (s("id"), Type::String),
                (s("manager"), Type::optional(Type::Principal)),
                (s("department"), Type::optional(Type::String)),
                (s("email"), Type::optional(Type::String)),
            ]),
            Type::Group => {
                BTreeMap::from([(s("id"), Type::String), (s("name"), Type::optional(Type::String))])
            }
            Type::File => BTreeMap::from([
                (s("name"), Type::String),
                (s("hash"), Type::String),
                (s("size"), Type::Int),
                (s("content_type"), Type::optional(Type::String)),
            ]),
            Type::Instance => BTreeMap::from([
                (s("id"), Type::String),
                (s("display_id"), Type::String),
                (s("started_at"), Type::DateTime),
            ]),
            Type::Money(_) => BTreeMap::from([(s("amount"), Type::Decimal), (s("currency"), Type::String)]),
            _ => return None,
        })
    }

    /// `self` can be used where `other` is expected.
    pub fn is_subtype(&self, other: &Type) -> bool {
        use Type::*;
        match (self, other) {
            (a, b) if a == b => true,
            (Never, _) => true,
            (Null, Optional(_)) => true,
            (Optional(a), Optional(b)) => a.is_subtype(b),
            (a, Optional(b)) => a.is_subtype(b),
            (Int, Decimal) => true,
            (Enum(_), String) => true,
            (Enum(a), Enum(b)) => a.iter().all(|v| b.contains(v)),
            (List(a), List(b)) => a.is_subtype(b),
            (Record(a), Record(b)) => {
                a.len() == b.len() && a.iter().all(|(k, t)| b.get(k).is_some_and(|u| t.is_subtype(u)))
            }
            _ => false,
        }
    }

    /// Least common supertype, if any (used for conditional branches, list literals, `==`).
    pub fn join(&self, other: &Type) -> Option<Type> {
        use Type::*;
        if self.is_subtype(other) {
            return Some(other.clone());
        }
        if other.is_subtype(self) {
            return Some(self.clone());
        }
        match (self, other) {
            (Null, t) | (t, Null) => Some(Type::optional(t.clone())),
            (Optional(a), b) | (b, Optional(a)) => a.join(b).map(Type::optional),
            (Enum(a), Enum(b)) => Some(Type::enumeration(a.iter().chain(b).cloned())),
            (Enum(_), String) | (String, Enum(_)) => Some(String),
            (List(a), List(b)) => a.join(b).map(Type::list),
            _ => None,
        }
    }

    /// Map a docs/11 §5 IR type (`"decimal"`, `{"list": …}`, `{"money": {"currency": "INR"}}`,
    /// `{"enum": [...]}`, `{"record": {...}}`) to an expression type.
    pub fn from_ir(v: &serde_json::Value) -> Result<Type, String> {
        use serde_json::Value as J;
        match v {
            J::String(s) => Ok(match s.as_str() {
                "string" | "text" => Type::String,
                "integer" => Type::Int,
                "decimal" => Type::Decimal,
                "boolean" => Type::Bool,
                "date" => Type::Date,
                "datetime" => Type::DateTime,
                "duration" => Type::Duration,
                "principal" => Type::Principal,
                "group" => Type::Group,
                "file" => Type::File,
                other => return Err(format!("unknown type `{other}`")),
            }),
            J::Object(o) if o.len() == 1 => {
                let (k, inner) = o.iter().next().ok_or("empty type")?;
                match k.as_str() {
                    "list" => Ok(Type::list(Type::from_ir(inner)?)),
                    "money" => {
                        let cur =
                            inner.get("currency").and_then(J::as_str).ok_or("money needs a currency")?;
                        if cur.len() == 3 && cur.bytes().all(|b| b.is_ascii_uppercase()) {
                            Ok(Type::Money(cur.to_string()))
                        } else {
                            Err(format!("`{cur}` is not an ISO 4217 currency code"))
                        }
                    }
                    "enum" => {
                        let vals = inner.as_array().ok_or("enum needs a list of values")?;
                        let strs: Option<Vec<&str>> = vals.iter().map(J::as_str).collect();
                        match strs {
                            Some(s) if !s.is_empty() => Ok(Type::enumeration(s)),
                            _ => Err("enum values must be a non-empty list of strings".into()),
                        }
                    }
                    "record" => {
                        let fields = inner.as_object().ok_or("record needs an object of fields")?;
                        let mut out = BTreeMap::new();
                        for (name, t) in fields {
                            out.insert(name.clone(), Type::from_ir(t)?);
                        }
                        Ok(Type::Record(out))
                    }
                    other => Err(format!("unknown type `{other}`")),
                }
            }
            _ => Err("a type is a name or a single-key object".into()),
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Null => f.write_str("null"),
            Type::Never => f.write_str("nothing"),
            Type::Bool => f.write_str("boolean"),
            Type::Int => f.write_str("integer"),
            Type::Decimal => f.write_str("decimal"),
            Type::String => f.write_str("string"),
            Type::Date => f.write_str("date"),
            Type::DateTime => f.write_str("datetime"),
            Type::Duration => f.write_str("duration"),
            Type::Money(c) => write!(f, "money<{c}>"),
            Type::Enum(v) => write!(f, "enum{{{}}}", v.join("|")),
            Type::Principal => f.write_str("principal"),
            Type::Group => f.write_str("group"),
            Type::File => f.write_str("file"),
            Type::Instance => f.write_str("instance"),
            Type::List(t) => write!(f, "list<{t}>"),
            Type::Record(fields) => {
                f.write_str("record{")?;
                for (i, (k, t)) in fields.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{k}: {t}")?;
                }
                f.write_str("}")
            }
            Type::Optional(t) => write!(f, "{t}?"),
        }
    }
}

/// Data classification (docs/11 §5, docs/12 §8), ordered from least to most sensitive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Classification {
    Public,
    #[default]
    Internal,
    Confidential,
    Restricted,
}

/// One variable visible to expressions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VarDecl {
    /// Already `Optional` unless the variable is required.
    pub ty: Type,
    pub classification: Classification,
}

/// The variables an expression may read. Built by the IR compiler from `variables` (docs/11 §5)
/// plus the context variables of §13.10.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schema {
    vars: BTreeMap<String, VarDecl>,
}

impl Schema {
    pub fn new() -> Self {
        Schema::default()
    }

    /// The context every process expression sees: `initiator` (principal) and `instance`.
    /// `now` is the `now()` function, bound from the evaluation environment.
    pub fn with_process_context() -> Self {
        let mut s = Schema::new();
        s.declare("initiator", Type::Principal, true, Classification::Internal);
        s.declare("instance", Type::Instance, true, Classification::Internal);
        s
    }

    /// Declare a variable. A non-required variable may be null.
    pub fn declare(
        &mut self,
        name: &str,
        ty: Type,
        required: bool,
        classification: Classification,
    ) -> &mut Self {
        let ty = if required { ty } else { Type::optional(ty) };
        self.vars.insert(name.to_string(), VarDecl { ty, classification });
        self
    }

    pub fn get(&self, name: &str) -> Option<&VarDecl> {
        self.vars.get(name)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &VarDecl)> {
        self.vars.iter()
    }
}
