//! Time source. Runtime and domain code MUST take a `Clock` instead of reading
//! the wall clock, so execution is deterministic and testable (docs/24 §3.6).

use std::fmt::Debug;
use time::OffsetDateTime;

/// A source of the current instant, always in UTC, at **microsecond** precision.
///
/// PostgreSQL stores microseconds; a nanosecond instant (Linux) would not round-trip, so a value
/// written and read back would compare unequal to the one returned to the caller.
pub trait Clock: Send + Sync + Debug {
    fn now(&self) -> OffsetDateTime;
}

/// Truncates an instant to whole microseconds (PostgreSQL `timestamptz` precision).
pub fn to_micros(t: OffsetDateTime) -> OffsetDateTime {
    let micros = t.nanosecond() / 1_000 * 1_000;
    t.replace_nanosecond(micros).unwrap_or(t)
}

/// Production clock backed by the system time.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> OffsetDateTime {
        to_micros(OffsetDateTime::now_utc())
    }
}

#[cfg(feature = "test-clock")]
pub use manual::ManualClock;

#[cfg(feature = "test-clock")]
mod manual {
    use super::{Clock, to_micros};
    use std::sync::Mutex;
    use time::{Duration, OffsetDateTime};

    /// A controllable clock for tests and the e2e build (`/__test/clock`).
    #[derive(Debug)]
    pub struct ManualClock {
        now: Mutex<OffsetDateTime>,
    }

    impl ManualClock {
        pub fn new(start: OffsetDateTime) -> Self {
            Self { now: Mutex::new(to_micros(start)) }
        }

        /// Moves time forward. Time never moves backwards.
        pub fn advance(&self, by: Duration) -> OffsetDateTime {
            let mut guard = self.now.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if by.is_positive() {
                *guard = to_micros(*guard + by);
            }
            *guard
        }
    }

    impl Clock for ManualClock {
        fn now(&self) -> OffsetDateTime {
            *self.now.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_clock_is_utc_with_microsecond_precision() {
        let now = SystemClock.now();
        assert!(now.offset().is_utc());
        assert_eq!(now.nanosecond() % 1_000, 0);
    }

    #[cfg(feature = "test-clock")]
    #[test]
    fn manual_clock_only_moves_forward() {
        use time::{Duration, macros::datetime};
        let c = ManualClock::new(datetime!(2026-11-02 00:00 UTC));
        c.advance(Duration::days(30));
        assert_eq!(c.now(), datetime!(2026-12-02 00:00 UTC));
        c.advance(Duration::days(-5));
        assert_eq!(c.now(), datetime!(2026-12-02 00:00 UTC));
    }
}
