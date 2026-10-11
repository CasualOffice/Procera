//! Lease and fencing rules for jobs (docs/04 §4.3).
//!
//! The SQL in [`crate::jobs`] implements exactly these rules; this pure model states them once
//! and lets property tests explore interleavings of workers, crashes and clock jumps.
//!
//! - A job is **claimable** when it is queued and due, or leased with an expired lease.
//! - A claim increments the **fence** and returns it. Only the holder of the current fence may
//!   complete or fail the job, so a worker whose lease expired (and whose job was re-claimed)
//!   cannot overwrite the newer claimant's result.
//! - The job's effect commits in the **same transaction** as the fenced completion, so a
//!   rejected completion discards the effect: an effect commits at most once.

use time::{Duration, OffsetDateTime};

/// How long a claim holds a job (docs/04 §4.3: 30 s).
pub const LEASE: Duration = Duration::seconds(30);
/// Attempts before a job is dead-lettered (docs/04 §2.5 `max_attempts` default).
pub const MAX_ATTEMPTS: i32 = 10;

/// Retry delay after the `attempts`-th failed attempt: 2^attempts seconds, at most one hour
/// (the same curve as email delivery, docs/04 §2.12).
pub fn backoff(attempts: i32) -> Duration {
    let exp = u32::try_from(attempts.clamp(0, 12)).unwrap_or(12);
    Duration::seconds(2_i64.pow(exp).min(3_600))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    Queued,
    Leased,
    Done,
    Dead,
}

impl JobState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Leased => "leased",
            Self::Done => "done",
            Self::Dead => "dead",
        }
    }
}

/// One job row, as far as leasing is concerned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lease {
    pub state: JobState,
    pub run_at: OffsetDateTime,
    pub lease_until: Option<OffsetDateTime>,
    pub fence: i64,
    pub attempts: i32,
    pub max_attempts: i32,
}

/// A completion or failure by a worker that no longer holds the job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("stale lease: fence {held} is no longer current")]
pub struct Stale {
    pub held: i64,
}

impl Lease {
    pub fn new(run_at: OffsetDateTime) -> Self {
        Self {
            state: JobState::Queued,
            run_at,
            lease_until: None,
            fence: 0,
            attempts: 0,
            max_attempts: MAX_ATTEMPTS,
        }
    }

    fn expired(&self, now: OffsetDateTime) -> bool {
        self.state == JobState::Leased && self.lease_until.is_some_and(|u| u < now)
    }

    pub fn claimable(&self, now: OffsetDateTime) -> bool {
        match self.state {
            JobState::Queued => self.run_at <= now,
            JobState::Leased => self.expired(now) && self.attempts < self.max_attempts,
            JobState::Done | JobState::Dead => false,
        }
    }

    /// Dead-letters a job whose last allowed attempt lost its lease (the worker crashed or hung
    /// every time). Returns whether it did.
    pub fn reap(&mut self, now: OffsetDateTime) -> bool {
        if self.expired(now) && self.attempts >= self.max_attempts {
            self.state = JobState::Dead;
            self.lease_until = None;
            return true;
        }
        false
    }

    /// Claims the job; returns the new fence.
    pub fn claim(&mut self, now: OffsetDateTime) -> Option<i64> {
        self.reap(now);
        if !self.claimable(now) {
            return None;
        }
        self.state = JobState::Leased;
        self.lease_until = Some(now + LEASE);
        self.fence += 1;
        self.attempts += 1;
        Some(self.fence)
    }

    fn check(&self, fence: i64) -> Result<(), Stale> {
        if self.state == JobState::Leased && self.fence == fence {
            Ok(())
        } else {
            Err(Stale { held: fence })
        }
    }

    /// Marks the job done. The caller commits its effect only when this succeeds.
    pub fn complete(&mut self, fence: i64) -> Result<(), Stale> {
        self.check(fence)?;
        self.state = JobState::Done;
        self.lease_until = None;
        Ok(())
    }

    /// Records a failed attempt: retry after [`backoff`], or dead-letter after `max_attempts`.
    pub fn fail(&mut self, fence: i64, now: OffsetDateTime) -> Result<JobState, Stale> {
        self.check(fence)?;
        self.lease_until = None;
        if self.attempts >= self.max_attempts {
            self.state = JobState::Dead;
        } else {
            self.state = JobState::Queued;
            self.run_at = now + backoff(self.attempts);
        }
        Ok(self.state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use time::macros::datetime;

    const T0: OffsetDateTime = datetime!(2026-10-11 09:00 UTC);

    #[test]
    fn a_stale_holder_cannot_complete() {
        let mut job = Lease::new(T0);
        let first = job.claim(T0).unwrap();
        assert_eq!(job.claim(T0 + Duration::seconds(10)), None, "the lease is still held");
        let second = job.claim(T0 + LEASE + Duration::seconds(1)).unwrap();
        assert_eq!(second, first + 1);
        assert_eq!(job.complete(first), Err(Stale { held: first }));
        assert_eq!(job.complete(second), Ok(()));
        assert_eq!(job.complete(second), Err(Stale { held: second }), "done is final");
    }

    #[test]
    fn failures_back_off_then_dead_letter() {
        assert_eq!(backoff(1), Duration::seconds(2));
        assert_eq!(backoff(20), Duration::hours(1));
        let mut job = Lease { max_attempts: 3, ..Lease::new(T0) };
        let mut now = T0;
        for n in 1..=3 {
            let f = job.claim(now).unwrap();
            let st = job.fail(f, now).unwrap();
            if n < 3 {
                assert_eq!(st, JobState::Queued);
                assert!(!job.claimable(now));
                now = job.run_at;
            } else {
                assert_eq!(st, JobState::Dead);
            }
        }
        assert!(!job.claimable(now + Duration::days(1)));
    }

    #[derive(Debug, Clone)]
    enum Op {
        Claim(usize),
        Complete(usize),
        Fail(usize),
        Tick(i64),
    }

    fn op() -> impl Strategy<Value = Op> {
        prop_oneof![
            (0usize..3).prop_map(Op::Claim),
            (0usize..3).prop_map(Op::Complete),
            (0usize..3).prop_map(Op::Fail),
            (0i64..90).prop_map(Op::Tick),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]

        /// Any interleaving of three workers: the effect commits at most once, only the current
        /// fence holder can finish, and fences only grow.
        #[test]
        fn effects_commit_at_most_once(ops in prop::collection::vec(op(), 1..60)) {
            let mut job = Lease::new(T0);
            let mut now = T0;
            let mut held: [Option<i64>; 3] = [None; 3];
            let mut committed = 0;
            let mut last_fence = 0;
            for o in ops {
                match o {
                    Op::Claim(w) => if let Some(f) = job.claim(now) {
                        prop_assert!(f > last_fence);
                        last_fence = f;
                        held[w] = Some(f);
                    },
                    Op::Complete(w) => if let Some(f) = held[w].take() {
                        let current = job.state == JobState::Leased && job.fence == f;
                        let ok = job.complete(f).is_ok();
                        prop_assert_eq!(ok, current);
                        if ok { committed += 1; }
                    },
                    Op::Fail(w) => if let Some(f) = held[w].take() {
                        let _ = job.fail(f, now);
                    },
                    Op::Tick(s) => now += Duration::seconds(s),
                }
                prop_assert!(committed <= 1);
                prop_assert!(job.attempts <= job.max_attempts);
            }
        }
    }
}
