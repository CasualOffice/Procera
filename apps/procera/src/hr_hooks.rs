//! HR time off on leave requests (BPMS-224): checks a leave request against the person's types,
//! dates and balance before it starts, holds its days while it is decided, and takes or releases
//! them when it ends. Workspaces without leave types keep the four original types and no balances.

use std::sync::Arc;

use procera_db::WsTx;
use procera_engine::BoxFuture;
use procera_hr::Hr;
use procera_hr::requests::{LEGACY_TYPES, ask_from};
use procera_kernel::Clock;
use procera_requests::forms::FieldError;
use procera_requests::hooks::{HookRequest, RequestHooks};

pub const LEAVE: &str = "hr.leave-request";

pub struct HrHooks {
    pub hr: Hr,
    pub clock: Arc<dyn Clock>,
}

impl std::fmt::Debug for HrHooks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HrHooks")
    }
}

impl RequestHooks for HrHooks {
    fn check<'a, 'b: 'a>(
        &'a self,
        tx: &'a mut WsTx<'b>,
        r: HookRequest<'a>,
    ) -> BoxFuture<'a, Result<Vec<FieldError>, String>> {
        Box::pin(async move {
            if r.key != LEAVE {
                return Ok(vec![]);
            }
            let enabled = self.hr.time_off_enabled(tx).await.map_err(|e| e.to_string())?;
            // The type on its own first (also when other answers are missing or malformed).
            let lt = r.vars.get("leave_type").and_then(|v| v.as_str()).map(str::trim).unwrap_or("");
            if !lt.is_empty() {
                let known = if enabled {
                    self.hr.leave_choices(tx).await.map_err(|e| e.to_string())?.iter().any(|(k, _)| k == lt)
                } else {
                    LEGACY_TYPES.contains(&lt)
                };
                if !known {
                    return Ok(vec![FieldError::new(
                        "leave_type",
                        if enabled { "unknown" } else { "invalid_option" },
                        "Choose a type of leave",
                    )]);
                }
            }
            let Some(ask) = ask_from(r.vars) else { return Ok(vec![]) };
            if !enabled {
                return Ok(vec![]);
            }
            let q = self
                .hr
                .quote_leave(tx, r.initiator, &ask, self.clock.now().date(), None)
                .await
                .map_err(|e| e.to_string())?;
            Ok(q.problems.into_iter().map(|p| FieldError::new(p.field, p.code, &p.message)).collect())
        })
    }

    fn status_changed<'a, 'b: 'a>(
        &'a self,
        tx: &'a mut WsTx<'b>,
        r: HookRequest<'a>,
        before: Option<&'a str>,
        after: &'a str,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let Some(id) = r.id else { return Ok(()) };
            if r.key != LEAVE || !self.hr.time_off_enabled(tx).await.map_err(|e| e.to_string())? {
                return Ok(());
            }
            let today = self.clock.now().date();
            match (before, after) {
                // Started, or sent again after changes: (re)count and hold.
                (None, "in_progress") | (Some("changes_requested"), "in_progress") => {
                    if let Some(ask) = ask_from(r.vars) {
                        self.hr
                            .place_leave(tx, id, r.initiator, &ask, today)
                            .await
                            .map_err(|e| e.to_string())?;
                    }
                }
                (_, "approved" | "completed" | "rejected" | "cancelled") => {
                    self.hr.settle_leave(tx, id, after).await.map_err(|e| e.to_string())?;
                }
                _ => {}
            }
            Ok(())
        })
    }
}
