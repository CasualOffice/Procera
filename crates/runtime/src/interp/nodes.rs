//! Executing a ready token at its node (docs/04 §4.2, docs/11 §3).

use super::Run;
use crate::command::{Command, InstanceResult, TimerDue, TimerPurpose, WorkKind};
use crate::state::{Frame, IncidentCode, Split, SplitId, TokenId, TokenState, TraceEvent, Wait, WaitKind};
use procera_ir::field_key;
use procera_ir::model::{EndResult, Node, NodeKind};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

impl Run<'_> {
    pub(super) fn execute(&mut self, id: TokenId) {
        let Some(node) = self.token_node(id).and_then(|n| self.node(&n)) else { return };
        match node.kind {
            NodeKind::Start => self.leave(id, "next"),
            NodeKind::End => self.end(id, &node),
            NodeKind::Set => self.set(id, &node),
            NodeKind::Decision => self.decide(id, &node),
            NodeKind::ParallelSplit => self.fork(id, &node),
            NodeKind::ParallelJoin => self.join(id, &node),
            NodeKind::HumanTask
            | NodeKind::HumanApproval
            | NodeKind::HumanSign
            | NodeKind::HumanAcknowledge => self.work_item(id, &node),
            NodeKind::WaitTimer => self.timer(id, &node),
            NodeKind::WaitSignal => self.signal(id, &node),
            NodeKind::ServiceCall => self.service(id, &node),
            NodeKind::Notify => self.notify(id, &node),
            NodeKind::SubprocessCall => self.subprocess(id, &node),
            NodeKind::CaseOpen => self.case(id, &node),
        }
    }

    fn end(&mut self, id: TokenId, node: &Node) {
        self.st.tokens.remove(&id);
        self.trace(Some(id), Some(&node.id), TraceEvent::TokenConsumed);
        if node.result == Some(EndResult::Terminated) {
            self.finish(InstanceResult::Terminated, format!("end:{}", node.id));
        }
    }

    /// `set`: every expression sees the variables as they were before the node (parallel
    /// assignment, A-23).
    fn set(&mut self, id: TokenId, node: &Node) {
        let mut values = BTreeMap::new();
        for (name, expr) in node.assign.iter().flatten() {
            match self.eval(&node.id, &field_key("assign", name), expr) {
                Ok(v) => values.insert(name.clone(), v),
                Err(e) => return self.eval_failed(id, e),
            };
        }
        let names: Vec<String> = values.keys().cloned().collect();
        self.st.variables.extend(values);
        self.trace(Some(id), Some(&node.id), TraceEvent::VariablesSet { names });
        self.leave(id, "next");
    }

    /// `decision`: the first branch whose condition is true, in declared order, else `default`.
    /// A rule-table decision takes the branch named by the output value (docs/11 §13.1).
    fn decide(&mut self, id: TokenId, node: &Node) {
        let branches = node.branches.clone().unwrap_or_default();
        if let (Some(rule), Some(output)) = (&node.rule, &node.output) {
            let reference = self.it.program.ir().refs.rules.get(rule).cloned();
            let result = self.it.evaluator.eval_rule(rule, reference.as_deref(), output, &self.scope());
            let chosen = match result {
                Ok(Value::String(s)) => Some(s),
                Ok(Value::Number(n)) => Some(n.to_string()),
                Ok(Value::Bool(b)) => Some(b.to_string()),
                Ok(_) => None,
                Err(e) => return self.eval_failed(id, e),
            };
            let outcome = chosen.filter(|c| branches.iter().any(|b| &b.name == c));
            return self.leave(id, outcome.as_deref().unwrap_or("default"));
        }
        for (i, b) in branches.iter().enumerate() {
            let Some(when) = &b.when else { continue };
            match self.eval(&node.id, &format!("branches/{i}/when"), when) {
                Ok(Value::Bool(true)) => return self.leave(id, &b.name),
                Ok(Value::Bool(false)) => {}
                Ok(other) => {
                    let msg = format!("branch {} condition gave {other}, not a boolean", b.name);
                    return self.incident(id, IncidentCode::NonBoolean, msg);
                }
                Err(e) => return self.eval_failed(id, e),
            }
        }
        self.leave(id, "default");
    }

    /// `parallel.split`: one child token per branch, in declared order (docs/04 §4.2).
    fn fork(&mut self, id: TokenId, node: &Node) {
        let names: Vec<String> = node.branches.iter().flatten().map(|b| b.name.clone()).collect();
        let targets: Option<Vec<String>> =
            names.iter().map(|b| self.it.program.target(&node.id, b).map(str::to_owned)).collect();
        let (Some(targets), Some(join)) = (targets, node.join_id.clone()) else {
            return self.incident(id, IncidentCode::NoEdge, "split branch without an edge".to_owned());
        };
        let Some(parent) = self.st.tokens.remove(&id) else { return };
        let split = SplitId(self.st.alloc());
        let expected = names.clone();
        let arrived = BTreeSet::new();
        self.st
            .splits
            .insert(split, Split { split_node: node.id.clone(), join_node: join, expected, arrived });
        self.trace(Some(id), Some(&node.id), TraceEvent::Forked { split, branches: names.clone() });
        for (branch, to) in names.into_iter().zip(targets) {
            let mut frames = parent.frames.clone();
            frames.push(Frame { split, branch });
            let child = self.spawn(&to, frames);
            self.enter(child, &to);
        }
    }

    /// `parallel.join`: records the arrival; the last branch to arrive releases one token.
    /// Arrivals are a set keyed by branch, and the split is removed on release, so a join
    /// releases exactly once (docs/04 §4.2, docs/15 §5).
    fn join(&mut self, id: TokenId, node: &Node) {
        let frame = self.st.tokens.get(&id).and_then(|t| t.frames.last().cloned());
        let Some(frame) =
            frame.filter(|f| self.st.splits.get(&f.split).is_some_and(|s| s.join_node == node.id))
        else {
            return self.incident(
                id,
                IncidentCode::Join,
                "token does not belong to this join's split".to_owned(),
            );
        };
        let Some(token) = self.st.tokens.remove(&id) else { return };
        let done = match self.st.splits.get_mut(&frame.split) {
            Some(s) => {
                s.arrived.insert(frame.branch.clone());
                s.expected.iter().all(|b| s.arrived.contains(b))
            }
            None => false,
        };
        self.trace(
            Some(id),
            Some(&node.id),
            TraceEvent::JoinArrived { split: frame.split, branch: frame.branch },
        );
        if done {
            self.st.splits.remove(&frame.split);
            self.trace(None, Some(&node.id), TraceEvent::JoinReleased { split: frame.split });
            let mut frames = token.frames;
            frames.pop();
            let out = self.spawn(&node.id, frames);
            self.trace(Some(out), Some(&node.id), TraceEvent::NodeEntered);
            self.leave(out, "next");
        }
    }

    fn activation(&mut self, node: &Node) -> String {
        format!("{}/{}/{}", self.iid(), node.id, self.st.alloc())
    }

    /// Parks the token and arms its `timeout` boundaries (docs/11 §4.1).
    fn wait(&mut self, id: TokenId, node: &Node, kind: WaitKind, key: String, timeout_timer: Option<String>) {
        let mut boundary_timers = Vec::new();
        for (i, b) in node.boundary.iter().flatten().enumerate() {
            let Some(secs) =
                (b.on == "timeout").then_some(()).and(b.after.as_ref()).and_then(|d| d.seconds())
            else {
                continue;
            };
            let timer = format!("{key}/boundary/{i}");
            let due = TimerDue::At { at: self.after(secs) };
            let purpose = TimerPurpose::Boundary;
            let cmd =
                Command::ScheduleTimer { key: timer.clone(), token: id, node: node.id.clone(), purpose, due };
            self.cmds.push(cmd);
            boundary_timers.push((timer, i));
        }
        self.trace(Some(id), Some(&node.id), TraceEvent::Waiting { wait: kind, key: key.clone() });
        if let Some(t) = self.st.tokens.get_mut(&id) {
            t.state = TokenState::Waiting(Wait { kind, key, boundary_timers, timeout_timer });
        }
    }

    fn after(&self, secs: u64) -> OffsetDateTime {
        let secs = i64::try_from(secs).unwrap_or(i64::MAX);
        self.at.checked_add(time::Duration::seconds(secs)).unwrap_or(self.at)
    }

    fn work_item(&mut self, id: TokenId, node: &Node) {
        let kind = match node.kind {
            NodeKind::HumanApproval => WorkKind::Approval,
            NodeKind::HumanSign => WorkKind::Sign,
            NodeKind::HumanAcknowledge => WorkKind::Acknowledge,
            _ => WorkKind::Task,
        };
        let key = self.activation(node);
        self.cmds.push(Command::CreateWorkItem {
            key: key.clone(),
            token: id,
            node: node.id.clone(),
            kind,
            assignment: node.assignment.clone().or_else(|| node.signers.clone()),
            form: node.form.clone(),
            outcomes: node.outcomes(),
            due: node.due.clone(),
            escalations: node.escalations.clone().unwrap_or_default(),
            quorum: node.quorum.clone(),
            signature: node.signature,
            subject: node.subject.clone().or_else(|| node.document.clone()),
        });
        self.wait(id, node, WaitKind::WorkItem, key, None);
    }

    fn timer(&mut self, id: TokenId, node: &Node) {
        let due = if let Some(expr) = &node.at {
            let instant = match self.eval(&node.id, "at", expr) {
                Ok(Value::String(s)) => OffsetDateTime::parse(&s, &Rfc3339).ok(),
                Ok(_) => None,
                Err(e) => return self.eval_failed(id, e),
            };
            match instant {
                Some(at) => TimerDue::At { at },
                None => {
                    return self.incident(
                        id,
                        IncidentCode::BadValue,
                        "`at` is not an RFC 3339 instant".into(),
                    );
                }
            }
        } else if let Some(after) = &node.after {
            let Some(secs) = after.seconds() else {
                return self.incident(id, IncidentCode::BadValue, format!("bad duration {}", after.0));
            };
            if node.business == Some(true) {
                let calendar =
                    node.calendar.clone().or_else(|| self.it.program.default_calendar().map(str::to_owned));
                TimerDue::Business { from: self.at, duration: after.0.clone(), calendar }
            } else {
                TimerDue::At { at: self.after(secs) }
            }
        } else {
            return self.incident(id, IncidentCode::BadValue, "timer without `at` or `after`".into());
        };
        let key = self.activation(node);
        let purpose = TimerPurpose::Wait;
        self.cmds.push(Command::ScheduleTimer {
            key: key.clone(),
            token: id,
            node: node.id.clone(),
            purpose,
            due,
        });
        self.wait(id, node, WaitKind::Timer, key, None);
    }

    fn signal(&mut self, id: TokenId, node: &Node) {
        let correlation = match node.correlation.as_ref().map(|c| self.eval(&node.id, "correlation", c)) {
            Some(Ok(v)) => v,
            Some(Err(e)) => return self.eval_failed(id, e),
            None => Value::Null,
        };
        let key = self.activation(node);
        let n = node.id.clone();
        self.cmds.push(Command::SubscribeSignal {
            key: key.clone(),
            token: id,
            node: n.clone(),
            correlation,
        });
        let timeout = node.timeout.as_ref().and_then(|d| d.seconds()).map(|secs| {
            let timer = format!("{key}/timeout");
            let due = TimerDue::At { at: self.after(secs) };
            let purpose = TimerPurpose::SignalTimeout;
            self.cmds.push(Command::ScheduleTimer { key: timer.clone(), token: id, node: n, purpose, due });
            timer
        });
        self.wait(id, node, WaitKind::Signal, key, timeout);
    }

    fn service(&mut self, id: TokenId, node: &Node) {
        let mut input = BTreeMap::new();
        for (name, expr) in node.input.iter().flatten() {
            match self.eval(&node.id, &field_key("input", name), expr) {
                Ok(v) => input.insert(name.clone(), v),
                Err(e) => return self.eval_failed(id, e),
            };
        }
        let connector_key =
            match node.idempotency_key.as_ref().map(|e| self.eval(&node.id, "idempotencyKey", e)) {
                Some(Ok(Value::String(s))) => s,
                Some(Ok(Value::Number(n))) => n.to_string(),
                Some(Ok(other)) => {
                    let msg = format!("idempotencyKey gave {other}, not a string");
                    return self.incident(id, IncidentCode::BadValue, msg);
                }
                Some(Err(e)) => return self.eval_failed(id, e),
                None => {
                    return self.incident(
                        id,
                        IncidentCode::BadValue,
                        "service.call without idempotencyKey".into(),
                    );
                }
            };
        let key = self.activation(node);
        self.cmds.push(Command::CallConnector {
            key: key.clone(),
            token: id,
            node: node.id.clone(),
            connector: node.connector.clone().unwrap_or_default(),
            action: node.action.clone().unwrap_or_default(),
            input,
            connector_key,
        });
        self.wait(id, node, WaitKind::Service, key, None);
    }

    fn notify(&mut self, id: TokenId, node: &Node) {
        let key = self.activation(node);
        self.cmds.push(Command::Notify {
            key,
            node: node.id.clone(),
            to: node.to.clone(),
            template: node.template.clone(),
            channels: node.channels.clone().unwrap_or_default(),
        });
        self.leave(id, "next");
    }

    fn subprocess(&mut self, id: TokenId, node: &Node) {
        let key = self.activation(node);
        let process = node.process.clone().unwrap_or_default();
        self.cmds.push(Command::StartSubprocess {
            key: key.clone(),
            token: id,
            node: node.id.clone(),
            process,
        });
        self.wait(id, node, WaitKind::Subprocess, key, None);
    }

    fn case(&mut self, id: TokenId, node: &Node) {
        let key = self.activation(node);
        let wait_for_close = node.wait_for_close == Some(true);
        self.cmds.push(Command::OpenCase {
            key: key.clone(),
            token: id,
            node: node.id.clone(),
            case_type: node.case_type.clone(),
            wait_for_close,
        });
        if wait_for_close {
            self.wait(id, node, WaitKind::Case, key, None);
        } else {
            self.leave(id, "opened");
        }
    }
}
