//! Counting leave days (docs/25 §4): which dates of a range are working days for a person, and how
//! many days a request takes, in half-day units so nothing is ever a float.
//!
//! A date is a working day when its ISO weekday is in the location's work week and it is not a
//! (non-optional) holiday, unless the person's roster says otherwise (a rostered shift makes any date
//! a working day; a rostered day off makes it not one). A leave type that `counts_non_working_days`
//! (the "sandwich" rule) counts every date. A request may start in the afternoon (`SecondHalf`) and
//! end at midday (`FirstHalf`); each takes half a day off that end when the day counts.

use std::collections::BTreeMap;

use serde::Serialize;
use time::{Date, Duration};

/// The longest range a single request may span.
pub const MAX_RANGE_DAYS: i64 = 366;

/// The days of the week a location works, as ISO weekdays (1 = Monday … 7 = Sunday).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkWeek(u8);

impl WorkWeek {
    /// Monday to Friday.
    pub const STANDARD: Self = Self(0b0011111);

    /// From ISO weekdays; `None` when empty or out of range.
    pub fn from_iso(days: &[i16]) -> Option<Self> {
        let mut bits = 0u8;
        for d in days {
            if !(1..=7).contains(d) {
                return None;
            }
            bits |= 1 << (d - 1);
        }
        (bits != 0).then_some(Self(bits))
    }

    pub fn iso_days(self) -> Vec<i16> {
        (1..=7).filter(|d| self.0 & (1 << (d - 1)) != 0).collect()
    }

    pub fn works(self, date: Date) -> bool {
        self.0 & (1 << (date.weekday().number_from_monday() - 1)) != 0
    }
}

/// Which part of the first or last day a request covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Half {
    /// The whole day.
    #[default]
    Full,
    /// The morning only (valid for the last day).
    FirstHalf,
    /// The afternoon only (valid for the first day).
    SecondHalf,
}

/// Why a date counts or doesn't.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Why {
    Working,
    WeeklyOff,
    Holiday { name: String },
    RosteredOff,
    RosteredShift,
}

/// One date of a quote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Line {
    #[serde(with = "crate::dates::iso")]
    pub date: Date,
    pub why: Why,
    /// Half-days this date takes (0, 1 or 2).
    pub halves: u8,
}

/// A counted range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Quote {
    /// Half-days in total; `days()` is this divided by two.
    pub halves: u32,
    pub lines: Vec<Line>,
}

impl Quote {
    /// The total in days (multiples of 0.5).
    pub fn days(&self) -> f64 {
        f64::from(self.halves) / 2.0
    }
}

/// What the counter knows about a person's dates.
#[derive(Debug, Clone, Default)]
pub struct Calendar {
    pub week: Option<WorkWeek>,
    /// Non-optional holidays by date.
    pub holidays: BTreeMap<Date, String>,
    /// The person's roster where it overrides the week: `true` = a shift, `false` = a day off.
    pub roster: BTreeMap<Date, bool>,
}

impl Calendar {
    pub fn why(&self, date: Date) -> Why {
        match self.roster.get(&date) {
            Some(true) => return Why::RosteredShift,
            Some(false) => return Why::RosteredOff,
            None => {}
        }
        if let Some(name) = self.holidays.get(&date) {
            return Why::Holiday { name: name.clone() };
        }
        if self.week.unwrap_or(WorkWeek::STANDARD).works(date) { Why::Working } else { Why::WeeklyOff }
    }

    pub fn is_working(&self, date: Date) -> bool {
        matches!(self.why(date), Why::Working | Why::RosteredShift)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RangeError {
    #[error("the last day is before the first")]
    Reversed,
    #[error("a request can span at most {MAX_RANGE_DAYS} days")]
    TooLong,
    #[error("a one-day request can be a morning or an afternoon, not both")]
    BothHalves,
    #[error("the first day can only start in the afternoon, and the last can only end at midday")]
    WrongHalf,
}

/// Counts a request from `start` (whole day or afternoon) to `end` (whole day or morning).
pub fn quote(
    cal: &Calendar,
    start: Date,
    start_half: Half,
    end: Date,
    end_half: Half,
    counts_non_working_days: bool,
) -> Result<Quote, RangeError> {
    if end < start {
        return Err(RangeError::Reversed);
    }
    if (end - start).whole_days() >= MAX_RANGE_DAYS {
        return Err(RangeError::TooLong);
    }
    if start_half == Half::FirstHalf || end_half == Half::SecondHalf {
        return Err(RangeError::WrongHalf);
    }
    if start == end && start_half == Half::SecondHalf && end_half == Half::FirstHalf {
        return Err(RangeError::BothHalves);
    }
    let mut lines = Vec::new();
    let mut halves = 0u32;
    let mut d = start;
    loop {
        let why = cal.why(d);
        let counts = counts_non_working_days || matches!(why, Why::Working | Why::RosteredShift);
        let mut h: u8 = if counts { 2 } else { 0 };
        if counts && d == start && start_half == Half::SecondHalf {
            h -= 1;
        }
        if counts && d == end && end_half == Half::FirstHalf {
            h -= 1;
        }
        halves += u32::from(h);
        lines.push(Line { date: d, why, halves: h });
        if d == end {
            break;
        }
        d += Duration::days(1);
    }
    Ok(Quote { halves, lines })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use time::macros::date;

    fn cal() -> Calendar {
        let mut c = Calendar { week: Some(WorkWeek::STANDARD), ..Default::default() };
        c.holidays.insert(date!(2026 - 10 - 02), "Gandhi Jayanti".into()); // a Friday
        c
    }

    #[test]
    fn weekends_and_holidays_dont_count() {
        // Thu 1 Oct to Tue 6 Oct: Thu, (Fri holiday), (Sat, Sun), Mon, Tue = 3 days.
        let q = quote(&cal(), date!(2026 - 10 - 01), Half::Full, date!(2026 - 10 - 06), Half::Full, false)
            .unwrap();
        assert_eq!(q.days(), 3.0);
        assert_eq!(q.lines[1].why, Why::Holiday { name: "Gandhi Jayanti".into() });
        assert_eq!(q.lines[2].why, Why::WeeklyOff);
    }

    #[test]
    fn sandwich_rule_counts_every_date() {
        let q = quote(&cal(), date!(2026 - 10 - 01), Half::Full, date!(2026 - 10 - 06), Half::Full, true)
            .unwrap();
        assert_eq!(q.days(), 6.0);
    }

    #[test]
    fn half_days() {
        let c = cal();
        let d = date!(2026 - 10 - 05);
        assert_eq!(quote(&c, d, Half::SecondHalf, d, Half::Full, false).unwrap().days(), 0.5);
        assert_eq!(quote(&c, d, Half::Full, d, Half::FirstHalf, false).unwrap().days(), 0.5);
        assert_eq!(quote(&c, d, Half::SecondHalf, d, Half::FirstHalf, false), Err(RangeError::BothHalves));
        assert_eq!(quote(&c, d, Half::FirstHalf, d, Half::Full, false), Err(RangeError::WrongHalf));
        // Mon afternoon to Wed morning = 0.5 + 1 + 0.5.
        assert_eq!(
            quote(&c, d, Half::SecondHalf, date!(2026 - 10 - 07), Half::FirstHalf, false).unwrap().days(),
            2.0
        );
        // A half on a day that doesn't count takes nothing off.
        assert_eq!(
            quote(&c, date!(2026 - 10 - 03), Half::SecondHalf, d, Half::Full, false).unwrap().days(),
            1.0
        );
    }

    #[test]
    fn roster_overrides_week_and_holidays() {
        let mut c = cal();
        c.roster.insert(date!(2026 - 10 - 03), true); // Saturday shift
        c.roster.insert(date!(2026 - 10 - 05), false); // Monday off
        let q =
            quote(&c, date!(2026 - 10 - 03), Half::Full, date!(2026 - 10 - 05), Half::Full, false).unwrap();
        assert_eq!(q.days(), 1.0);
        assert_eq!(q.lines[0].why, Why::RosteredShift);
        assert_eq!(q.lines[2].why, Why::RosteredOff);
    }

    #[test]
    fn ranges_are_checked() {
        let c = cal();
        assert_eq!(
            quote(&c, date!(2026 - 10 - 05), Half::Full, date!(2026 - 10 - 04), Half::Full, false),
            Err(RangeError::Reversed)
        );
        assert_eq!(
            quote(&c, date!(2026 - 01 - 01), Half::Full, date!(2027 - 01 - 02), Half::Full, false),
            Err(RangeError::TooLong)
        );
    }

    #[test]
    fn work_week_round_trips() {
        assert_eq!(WorkWeek::from_iso(&[1, 2, 3, 4, 5]), Some(WorkWeek::STANDARD));
        assert_eq!(WorkWeek::from_iso(&[6, 7]).unwrap().iso_days(), vec![6, 7]);
        assert_eq!(WorkWeek::from_iso(&[]), None);
        assert_eq!(WorkWeek::from_iso(&[0]), None);
        assert_eq!(WorkWeek::from_iso(&[8]), None);
    }

    fn arb_cal() -> impl Strategy<Value = Calendar> {
        (
            1u8..128,
            proptest::collection::vec(0i64..120, 0..15),
            proptest::collection::vec((0i64..120, any::<bool>()), 0..10),
        )
            .prop_map(|(bits, hol, ros)| {
                let base = date!(2026 - 01 - 01);
                let mut c = Calendar { week: Some(WorkWeek(bits)), ..Default::default() };
                for h in hol {
                    c.holidays.insert(base + Duration::days(h), "H".into());
                }
                for (r, on) in ros {
                    c.roster.insert(base + Duration::days(r), on);
                }
                c
            })
    }

    proptest! {
        /// Splitting a whole-day range anywhere never changes the total.
        #[test]
        fn splitting_keeps_the_total(c in arb_cal(), a in 0i64..120, len in 1i64..60, cut in 0i64..60, sandwich: bool) {
            let base = date!(2026 - 01 - 01);
            let (s, e) = (base + Duration::days(a), base + Duration::days(a + len));
            let m = s + Duration::days(cut % len);
            let whole = quote(&c, s, Half::Full, e, Half::Full, sandwich).unwrap().halves;
            let left = quote(&c, s, Half::Full, m, Half::Full, sandwich).unwrap().halves;
            let right = quote(&c, m + Duration::days(1), Half::Full, e, Half::Full, sandwich).unwrap().halves;
            prop_assert_eq!(whole, left + right);
        }

        /// Extending a range never lowers its count, and a count never exceeds two halves a date.
        #[test]
        fn monotonic_and_bounded(c in arb_cal(), a in 0i64..120, len in 0i64..60, more in 1i64..10, sandwich: bool) {
            let base = date!(2026 - 01 - 01);
            let s = base + Duration::days(a);
            let q = quote(&c, s, Half::Full, s + Duration::days(len), Half::Full, sandwich).unwrap();
            let longer = quote(&c, s, Half::Full, s + Duration::days(len + more), Half::Full, sandwich).unwrap();
            prop_assert!(longer.halves >= q.halves);
            prop_assert!(q.halves <= 2 * (len as u32 + 1));
            if sandwich { prop_assert_eq!(q.halves, 2 * (len as u32 + 1)); }
            // Every counted line is a working day unless the sandwich rule applies.
            for l in &q.lines {
                prop_assert!(sandwich || l.halves == 0 || c.is_working(l.date));
            }
        }

        /// Halves only ever take off half a day at each end that counts.
        #[test]
        fn halves_take_at_most_one(c in arb_cal(), a in 0i64..120, len in 1i64..30) {
            let base = date!(2026 - 01 - 01);
            let (s, e) = (base + Duration::days(a), base + Duration::days(a + len));
            let full = quote(&c, s, Half::Full, e, Half::Full, false).unwrap().halves;
            let halved = quote(&c, s, Half::SecondHalf, e, Half::FirstHalf, false).unwrap().halves;
            let ends = u32::from(c.is_working(s)) + u32::from(c.is_working(e));
            prop_assert_eq!(full - halved, ends);
        }
    }
}
