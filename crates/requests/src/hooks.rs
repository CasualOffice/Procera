//! Domain hooks on requests (BPMS-224): another part of Procera checks a request before it starts
//! and follows its status, inside the same transactions. HR time off uses them to check a leave
//! request against the person's balance, hold the days while it is decided, and take or release them.

use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

use procera_db::WsTx;
use procera_engine::BoxFuture;
use serde_json::Value;
use uuid::Uuid;

use crate::forms::FieldError;

/// A request at one of the hook points.
#[derive(Debug, Clone, Copy)]
pub struct HookRequest<'a> {
    /// The request type's key, e.g. `hr.leave-request`.
    pub key: &'a str,
    /// The request (its process instance), once it exists.
    pub id: Option<Uuid>,
    pub initiator: Uuid,
    pub vars: &'a BTreeMap<String, Value>,
}

pub trait RequestHooks: Send + Sync + std::fmt::Debug {
    /// Extra checks before a request starts; errors name fields of the start form.
    fn check<'a, 'b: 'a>(
        &'a self,
        tx: &'a mut WsTx<'b>,
        r: HookRequest<'a>,
    ) -> BoxFuture<'a, Result<Vec<FieldError>, String>>;

    /// The request's status changed (`before` is `None` when it has just started).
    fn status_changed<'a, 'b: 'a>(
        &'a self,
        tx: &'a mut WsTx<'b>,
        r: HookRequest<'a>,
        before: Option<&'a str>,
        after: &'a str,
    ) -> BoxFuture<'a, Result<(), String>>;
}

/// The hooks in use, set once at start-up (shared by the service and the engine's effect handler).
pub type HookSlot = Arc<OnceLock<Arc<dyn RequestHooks>>>;
