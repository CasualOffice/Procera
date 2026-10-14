//! Reading holidays from text (docs/25 §2.1): an iCal file (`BEGIN:VCALENDAR`, all-day `VEVENT`s,
//! multi-day events become one holiday per day) or CSV lines `date,name[,optional]` with ISO dates,
//! an optional header row and quoted names. Lines that can't be read are reported, not fatal.

use serde::Serialize;
use time::{Date, Duration, format_description::well_known::Iso8601};

use crate::Holiday;

/// A line that wasn't imported, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Skipped {
    /// 1-based line number in the input (0 when not from a line).
    pub line: u32,
    pub text: String,
    pub reason: String,
}

/// The longest event an iCal import expands into daily holidays.
const MAX_EVENT_DAYS: i64 = 14;

pub fn parse(text: &str) -> (Vec<Holiday>, Vec<Skipped>) {
    if text.trim_start().to_ascii_uppercase().starts_with("BEGIN:VCALENDAR") { ical(text) } else { csv(text) }
}

fn skip(line: usize, text: &str, reason: &str) -> Skipped {
    Skipped { line: line as u32 + 1, text: text.chars().take(120).collect(), reason: reason.into() }
}

fn iso_date(s: &str) -> Option<Date> {
    Date::parse(s.trim(), &Iso8601::DATE).ok().filter(|_| s.trim().len() == 10)
}

fn truthy(s: &str) -> Option<bool> {
    match s.trim().to_ascii_lowercase().as_str() {
        "" | "no" | "false" | "0" | "n" => Some(false),
        "yes" | "true" | "1" | "y" | "optional" | "restricted" => Some(true),
        _ => None,
    }
}

/// Splits one CSV line (commas, double-quoted fields with `""` escapes).
fn fields(line: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                chars.next();
                if let Some(f) = out.last_mut() {
                    f.push('"');
                }
            }
            ('"', _) => quoted = !quoted,
            (',', false) => out.push(String::new()),
            (c, _) => {
                if let Some(f) = out.last_mut() {
                    f.push(c);
                }
            }
        }
    }
    out.into_iter().map(|f| f.trim().to_string()).collect()
}

fn csv(text: &str) -> (Vec<Holiday>, Vec<Skipped>) {
    let mut found = Vec::new();
    let mut skipped = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let f = fields(line);
        let Some(date) = iso_date(&f[0]) else {
            if i == 0 && f[0].eq_ignore_ascii_case("date") {
                continue; // header
            }
            skipped.push(skip(i, line, "The date must look like 2026-10-02."));
            continue;
        };
        let name = f.get(1).map(|s| s.as_str()).unwrap_or("");
        if name.is_empty() {
            skipped.push(skip(i, line, "The holiday needs a name."));
            continue;
        }
        let Some(optional) = truthy(f.get(2).map(|s| s.as_str()).unwrap_or("")) else {
            skipped.push(skip(i, line, "The third column is optional: yes or no."));
            continue;
        };
        found.push(Holiday { date, name: name.into(), optional });
    }
    (found, skipped)
}

/// Unfolds iCal content lines (a line starting with a space or tab continues the previous one).
fn unfold(text: &str) -> Vec<(usize, String)> {
    let mut out: Vec<(usize, String)> = Vec::new();
    for (i, l) in text.lines().enumerate() {
        let l = l.trim_end_matches('\r');
        if let Some((_, prev)) = out.last_mut().filter(|_| l.starts_with(' ') || l.starts_with('\t')) {
            prev.push_str(&l[1..]);
        } else {
            out.push((i, l.to_string()));
        }
    }
    out
}

fn unescape(v: &str) -> String {
    v.replace("\\n", " ").replace("\\N", " ").replace("\\,", ",").replace("\\;", ";").replace("\\\\", "\\")
}

/// `DTSTART;VALUE=DATE:20261002` or `DTSTART:20261002T000000Z` → the date.
fn ical_date(v: &str) -> Option<Date> {
    let d = v.get(..8)?;
    if !d.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    iso_date(&format!("{}-{}-{}", &d[..4], &d[4..6], &d[6..8]))
}

fn ical(text: &str) -> (Vec<Holiday>, Vec<Skipped>) {
    let mut found = Vec::new();
    let mut skipped = Vec::new();
    let mut ev: Option<(usize, Option<Date>, Option<Date>, String, bool)> = None;
    for (i, line) in unfold(text) {
        let (key, value) = match line.split_once(':') {
            Some((k, v)) => (k.to_ascii_uppercase(), v),
            None => continue,
        };
        let prop = key.split(';').next().unwrap_or("");
        match (prop, &mut ev) {
            ("BEGIN", None) if value.eq_ignore_ascii_case("VEVENT") => {
                ev = Some((i, None, None, String::new(), false))
            }
            ("DTSTART", Some(e)) => e.1 = ical_date(value),
            ("DTEND", Some(e)) => e.2 = ical_date(value),
            ("SUMMARY", Some(e)) => e.3 = unescape(value).split_whitespace().collect::<Vec<_>>().join(" "),
            ("CATEGORIES", Some(e)) | ("X-OPTIONAL", Some(e)) => {
                let v = value.to_ascii_lowercase();
                e.4 |= v.contains("optional") || v.contains("restricted") || v == "true";
            }
            ("END", Some(_)) if value.eq_ignore_ascii_case("VEVENT") => {
                let Some((at, start, end, name, optional)) = ev.take() else { continue };
                let Some(start) = start else {
                    skipped.push(skip(at, &name, "The event has no date."));
                    continue;
                };
                if name.is_empty() {
                    skipped.push(skip(at, &start.to_string(), "The event has no name."));
                    continue;
                }
                // DTEND is exclusive for all-day events; absent or equal means one day.
                let days = end.map(|e| (e - start).whole_days()).filter(|d| *d > 1).unwrap_or(1);
                if days > MAX_EVENT_DAYS {
                    skipped.push(skip(at, &name, "The event is longer than two weeks."));
                    continue;
                }
                for d in 0..days {
                    found.push(Holiday { date: start + Duration::days(d), name: name.clone(), optional });
                }
            }
            _ => {}
        }
    }
    (found, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn csv_with_header_quotes_and_optional() {
        let (h, s) = parse(
            "date,name,optional\n2026-10-02,Gandhi Jayanti\n2026-11-08,\"Diwali, day 1\",yes\n\n# comment\n02/10/2026,Bad\n2026-12-25,\n2026-12-31,Eve,maybe",
        );
        assert_eq!(
            h,
            vec![
                Holiday { date: date!(2026 - 10 - 02), name: "Gandhi Jayanti".into(), optional: false },
                Holiday { date: date!(2026 - 11 - 08), name: "Diwali, day 1".into(), optional: true },
            ]
        );
        assert_eq!(s.iter().map(|x| x.line).collect::<Vec<_>>(), vec![6, 7, 8]);
    }

    #[test]
    fn ical_all_day_multi_day_and_folded() {
        let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nDTSTART;VALUE=DATE:20261002\r\nDTEND;VALUE=DATE:20261003\r\nSUMMARY:Gandhi\r\n  Jayanti\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nDTSTART;VALUE=DATE:20261224\r\nDTEND;VALUE=DATE:20261227\r\nSUMMARY:Winter break\\, office closed\r\nCATEGORIES:Optional\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nSUMMARY:No date\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let (h, s) = parse(ics);
        assert_eq!(h.len(), 4);
        assert_eq!(
            h[0],
            Holiday { date: date!(2026 - 10 - 02), name: "Gandhi Jayanti".into(), optional: false }
        );
        assert_eq!(
            h[3],
            Holiday {
                date: date!(2026 - 12 - 26),
                name: "Winter break, office closed".into(),
                optional: true
            }
        );
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].reason, "The event has no date.");
    }
}
