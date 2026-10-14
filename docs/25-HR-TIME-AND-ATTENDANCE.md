# 25 — HR: time off, holidays, shifts and attendance

> Status: **accepted direction** (product owner, 2026-10-14). Release **0.5**, after the 0.4 integrations.
> Payroll calculation and releasing documents to people (payslips, letters) are the **future pipeline**
> (§11), not part of 0.5. Change process: docs/21 §17 rows HR-01..HR-09. Tracker: BPMS-222..231.

## 1. Why

Procera already runs onboarding, offboarding (`personLeave`), out-of-office delegation and an
`hr.leave-request` template. A leave form on its own doesn't hold up: people need to know their
balance, managers need to see who else is out, HR needs policies, holidays and attendance that agree
with each other. This document turns those pieces into one **time and attendance** area that:

- reuses what exists: the directory (manager, department, location), the process engine (approvals,
  delegation, SLAs, email approvals), audiences, settings layers, DataGrid exports, the audit chain;
- keeps every balance explainable: balances are a **ledger** of entries, never an edited number;
- treats leave reasons and leave types as **personal data**: colleagues see "Out", not "Sick".

Audience: companies from 10 to a few thousand people, any country. India-style needs (half days,
sandwich rule, restricted holidays, regularization, shifts) are first-class, not add-ons.

## 2. Scope of 0.5

| # | Slice | Tracker | In |
|---|---|---|---|
| 1 | Work locations, work weeks, holiday calendars | BPMS-222 | locations with IANA time zone and work week; holiday calendars per year with optional (restricted) holidays; engine business calendars follow them |
| 2 | Leave types, policies, ledger, balances | BPMS-223 | types (unit, paid, attachment rule, notice, negative limit, counts non-working days or not); policies with yearly allowance, accrual (up front / monthly), pro-rating, carry-over cap and expiry; assignment by audience; balances from an append-only ledger; audited admin adjustments; yearly rollover job |
| 3 | Leave requests | BPMS-224 | day counting (work week, holidays, half days), balance hold while pending, overlap check, approve → taken, reject/withdraw → release, cancel after approval (approval when already started), My time off page |
| 4 | Team calendar and org chart | BPMS-225 | who's out by team, department, location (month and week); holidays and shifts on it; org chart by manager; privacy rules (§6) |
| 5 | Shifts and rosters | BPMS-226 | shift patterns (start, end, break, crossing midnight), default shift per location or person, weekly rosters drafted and published (people notified), my schedule |
| 6 | Attendance | BPMS-227 | clock in/out on web and mobile web, imported punches (CSV and API) for devices; daily status computed from shift, leave, holidays and punches; late and early-leave grace |
| 7 | Regularization | BPMS-228 | a request to correct a day (missed punch, wrong time, on duty / work from home), manager approval, original punches kept |
| 8 | Reports and month close | BPMS-229 | attendance register (month grid), balances, leave taken, late arrivals, missing punches; lock a month; payroll-ready export (CSV/XLSX) |

Out of 0.5: payroll, payslips (§11); biometric device drivers (devices push through the punch API or
CSV import); geofencing; shift swaps; compensatory off; overtime pay (overtime minutes are reported,
not paid); encashment; hourly leave (unit `hour` is reserved in the schema, the UI offers days and half
days).

## 3. Data model (all workspace tables, forced RLS, composite keys)

```text
work_locations      id, name, timezone (IANA), work_week smallint[] (ISO weekday 1-7),
                    day_hours numeric (default 8), holiday_calendar_id, default_shift_id, archived_at
holiday_calendars   id, name, revision
holidays            calendar_id, date, name, optional bool          -- optional = restricted holiday
membership_hr       membership_id PK, employee_no, employment_type (full_time|part_time|contract|intern),
                    work_location_id, start_date, probation_end, leave_policy_id (override, null = by audience)
leave_types         id, key, name, colour, unit (day|half_day|hour), paid, needs_attachment_after_days,
                    min_notice_days, max_consecutive_days, allow_negative_days, counts_non_working_days,
                    sensitive bool (default true for sick/medical), archived_at
leave_policies      id, name, audience jsonb, priority, year_start_month, revision
leave_policy_rules  policy_id, leave_type_id, yearly_days, accrual (upfront|monthly), prorate bool,
                    probation_waits bool, carry_over_max_days, carry_over_expiry_months
leave_ledger        id, membership_id, leave_type_id, leave_year date, kind (grant|accrual|hold|release|
                    taken|cancelled|adjustment|carry_over|expiry), days numeric(6,2), request_id?,
                    reason?, created_by?, created_at                  -- append-only (trigger)
leave_requests      request_id PK, membership_id, leave_type_id, start_date, end_date, start_half
                    (full|second_half), end_half (full|first_half), days numeric(6,2), state
                    (pending|approved|rejected|withdrawn|cancelled), detail jsonb (per-day breakdown)
shift_patterns      id, name, start time, end time, break_minutes, colour, late_grace_minutes,
                    early_grace_minutes, archived_at
roster_entries      membership_id, date, shift_id | off, published_at?, revision
attendance_punches  id, membership_id, at timestamptz, direction (in|out), source (web|mobile|import|api|
                    regularization), device?, ip?, superseded_by?, created_by
attendance_days     membership_id, date, expected jsonb (shift or off/holiday/leave), first_in, last_out,
                    worked_minutes, late_minutes, early_minutes, status, computed_at, locked bool
attendance_periods  month date, locked_at?, locked_by?            -- month close
```

Rules that hold the model together:
- **Balance = sum of the ledger** for (person, type, leave year). Nothing stores a balance number. An
  admin "adjustment" is a ledger entry with a reason; a mistake is corrected with a counter-entry.
- A pending request writes a **hold** (negative); approval writes `taken` and a matching `release` of
  the hold; rejection or withdrawal writes `release`. So "available = balance − holds" is always
  explainable line by line.
- Accrual and the yearly rollover (carry-over, then expiry) are scheduler jobs; each run is
  idempotent per (person, type, period) through a unique key, so a re-run never doubles a grant.
- `attendance_days` is **derived**: recomputed from roster, leave, holidays and punches by a pure
  function (§5). Locked months are never recomputed; a change to a locked day needs the month reopened
  (audited).
- Punches are never edited or deleted: corrections add punches and mark the earlier ones
  `superseded_by`.

`memberships.location` (free text, used by audiences) stays. Migration creates a work location for each
distinct value and fills `membership_hr.work_location_id`; afterwards the directory edits the location
through the list, and `memberships.location` mirrors the location's name so audiences keep working.

## 4. Counting days

For a request from `start` to `end` with half-day flags, for the person's work location:
1. Each date is a **working day** when its ISO weekday is in the location's work week, it is not a
   (non-optional) holiday of the location's calendar, and the person's roster for that date is not `off`
   (a rostered shift on a weekend makes it a working day).
2. If the type `counts_non_working_days` (the "sandwich" rule some companies use), every date counts;
   otherwise only working days do.
3. `start_half = second_half` takes 0.5 off the first day; `end_half = first_half` takes 0.5 off the
   last; a single-day request may be either half, not both.
4. An optional holiday is a leave type of its own (`unit = day`, allowance e.g. 2 per year) whose
   requests may only fall on optional holidays.

The breakdown (date → counted, why) is stored in `leave_requests.detail` and shown to the requester,
the approver and HR. Day counting lives in a pure kernel module with property tests (counting is
monotonic in the range; a range of only non-working days counts 0 unless the sandwich rule applies;
splitting a range never changes the total).

## 5. Daily attendance status

Inputs for a person and a date: the expected day (rostered shift, else the default shift of the person or
location, else the work week with `day_hours`), approved leave (full or half), holidays, punches.

| Status | When |
|---|---|
| `holiday` / `weekly_off` | not a working day and no punches |
| `on_leave` / `half_leave` | approved leave covers the day / half of it |
| `present` | worked ≥ expected minutes − early grace (half-leave days need half) |
| `late` | present, first in after start + late grace (also shown on the day) |
| `half_day` | worked between half and full expected minutes |
| `absent` | a working day, no leave, no punches, and the day has ended |
| `missing_punch` | an `in` without an `out` (or the reverse) once the day has ended |
| `worked_off_day` | punches on a holiday or weekly off (reported as overtime minutes) |

Night shifts belong to the date they start on. The day "has ended" at shift end + 4 hours in the location's
time zone. The status function is pure and table-tested; a scheduler job computes yesterday's days for
every location after its local midnight and recomputes a day whenever its inputs change (punch, leave,
roster, regularization) while the month is open.

## 6. Who sees what

| Viewer | Leave dates | Leave type and reason | Attendance |
|---|---|---|---|
| The person | own | own | own |
| Colleagues (same team calendar) | "Out" / "Out (half day)" | no | no |
| Manager chain (direct and indirect reports) | yes | type yes; reason and attachment yes for direct reports | their reports |
| HR with `time_off:view_all` / `attendance:view_all` | everyone | yes (access-logged when `sensitive`) | everyone |
| Workspace admins | as HR (docs/12 admin model); sensitive reads are access-logged | | |

Sensitive types (sick, medical, parental by default) also appear as "Out" in notifications to
anyone but the person, their manager and HR. Webhook events carry ids and dates, never the type.

New permissions (docs/12 catalogue): `time_off:request`, `time_off:view_all`, `time_off:manage_policies`,
`time_off:adjust_balances`, `attendance:punch`, `attendance:view_all`,
`attendance:manage_rosters`, `attendance:regularize_any`, `attendance:close_period`. Locations and holiday calendars use the existing `calendar:manage` (reads: `directory:view`); HR records use `directory:manage`. Manager visibility
comes from the reporting line (a new `reports_to` relation evaluated in Cedar), not from a role.
Built-in role templates gain an **HR administrator** with all of the above; **Member** gets
`time_off:request` and `attendance:punch`.

## 7. Flows on the process engine

Leave requests, cancellations of approved leave and regularizations are **processes** (templates
`hr.leave-request` v2, `hr.leave-cancel`, `hr.attendance-regularization`), so delegation, reminders,
escalations, email approvals, the request timeline and audit come for free. The process calls typed
service tasks:
- `time_off.check` (on submit, synchronous): eligibility, notice, overlap, max consecutive, attachment
  rule, balance (with negative limit); a failure is a field error on the form, not a rejected request;
- `time_off.hold` / `.release` / `.take` (ledger entries in the same transaction as the step);
- `attendance.apply_regularization` (adds punches, supersedes the old ones, recomputes the day).

Approvals default to the manager (P2D business, as today), with an optional HR step for types over N
days. Companies change the process in the designer like any other.

## 8. Screens

- **Time off** (everyone): balances by type with "how it's calculated" (the ledger), upcoming and past
  requests, **Request time off** (a dedicated, calendar-first form: pick dates on a calendar that shows
  holidays and team absences, half-day toggles, live day count and balance after).
- **Team calendar** (everyone; scope by manager chain, department, location): month and week views,
  holidays, leave, shifts; "Out today" on Home.
- **My schedule and attendance**: this week's shifts, a clock in/out button (also on Home), my month with
  statuses, "Fix this day" (regularization).
- **People › Org chart**: tree by manager with search, department colours, open profile.
- **Admin › Time off**: leave types, policies (assignment by audience, preview "who gets this policy"),
  balances grid with **Adjust** (reason required), year-end preview and run.
- **Admin › Holidays and locations**: locations (time zone, work week, calendar), calendars by year,
  import from iCal/CSV, copy to next year.
- **Admin › Shifts and rosters**: shift patterns, roster grid by team and week (draft → publish), copy
  previous week.
- **Admin › Attendance**: today board (in, not in yet, on leave), register (month grid), exceptions
  (missing punches, absences without leave), close month, exports.

All follow docs/05 (rem units, React Aria, DataGrid for lists, axe clean, mobile-first for clock-in and
time off).

## 9. API (docs/08 gets the full rows with each slice)

```text
GET/POST/PATCH  /work-locations, /holiday-calendars/{id}/holidays (+ import), /holidays?from=&to=
GET/POST/PATCH  /leave-types, /leave-policies, GET /leave-policies/{id}/audience-preview
GET             /me/time-off (balances, ledger, requests), /members/{id}/time-off
POST            /members/{id}/time-off/adjustments {leave_type, days, reason}       (step-up)
POST            /time-off/quote {type, start, end, start_half, end_half} → days, breakdown, balance after
GET             /team-calendar?from=&to=&scope=team|department|location&id=
GET/POST/PATCH  /shift-patterns, /rosters?week=&scope= , POST /rosters/publish
POST            /attendance/punch {direction}; POST /attendance/punches/import (CSV); POST /attendance/punches (API, token scope attendance:import)
GET             /me/attendance?month=, /attendance/days?month=&scope=, /attendance/today
POST            /attendance/periods/{month}/close | /reopen                         (step-up)
GET             /reports/attendance-register?month= (DataGrid + CSV/XLSX)
```

Requests themselves go through `POST /requests` with the leave/regularization process (§7).

## 10. Audit, events, settings

Audit: `holiday_calendar.*`, `work_location.*`, `leave_type.*`, `leave_policy.*`,
`time_off.adjusted`, `time_off.year_closed`, `shift.*`, `roster.published`, `attendance.punched`
(batched per import), `attendance.regularized`, `attendance.period_closed` / `.reopened`, and access
logs for sensitive leave reads. Outbox/webhook events: `time_off.approved.v1`, `time_off.cancelled.v1`,
`roster.published.v1`, `attendance.period_closed.v1` (ids and dates only).

Settings (docs/20, category **Time & attendance**): `time_off.year_start_month`,
`time_off.show_type_to_team` (default off), `attendance.enabled`, `attendance.clock_in_sources`,
`attendance.require_note_when_late`, `attendance.regularization_window_days` (default 7),
`attendance.regularizations_per_month` (default 3), `attendance.ip_allowlist` (optional).

## 11. Future pipeline (not 0.5)

- **Payroll calculation** (BPMS-230): pay components and structures, pay groups, a monthly run that
  reads the closed attendance month and unpaid leave, statutory deductions by country as plug-ins,
  approval of the run, bank file export. Needs its own design doc and an ADR (money precision, country
  rules as data vs. code).
- **Releasing documents to people** (BPMS-231): payslips, offer and appointment letters, tax
  forms generated from templates per person, encrypted at rest, released in a batch with a notification,
  seen in **My documents**, acknowledgement or e-signature when needed (reuses BPMS-107 signing and the
  library's encryption). Useful even before payroll (letters).

## 12. Decisions and open questions

Decisions taken here (docs/22 A-51): ledger instead of balance columns; leave runs on the process
engine; colleagues see "Out" only; day counting per the person's work location; status is derived and
locked by month; manual adjustments need a reason and step-up; hours-based leave and overtime pay are
later.

Open (docs/22 OQ-17..OQ-19):
- OQ-17: should leave years follow the calendar year for everyone, or per policy (`year_start_month`
  per policy is in the model; the UI can hide it)?
- OQ-18: clock-in from a phone outside the office: allowed by default with the IP shown, or require an
  allow-listed network? (Proposal: allowed, IP recorded, optional allow-list setting.)
- OQ-19: when a holiday is added after leave was approved over that date, re-credit automatically
  (proposal: yes, with a ledger entry and a notification) or leave it to HR?

## 13. E2E (docs/23 pack R5-HR)

Each slice ships an API scenario and a browser scenario, e.g. E2E-R5-HR-01 (locations and holidays drive
day counting), HR-02 (policy assignment, accrual, adjustment with reason, ledger explains the balance),
HR-03 (request → hold → approve → taken; reject releases; half days; sandwich rule), HR-04 (team
calendar privacy: colleague sees Out, manager sees type), HR-05 (roster publish notifies, weekend shift
makes a working day), HR-06 (clock in late → `late`; missing punch), HR-07 (regularization approved
fixes the day, original kept), HR-08 (close month locks it; export matches the grid), plus UI-xx for each
screen with axe.
