//! Business calendars and business-duration arithmetic (docs/04 §2.12, §4.4; FR-W05, FR-E12).
//!
//! A calendar version is immutable: working hours per weekday, holiday dates and a time zone.
//! The time zone is a **fixed UTC offset** (`+05:30`); there is no IANA zone database in the
//! engine yet (A-22 (5), A-27).
//!
//! A business duration (`TimerDue::Business`, business SLAs) is an ISO-8601 v1 duration whose
//! parts mean (A-27):
//! - **date part** (`W`, `D`): working days. `nD` lands at the same local time on the n-th next
//!   working day, after first rolling a start outside working time forward to the next working
//!   instant; a time the target day does not work is clamped into that day's hours. `nW` is
//!   `n × (working weekdays of the calendar)` days, so `P1W` is one working week;
//! - **time part** (`H`, `M`, `S`): working time, counted only inside working hours.
//!
//! Adding a zero duration is the identity, and every addition is monotonic in its start and in
//! each part (property tests below).

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use time::{Date, Duration, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset};

const DAY_SECS: u32 = 86_400;
const WEEKDAYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CalendarError {
    #[error("time zone {0} is not a fixed offset like +05:30")]
    Offset(String),
    #[error("unknown weekday {0}")]
    Weekday(String),
    #[error("bad working-hours time {0} (HH:MM, 00:00-24:00)")]
    Time(String),
    #[error("working hours on {0} must be ordered, non-empty and non-overlapping")]
    Ranges(String),
    #[error("the calendar has no working time")]
    NoWorkingTime,
    #[error("bad business duration {0}")]
    Duration(String),
    #[error("the result is out of range")]
    Range,
}

/// The stored form of a calendar version (`calendar_versions`): `{"mon": [["09:00", "17:00"]]}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalendarSpec {
    pub timezone: String,
    pub working_hours: BTreeMap<String, Vec<[String; 2]>>,
    #[serde(default)]
    pub holidays: Vec<Date>,
}

/// A validated calendar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Calendar {
    offset: UtcOffset,
    /// Working ranges per weekday (Monday first), seconds from local midnight, `[start, end)`.
    week: [Vec<(u32, u32)>; 7],
    holidays: BTreeSet<Date>,
}

/// A parsed business duration (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BusinessDuration {
    pub weeks: u64,
    pub days: u64,
    pub seconds: u64,
}

impl BusinessDuration {
    /// Parses the docs/11 §13.9 subset: `P[nW][nD][T[nH][nM][nS]]`.
    pub fn parse(s: &str) -> Result<Self, CalendarError> {
        let bad = || CalendarError::Duration(s.to_owned());
        let rest = s.strip_prefix('P').ok_or_else(bad)?;
        let (date, time) = match rest.split_once('T') {
            Some((_, "")) => return Err(bad()),
            Some((d, t)) => (d, t),
            None => (rest, ""),
        };
        if date.is_empty() && time.is_empty() {
            return Err(bad());
        }
        let mut out = Self::default();
        let parts = |text: &str, units: &[char]| -> Result<Vec<(char, u64)>, CalendarError> {
            let (mut num, mut seen, mut next) = (String::new(), Vec::new(), 0usize);
            for c in text.chars() {
                if c.is_ascii_digit() {
                    num.push(c);
                    continue;
                }
                let pos = units.iter().skip(next).position(|u| *u == c).ok_or_else(bad)? + next;
                seen.push((c, num.parse().map_err(|_| bad())?));
                num.clear();
                next = pos + 1;
            }
            if num.is_empty() { Ok(seen) } else { Err(bad()) }
        };
        for (u, n) in parts(date, &['W', 'D'])? {
            if u == 'W' { out.weeks = n } else { out.days = n }
        }
        for (u, n) in parts(time, &['H', 'M', 'S'])? {
            let mul = match u {
                'H' => 3_600,
                'M' => 60,
                _ => 1,
            };
            out.seconds = n.checked_mul(mul).and_then(|v| out.seconds.checked_add(v)).ok_or_else(bad)?;
        }
        Ok(out)
    }

    pub fn is_zero(&self) -> bool {
        self.weeks == 0 && self.days == 0 && self.seconds == 0
    }
}

/// Parses `Z`, `UTC` or `±HH:MM`.
pub fn parse_offset(s: &str) -> Result<UtcOffset, CalendarError> {
    let bad = || CalendarError::Offset(s.to_owned());
    if s == "Z" || s == "UTC" {
        return Ok(UtcOffset::UTC);
    }
    let (sign, rest) = match s.split_at_checked(1) {
        Some(("+", r)) => (1i8, r),
        Some(("-", r)) => (-1i8, r),
        _ => return Err(bad()),
    };
    let (h, m) = rest.split_once(':').ok_or_else(bad)?;
    if h.len() != 2 || m.len() != 2 {
        return Err(bad());
    }
    let h: i8 = h.parse().map_err(|_| bad())?;
    let m: i8 = m.parse().map_err(|_| bad())?;
    if !(0..=14).contains(&h) || !(0..60).contains(&m) {
        return Err(bad());
    }
    UtcOffset::from_hms(sign * h, sign * m, 0).map_err(|_| bad())
}

fn parse_hhmm(s: &str) -> Result<u32, CalendarError> {
    let bad = || CalendarError::Time(s.to_owned());
    let (h, m) = s.split_once(':').ok_or_else(bad)?;
    let (h, m): (u32, u32) = (h.parse().map_err(|_| bad())?, m.parse().map_err(|_| bad())?);
    let secs = h * 3_600 + m * 60;
    if m >= 60 || secs > DAY_SECS { Err(bad()) } else { Ok(secs) }
}

impl Calendar {
    pub fn from_spec(spec: &CalendarSpec) -> Result<Self, CalendarError> {
        let offset = parse_offset(&spec.timezone)?;
        let mut week: [Vec<(u32, u32)>; 7] = Default::default();
        for (day, ranges) in &spec.working_hours {
            let i =
                WEEKDAYS.iter().position(|d| d == day).ok_or_else(|| CalendarError::Weekday(day.clone()))?;
            for [s, e] in ranges {
                week[i].push((parse_hhmm(s)?, parse_hhmm(e)?));
            }
        }
        Self::new(offset, week, spec.holidays.iter().copied().collect())
    }

    pub fn new(
        offset: UtcOffset,
        week: [Vec<(u32, u32)>; 7],
        holidays: BTreeSet<Date>,
    ) -> Result<Self, CalendarError> {
        for (i, ranges) in week.iter().enumerate() {
            let mut prev_end = 0;
            for (n, &(s, e)) in ranges.iter().enumerate() {
                if s >= e || e > DAY_SECS || (n > 0 && s < prev_end) {
                    return Err(CalendarError::Ranges(WEEKDAYS[i].to_owned()));
                }
                prev_end = e;
            }
        }
        if week.iter().all(Vec::is_empty) {
            return Err(CalendarError::NoWorkingTime);
        }
        Ok(Self { offset, week, holidays })
    }

    pub fn offset(&self) -> UtcOffset {
        self.offset
    }

    fn weekly_secs(&self) -> u64 {
        self.week.iter().flatten().map(|&(s, e)| u64::from(e - s)).sum()
    }

    fn working_weekdays(&self) -> u64 {
        self.week.iter().filter(|r| !r.is_empty()).count() as u64
    }

    fn ranges(&self, d: Date) -> &[(u32, u32)] {
        if self.holidays.contains(&d) {
            &[]
        } else {
            &self.week[d.weekday().number_days_from_monday() as usize]
        }
    }

    fn at(&self, d: Date, secs: u32) -> OffsetDateTime {
        PrimitiveDateTime::new(d, Time::MIDNIGHT).assume_offset(self.offset)
            + Duration::seconds(i64::from(secs))
    }

    fn date_of(&self, t: OffsetDateTime) -> Date {
        t.to_offset(self.offset).date()
    }

    /// An upper bound on the days a walk may visit before it must have finished.
    fn day_bound(&self, working_days: u64, secs: u64) -> u64 {
        let weeks = secs / self.weekly_secs().max(1) + working_days / self.working_weekdays().max(1) + 2;
        weeks * 7 + self.holidays.len() as u64 + 7
    }

    /// Is `t` inside working time (`[start, end)` of a range on a non-holiday)?
    pub fn is_working(&self, t: OffsetDateTime) -> bool {
        let d = self.date_of(t);
        self.ranges(d).iter().any(|&(s, e)| self.at(d, s) <= t && t < self.at(d, e))
    }

    /// The first working instant at or after `t`.
    fn roll_forward(&self, t: OffsetDateTime) -> Option<OffsetDateTime> {
        let mut d = self.date_of(t);
        for _ in 0..self.day_bound(1, 0) {
            for &(s, e) in self.ranges(d) {
                if self.at(d, e) > t {
                    return Some(t.max(self.at(d, s)));
                }
            }
            d = d.next_day()?;
        }
        None
    }

    /// Adds `secs` of working time to `t`. Zero is the identity.
    pub fn add_working(&self, t: OffsetDateTime, secs: Duration) -> Result<OffsetDateTime, CalendarError> {
        if !secs.is_positive() {
            return Ok(t);
        }
        let mut remaining = secs;
        let mut cur = t;
        let mut d = self.date_of(t);
        let bound = self.day_bound(0, u64::try_from(secs.whole_seconds()).unwrap_or(u64::MAX));
        for _ in 0..bound {
            for &(s, e) in self.ranges(d) {
                let end = self.at(d, e);
                if end <= cur {
                    continue;
                }
                let from = cur.max(self.at(d, s));
                let avail = end - from;
                if remaining <= avail {
                    return Ok((from + remaining).to_offset(UtcOffset::UTC));
                }
                remaining -= avail;
                cur = end;
            }
            d = d.next_day().ok_or(CalendarError::Range)?;
        }
        Err(CalendarError::Range)
    }

    /// Moves `t` by `n` working days (see the module docs). Zero is the identity.
    pub fn add_days(&self, t: OffsetDateTime, n: u64) -> Result<OffsetDateTime, CalendarError> {
        if n == 0 {
            return Ok(t);
        }
        let r = self.roll_forward(t).ok_or(CalendarError::Range)?;
        let mut d = self.date_of(r);
        let tod = r - self.at(d, 0);
        let (mut k, mut walked) = (0, 0);
        while k < n {
            d = d.next_day().ok_or(CalendarError::Range)?;
            walked += 1;
            if walked > self.day_bound(n, 0) {
                return Err(CalendarError::Range);
            }
            if !self.ranges(d).is_empty() {
                k += 1;
            }
        }
        let ranges = self.ranges(d);
        for &(s, e) in ranges {
            if tod < Duration::seconds(i64::from(s)) {
                return Ok(self.at(d, s).to_offset(UtcOffset::UTC));
            }
            if tod < Duration::seconds(i64::from(e)) {
                return Ok((self.at(d, 0) + tod).to_offset(UtcOffset::UTC));
            }
        }
        let last = ranges.last().map_or(0, |r| r.1);
        Ok(self.at(d, last).to_offset(UtcOffset::UTC))
    }

    /// `calendar.add(start, duration)` for a business duration (docs/04 §4.4).
    pub fn add(&self, t: OffsetDateTime, dur: BusinessDuration) -> Result<OffsetDateTime, CalendarError> {
        let days = dur
            .weeks
            .checked_mul(self.working_weekdays())
            .and_then(|w| w.checked_add(dur.days))
            .ok_or(CalendarError::Range)?;
        let secs = i64::try_from(dur.seconds).map_err(|_| CalendarError::Range)?;
        let after_days = self.add_days(t, days)?;
        self.add_working(after_days, Duration::seconds(secs))
    }

    /// Working time in `[a, b)`; zero when `b <= a`.
    pub fn working_between(&self, a: OffsetDateTime, b: OffsetDateTime) -> Duration {
        if b <= a {
            return Duration::ZERO;
        }
        let mut total = Duration::ZERO;
        let mut d = self.date_of(a);
        let last = self.date_of(b);
        loop {
            for &(s, e) in self.ranges(d) {
                let (from, to) = (a.max(self.at(d, s)), b.min(self.at(d, e)));
                if to > from {
                    total += to - from;
                }
            }
            match d.next_day() {
                Some(n) if d < last => d = n,
                _ => return total,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use time::macros::{date, datetime};

    fn office() -> Calendar {
        let mut hours = BTreeMap::new();
        for d in ["mon", "tue", "wed", "thu", "fri"] {
            hours.insert(
                d.to_owned(),
                vec![["09:00".to_owned(), "13:00".to_owned()], ["14:00".to_owned(), "18:00".to_owned()]],
            );
        }
        let spec = CalendarSpec {
            timezone: "+05:30".into(),
            working_hours: hours,
            holidays: vec![date!(2026 - 10 - 20), date!(2026 - 12 - 25)],
        };
        Calendar::from_spec(&spec).unwrap()
    }

    fn ist(y_m_d_h_m: OffsetDateTime) -> OffsetDateTime {
        y_m_d_h_m.replace_offset(UtcOffset::from_hms(5, 30, 0).unwrap())
    }

    #[test]
    fn parses_durations_and_offsets() {
        assert_eq!(
            BusinessDuration::parse("P2D").unwrap(),
            BusinessDuration { weeks: 0, days: 2, seconds: 0 }
        );
        assert_eq!(BusinessDuration::parse("P1W2DT1H30M").unwrap().seconds, 5_400);
        assert_eq!(BusinessDuration::parse("PT4H").unwrap().seconds, 14_400);
        for bad in ["", "P", "PT", "P1M", "P1H", "PT1D", "P1D1W", "P2", "1D", "PT1H1H"] {
            assert!(BusinessDuration::parse(bad).is_err(), "{bad}");
        }
        assert_eq!(parse_offset("-03:30").unwrap(), UtcOffset::from_hms(-3, -30, 0).unwrap());
        assert_eq!(parse_offset("Z").unwrap(), UtcOffset::UTC);
        for bad in ["Asia/Kolkata", "+5:30", "+25:00", "05:30"] {
            assert!(parse_offset(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn rejects_bad_calendars() {
        let mk = |ranges: Vec<(u32, u32)>| {
            let mut w: [Vec<(u32, u32)>; 7] = Default::default();
            w[0] = ranges;
            Calendar::new(UtcOffset::UTC, w, BTreeSet::new())
        };
        assert_eq!(mk(vec![]), Err(CalendarError::NoWorkingTime));
        assert!(mk(vec![(10, 5)]).is_err());
        assert!(mk(vec![(0, 100), (50, 200)]).is_err());
        assert!(mk(vec![(0, DAY_SECS + 1)]).is_err());
        assert!(mk(vec![(0, DAY_SECS)]).is_ok());
    }

    #[test]
    fn working_hours_skip_lunch_nights_weekends_and_holidays() {
        let c = office();
        // Friday 16:00 IST + 3 working hours = Monday 10:00 IST (two hours on Friday, one on Monday).
        let fri = ist(datetime!(2026-10-16 16:00 UTC));
        assert_eq!(c.add_working(fri, Duration::hours(3)).unwrap(), ist(datetime!(2026-10-19 10:00 UTC)));
        // Monday 12:00 + 4 h crosses lunch: 13:00, then 14:00-17:00.
        let mon = ist(datetime!(2026-10-19 12:00 UTC));
        assert_eq!(c.add_working(mon, Duration::hours(4)).unwrap(), ist(datetime!(2026-10-19 17:00 UTC)));
        // Monday 17:00 + 2 h skips Tuesday 20 Oct (holiday): Wednesday 10:00.
        let late = ist(datetime!(2026-10-19 17:00 UTC));
        assert_eq!(c.add_working(late, Duration::hours(2)).unwrap(), ist(datetime!(2026-10-21 10:00 UTC)));
        // Exactly to the end of a range stays at the end.
        assert_eq!(c.add_working(mon, Duration::hours(1)).unwrap(), ist(datetime!(2026-10-19 13:00 UTC)));
    }

    #[test]
    fn working_days_keep_the_time_of_day() {
        let c = office();
        let fri = ist(datetime!(2026-10-16 15:00 UTC));
        assert_eq!(
            c.add(fri, BusinessDuration::parse("P1D").unwrap()).unwrap(),
            ist(datetime!(2026-10-19 15:00 UTC))
        );
        // Two working days from Monday skip the Tuesday holiday.
        let mon = ist(datetime!(2026-10-19 10:00 UTC));
        assert_eq!(
            c.add(mon, BusinessDuration::parse("P2D").unwrap()).unwrap(),
            ist(datetime!(2026-10-22 10:00 UTC))
        );
        // A Saturday start rolls to Monday 09:00 first.
        let sat = ist(datetime!(2026-10-17 11:00 UTC));
        assert_eq!(
            c.add(sat, BusinessDuration::parse("P1D").unwrap()).unwrap(),
            ist(datetime!(2026-10-21 09:00 UTC))
        );
        // P1W is five working days here.
        assert_eq!(
            c.add(mon, BusinessDuration::parse("P1W").unwrap()).unwrap(),
            c.add(mon, BusinessDuration::parse("P5D").unwrap()).unwrap()
        );
        // Days then working time: P1DT2H from Friday 17:00 = Monday 17:00 + 2 h = Tuesday... holiday → Wed 10:00.
        let fri_late = ist(datetime!(2026-10-16 17:00 UTC));
        assert_eq!(
            c.add(fri_late, BusinessDuration::parse("P1DT2H").unwrap()).unwrap(),
            ist(datetime!(2026-10-21 10:00 UTC))
        );
    }

    #[test]
    fn working_between_counts_only_working_time() {
        let c = office();
        let a = ist(datetime!(2026-10-16 16:00 UTC));
        let b = ist(datetime!(2026-10-19 11:00 UTC));
        assert_eq!(c.working_between(a, b), Duration::hours(4));
        assert_eq!(c.working_between(b, a), Duration::ZERO);
    }

    fn start() -> impl Strategy<Value = OffsetDateTime> {
        // Any minute in late 2026, across weekends and both holidays.
        (0i64..(120 * 24 * 60)).prop_map(|m| datetime!(2026-10-01 00:00 UTC) + Duration::minutes(m))
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]

        #[test]
        fn adding_zero_is_identity(t in start()) {
            let c = office();
            prop_assert_eq!(c.add(t, BusinessDuration::default()).unwrap(), t);
            prop_assert_eq!(c.add_working(t, Duration::ZERO).unwrap(), t);
        }

        #[test]
        fn monotonic_in_start_and_in_each_part(t in start(), dt in 0i64..10_000, d in 0u64..15, s in 0u64..200_000, ds in 0u64..50_000) {
            let c = office();
            let base = BusinessDuration { weeks: 0, days: d, seconds: s };
            let r = c.add(t, base).unwrap();
            prop_assert!(r >= t);
            prop_assert!(c.add(t + Duration::minutes(dt), base).unwrap() >= r);
            let more_time = BusinessDuration { seconds: s + ds, ..base };
            prop_assert!(c.add(t, more_time).unwrap() >= r);
            let (next_day, same_day) = (BusinessDuration { days: d + 1, ..base }, BusinessDuration { days: d, seconds: 0, weeks: 0 });
            prop_assert!(c.add(t, next_day).unwrap() > c.add(t, same_day).unwrap());
        }

        #[test]
        fn working_time_skips_non_working_time(t in start(), s in 1i64..400_000) {
            let c = office();
            let r = c.add_working(t, Duration::seconds(s)).unwrap();
            // Exactly `s` of working time lies between the start and the result…
            prop_assert_eq!(c.working_between(t, r), Duration::seconds(s));
            // …and the result is the end of working time, never inside a gap: just before it is working.
            prop_assert!(c.is_working(r - Duration::seconds(1)));
        }

        #[test]
        fn working_days_land_on_working_days(t in start(), d in 1u64..30) {
            let c = office();
            let r = c.add_days(t, d).unwrap();
            prop_assert!(!c.ranges(c.date_of(r)).is_empty());
            prop_assert!(r > t);
        }
    }
}
