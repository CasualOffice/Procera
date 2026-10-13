//! Search API (BPMS-049, docs/08 "Search", docs/05 §2.4): one ranked list across the types the
//! caller may view. Each type is gated by the policy evaluator before the query runs: artifact
//! kinds by their `*:view` permission, members by `member:view`, departments and groups by
//! `directory:view`. Types the caller may not view are left out silently, as in library lists.

use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use procera_access::Resource;
use procera_search::{SearchError, parse_types, search as run};
use serde::Deserialize;
use serde_json::json;

use super::AppState;
use super::error::ApiError;
use super::workspace::{WsMember, members};

pub fn router() -> Router<AppState> {
    Router::new().route("/search", get(search))
}

impl From<SearchError> for ApiError {
    fn from(e: SearchError) -> Self {
        match e {
            SearchError::Invalid(m) => Self::new(422, "validation_failed", "Validation failed").detail(m),
            SearchError::Internal(m) => {
                tracing::error!(error = %m, "search internal error");
                Self::new(500, "internal", "Internal error")
            }
        }
    }
}

fn internal(e: impl std::fmt::Display) -> ApiError {
    tracing::error!(error = %e, "search api internal error");
    ApiError::new(500, "internal", "Internal error")
}

/// The permission that lets a member see results of `kind`.
fn view_permission(kind: &str) -> Result<&'static str, ApiError> {
    Ok(match kind {
        "member" => "member:view",
        "department" | "group" => "directory:view",
        artifact_kind => procera_library::permission(artifact_kind, "view")?,
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchQuery {
    #[serde(default)]
    q: String,
    #[serde(default)]
    types: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
}

/// `GET /search`: one ranked list across the types you may view. Query: `q`, `types` (comma list) and
/// `limit` (default 20). Types you may not view, and documents hidden from you, are left out silently.
async fn search(
    State(s): State<AppState>,
    me: WsMember,
    Query(r): Query<SearchQuery>,
) -> Result<Response, ApiError> {
    let wanted = parse_types(r.types.as_deref())?;
    let m = members(&s)?;
    let mut tx = me.tx(m).await?;
    let mut allowed = Vec::with_capacity(wanted.len());
    for kind in wanted {
        let d = m
            .access()
            .check(&mut tx, me.actor.membership, view_permission(kind)?, &Resource::Workspace)
            .await?;
        if d.allowed {
            allowed.push(kind);
        }
    }
    let mut items = run(&mut tx, &r.q, &allowed, r.limit.unwrap_or(20)).await?;
    // Artifacts the caller may not see (drafts, restricted policies outside their applicability;
    // A-14) are dropped from the page.
    let artifact_ids: Vec<uuid::Uuid> = items
        .iter()
        .filter(|h| procera_library::AUTHORABLE.contains(&h.kind.as_str()))
        .map(|h| h.id)
        .collect();
    if !artifact_ids.is_empty() {
        let full = super::library::full_kinds(&s, &me, &mut tx).await?;
        let reader = procera_library::Reader { membership: me.actor.membership, full_kinds: &full };
        let lib = s.library.as_ref().ok_or_else(|| ApiError::unavailable("database"))?;
        let visible = lib.visible_ids(&mut tx, &artifact_ids, &reader).await?;
        items.retain(|h| !procera_library::AUTHORABLE.contains(&h.kind.as_str()) || visible.contains(&h.id));
    }
    tx.rollback().await.map_err(internal)?;
    Ok(Json(json!({ "items": items, "types": allowed })).into_response())
}

#[cfg(test)]
mod tests {
    use super::view_permission;

    #[test]
    fn every_type_maps_to_a_catalogue_permission() {
        for t in procera_search::TYPES {
            let p = view_permission(t).unwrap_or_default();
            assert!(procera_access::catalogue::is_permission(p), "{t}");
        }
    }
}
