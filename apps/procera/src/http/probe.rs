//! Denied-access audit for object ids that are not in the caller's workspace (SEC-01/02,
//! E2E-R1-ISO). Any workspace-plane request that names an object id and answers 404 after the
//! caller was authenticated as a member appends `access.denied` to the caller's own workspace chain:
//! a probe for another workspace's ids is indistinguishable from a missing id to the caller, but
//! never invisible to the workspace's auditors. Unknown paths and unauthenticated requests are not
//! recorded (they never reach a member).

use std::sync::{Arc, Mutex};

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use procera_audit::{ActorKind, AuditEvent, Category, Outcome, append};
use procera_db::WsCtx;
use procera_kernel::UuidV7Gen;
use uuid::Uuid;

use super::AppState;

/// Filled by the member extractor with who the request was authenticated as.
#[derive(Debug, Clone, Default)]
pub struct Seen(Arc<Mutex<Option<(WsCtx, Uuid)>>>);

impl Seen {
    pub fn record(&self, ctx: WsCtx, subject_ref: Uuid) {
        if let Ok(mut g) = self.0.lock() {
            *g = Some((ctx, subject_ref));
        }
    }
}

/// The object a path names: the segment before its first id, and the id.
fn object_of(path: &str) -> Option<(String, Uuid)> {
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    segs.iter().enumerate().find_map(|(i, s)| {
        let id = Uuid::parse_str(s).ok()?;
        let kind = i.checked_sub(1).and_then(|j| segs.get(j)).map_or("object", |k| k.trim_end_matches('s'));
        Some((kind.replace('-', "_"), id))
    })
}

pub async fn audit_not_found(State(s): State<AppState>, mut req: Request, next: Next) -> Response {
    let seen = Seen::default();
    req.extensions_mut().insert(seen.clone());
    let (method, path) = (req.method().clone(), req.uri().path().to_owned());
    let res = next.run(req).await;
    if res.status().as_u16() != 404 {
        return res;
    }
    let who = seen.0.lock().ok().and_then(|mut g| g.take());
    if let (Some((ctx, subject)), Some((kind, id)), Some(db)) = (who, object_of(&path), s.db.as_ref()) {
        let mut ev = AuditEvent::new(
            Some(subject),
            ActorKind::User,
            Category::Access,
            "access.denied",
            Outcome::Denied,
        )
        .object(&kind, id);
        ev.reason = Some(format!("{method} {path}: not found in this workspace"));
        let r: Result<(), String> = async {
            let mut tx = db.begin_ws(ctx).await.map_err(|e| e.to_string())?;
            append(&mut tx, s.clock.as_ref(), &UuidV7Gen, &ev).await.map_err(|e| e.to_string())?;
            tx.commit().await.map_err(|e| e.to_string())
        }
        .await;
        if let Err(e) = r {
            tracing::warn!(error = %e, "could not record a denied access");
        }
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_object_before_the_first_id() {
        let id = "01920000-0000-7000-8000-000000000001";
        assert_eq!(
            object_of(&format!("/api/v1/artifacts/{id}/raci")),
            Some(("artifact".into(), id.parse().unwrap()))
        );
        assert_eq!(object_of(&format!("/api/v1/files/{id}")).unwrap().0, "file");
        assert_eq!(object_of("/api/v1/members"), None);
    }
}
