//! Completion events resume the waiting token they address (docs/04 §4.2 "Completion signals").

use super::{EventError, Run};
use crate::command::{Command, InstanceResult};
use crate::event::Event;
use crate::state::{TokenId, TokenState, TraceEvent, Wait, WaitKind};
use serde_json::Value;
use std::collections::BTreeMap;

/// Which part of a token's wait a key names.
enum Hit {
    Activation,
    SignalTimeout,
    Boundary(usize),
}

impl Run<'_> {
    pub(super) fn apply(&mut self, event: &Event) -> Result<(), EventError> {
        match event {
            Event::Start { .. } => Err(EventError::AlreadyStarted),
            Event::WorkItemCompleted { key, outcome, variables } => {
                let id = self.expect(key, WaitKind::WorkItem, "work_item_completed")?;
                self.check_outcome(id, outcome)?;
                self.resume(id, variables, outcome)
            }
            Event::SignalReceived { key, variables } => {
                let id = self.expect(key, WaitKind::Signal, "signal_received")?;
                self.resume(id, variables, "received")
            }
            Event::ServiceCompleted { key, outcome, variables } => {
                let id = self.expect(key, WaitKind::Service, "service_completed")?;
                if let Some(code) = outcome.strip_prefix("error:") {
                    // A typed error goes to its boundary when one is declared, else to `error`.
                    let on = format!("error:{code}");
                    let node = self.token_node(id).and_then(|n| self.node(&n));
                    let boundary =
                        node.and_then(|n| n.boundary.into_iter().flatten().position(|b| b.on == on));
                    self.it.check_declared(variables)?;
                    self.set_variables(id, variables);
                    return match boundary {
                        Some(i) => {
                            self.boundary(id, i);
                            Ok(())
                        }
                        None => self.resume(id, &BTreeMap::new(), "error"),
                    };
                }
                self.check_outcome(id, outcome)?;
                self.resume(id, variables, outcome)
            }
            Event::SubprocessFinished { key, outcome, variables } => {
                let id = self.expect(key, WaitKind::Subprocess, "subprocess_finished")?;
                self.check_outcome(id, outcome)?;
                self.resume(id, variables, outcome)
            }
            Event::CaseClosed { key } => {
                let id = self.expect(key, WaitKind::Case, "case_closed")?;
                self.resume(id, &BTreeMap::new(), "closed")
            }
            Event::TimerFired { key } => {
                let (id, hit) = self.find(key).ok_or_else(|| EventError::UnknownKey(key.clone()))?;
                match hit {
                    Hit::Activation if self.wait_of(id).is_some_and(|w| w.kind == WaitKind::Timer) => {
                        self.resume(id, &BTreeMap::new(), "fired")
                    }
                    Hit::Activation => Err(EventError::WrongEvent { key: key.clone(), event: "timer_fired" }),
                    Hit::SignalTimeout => {
                        // The timer itself fired; withdraw the subscription instead.
                        if let Some(TokenState::Waiting(w)) =
                            self.st.tokens.get_mut(&id).map(|t| &mut t.state)
                        {
                            w.timeout_timer = None;
                            let key = w.key.clone();
                            self.cmds.push(Command::CancelSubscription { key });
                        }
                        self.resume(id, &BTreeMap::new(), "timeout")
                    }
                    Hit::Boundary(i) => {
                        self.boundary(id, i);
                        Ok(())
                    }
                }
            }
            Event::Cancel { reason } => {
                self.finish(InstanceResult::Terminated, format!("cancelled:{reason}"));
                Ok(())
            }
            Event::Continue => {
                if self.st.tokens.values().any(|t| t.state == TokenState::Ready) {
                    Ok(())
                } else {
                    Err(EventError::NothingToContinue)
                }
            }
        }
    }

    fn find(&self, key: &str) -> Option<(TokenId, Hit)> {
        self.st.waits().find_map(|(t, w)| {
            if w.key == key {
                Some((t.id, Hit::Activation))
            } else if w.timeout_timer.as_deref() == Some(key) {
                Some((t.id, Hit::SignalTimeout))
            } else {
                w.boundary_timers.iter().find(|(k, _)| k == key).map(|(_, i)| (t.id, Hit::Boundary(*i)))
            }
        })
    }

    fn wait_of(&self, id: TokenId) -> Option<&Wait> {
        match &self.st.tokens.get(&id)?.state {
            TokenState::Waiting(w) => Some(w),
            _ => None,
        }
    }

    /// The token waiting on activation `key`, which must be of `kind`.
    fn expect(&self, key: &str, kind: WaitKind, event: &'static str) -> Result<TokenId, EventError> {
        match self.find(key) {
            Some((id, Hit::Activation)) if self.wait_of(id).is_some_and(|w| w.kind == kind) => Ok(id),
            Some(_) => Err(EventError::WrongEvent { key: key.to_owned(), event }),
            None => Err(EventError::UnknownKey(key.to_owned())),
        }
    }

    fn check_outcome(&self, id: TokenId, outcome: &str) -> Result<(), EventError> {
        let node = self.token_node(id).and_then(|n| self.node(&n));
        match node {
            Some(n) if n.outcomes().iter().any(|o| o == outcome) => Ok(()),
            Some(n) => Err(EventError::Outcome { node: n.id, outcome: outcome.to_owned() }),
            None => Err(EventError::UnknownKey(String::new())),
        }
    }

    fn set_variables(&mut self, id: TokenId, variables: &BTreeMap<String, Value>) {
        if variables.is_empty() {
            return;
        }
        let node = self.token_node(id);
        self.st.variables.extend(variables.clone());
        let names = variables.keys().cloned().collect();
        self.trace(Some(id), node.as_deref(), TraceEvent::VariablesSet { names });
    }

    /// Takes the token out of its wait, disarming every other timer of the activation.
    fn unpark(&mut self, id: TokenId, keep_activation: bool) -> Option<Wait> {
        let wait = self.wait_of(id)?.clone();
        for (timer, _) in &wait.boundary_timers {
            self.cmds.push(Command::CancelTimer { key: timer.clone() });
        }
        if keep_activation {
            // The activation itself completed; only its signal timeout is still armed.
            if let Some(timer) = &wait.timeout_timer {
                self.cmds.push(Command::CancelTimer { key: timer.clone() });
            }
        } else {
            self.cancel_activation(&wait);
        }
        if let Some(t) = self.st.tokens.get_mut(&id) {
            t.state = TokenState::Ready;
        }
        let node = self.token_node(id);
        self.trace(Some(id), node.as_deref(), TraceEvent::Resumed { key: wait.key.clone() });
        Some(wait)
    }

    fn resume(
        &mut self,
        id: TokenId,
        variables: &BTreeMap<String, Value>,
        outcome: &str,
    ) -> Result<(), EventError> {
        self.it.check_declared(variables)?;
        self.unpark(id, true);
        self.set_variables(id, variables);
        self.leave(id, outcome);
        Ok(())
    }

    /// A boundary fired (docs/11 §4.1). Interrupting (the default) cancels the activation and
    /// moves the token; non-interrupting leaves it waiting and spawns a token at the target.
    fn boundary(&mut self, id: TokenId, index: usize) {
        let Some(node) = self.token_node(id).and_then(|n| self.node(&n)) else { return };
        let Some(b) = node.boundary.iter().flatten().nth(index).cloned() else { return };
        let interrupting = b.interrupting != Some(false);
        self.trace(
            Some(id),
            Some(&node.id),
            TraceEvent::BoundaryTriggered { on: b.on.clone(), interrupting },
        );
        if interrupting {
            self.unpark(id, false);
            self.enter(id, &b.to);
            return;
        }
        let frames = self.st.tokens.get(&id).map(|t| t.frames.clone()).unwrap_or_default();
        if let Some(TokenState::Waiting(w)) = self.st.tokens.get_mut(&id).map(|t| &mut t.state) {
            w.boundary_timers.retain(|(_, i)| *i != index);
        }
        let spawned = self.spawn(&b.to, frames);
        self.enter(spawned, &b.to);
    }

    /// Cancels an open activation and all of its timers.
    pub(super) fn cancel_wait(&mut self, wait: &Wait) {
        for (timer, _) in &wait.boundary_timers {
            self.cmds.push(Command::CancelTimer { key: timer.clone() });
        }
        self.cancel_activation(wait);
    }

    /// In-flight connector calls and opened cases are not cancelled: a late connector result
    /// is rejected as an unknown key, and a case is its own aggregate (A-23).
    fn cancel_activation(&mut self, wait: &Wait) {
        let key = wait.key.clone();
        match wait.kind {
            WaitKind::WorkItem => self.cmds.push(Command::CancelWorkItem { key }),
            WaitKind::Timer => self.cmds.push(Command::CancelTimer { key }),
            WaitKind::Signal => {
                self.cmds.push(Command::CancelSubscription { key });
                if let Some(timer) = &wait.timeout_timer {
                    self.cmds.push(Command::CancelTimer { key: timer.clone() });
                }
            }
            WaitKind::Subprocess => self.cmds.push(Command::CancelSubprocess { key }),
            WaitKind::Service | WaitKind::Case => {}
        }
    }
}
