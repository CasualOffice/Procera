//! BPMS-223 against a real database: monthly accrual, up-front grants pro-rated from the start date,
//! probation that delays entitlement, idempotent writes, adjustments, the year-end carry-over (cap,
//! lapse) and the expiry of carried days; the ledger refuses updates and deletes.

use std::sync::Arc;

use procera_db::{WsCtx, test_support};
use procera_hr::Hr;
use procera_hr::leave::{Balance, LeaveTypeInput, PolicyInput, Rule};
use procera_kernel::UuidV7Gen;
use procera_kernel::clock::ManualClock;
use serde_json::json;
use time::macros::{date, datetime};
use uuid::Uuid;

fn ty(key: &str) -> LeaveTypeInput {
    LeaveTypeInput {
        key: Some(key.into()),
        name: key.into(),
        colour: None,
        paid: true,
        limited: true,
        sensitive: false,
        half_days: true,
        needs_attachment_after_days: None,
        min_notice_days: 0,
        max_consecutive_days: None,
        negative_limit_days: 0.0,
        counts_non_working_days: false,
        optional_holidays_only: false,
        archived: None,
    }
}

fn of<'a>(b: &'a [Balance], key: &str) -> &'a Balance {
    b.iter().find(|x| x.leave_type["key"] == key).unwrap_or_else(|| panic!("no {key} balance"))
}

#[tokio::test]
async fn accrual_proration_probation_rollover_and_expiry() -> anyhow::Result<()> {
    let Some(t) = test_support::fresh().await else { return Ok(()) };
    let t = t?;
    let mut admin = <sqlx::PgConnection as sqlx::Connection>::connect(&t.admin_url).await?;
    let clock = Arc::new(ManualClock::new(datetime!(2025-06-15 09:00 UTC)));
    let hr = Hr::new(clock.clone(), Arc::new(UuidV7Gen));

    let ws = Uuid::now_v7();
    let mut users = vec![];
    for name in ["ann", "bo", "cy"] {
        let u = Uuid::now_v7();
        sqlx::query("INSERT INTO users (id, email, display_name, subject_ref) VALUES ($1, $2, $3, $4)")
            .bind(u)
            .bind(format!("{name}@acme.test"))
            .bind(name)
            .bind(Uuid::new_v4())
            .execute(&mut admin)
            .await?;
        users.push(u);
    }
    sqlx::query("INSERT INTO workspaces (id, slug, name, profile, state, created_by) VALUES ($1, 'acme', 'Acme', 'standard', 'active', $2)")
        .bind(ws)
        .bind(users[0])
        .execute(&mut admin)
        .await?;
    let mut tx = t.db.begin_ws(WsCtx { workspace_id: ws }).await?;
    // Ann started 1 January 2025, Bo on 1 October 2025, Cy on 1 March 2025 with probation to 1 September.
    let ms: Vec<Uuid> = (0..3).map(|_| Uuid::now_v7()).collect();
    for (i, (start, probation)) in [
        (date!(2025 - 01 - 01), None),
        (date!(2025 - 10 - 01), None),
        (date!(2025 - 03 - 01), Some(date!(2025 - 09 - 01))),
    ]
    .into_iter()
    .enumerate()
    {
        sqlx::query(
            "INSERT INTO memberships (workspace_id, id, user_id, status) VALUES ($1, $2, $3, 'active')",
        )
        .bind(ws)
        .bind(ms[i])
        .bind(users[i])
        .execute(tx.conn())
        .await?;
        sqlx::query("INSERT INTO membership_hr (workspace_id, membership_id, start_date, probation_end, updated_at) VALUES ($1, $2, $3, $4, now())")
            .bind(ws)
            .bind(ms[i])
            .bind(start)
            .bind(probation)
            .execute(tx.conn())
            .await?;
    }
    let (ann, bo, cy) = (ms[0], ms[1], ms[2]);

    let annual = hr.create_leave_type(&mut tx, &ty("annual")).await?;
    let sick = hr.create_leave_type(&mut tx, &ty("sick")).await?;
    let rule = |t: Uuid, d: f64, accrual: &str| Rule {
        leave_type_id: t,
        yearly_days: d,
        accrual: accrual.into(),
        prorate: true,
        probation_waits: true,
        carry_over_max_days: 0.0,
        carry_over_expiry_months: None,
    };
    hr.create_policy(
        &mut tx,
        &PolicyInput {
            name: "Everyone".into(),
            audience: json!({ "all": true, "departments": [], "groups": [], "roles": [] }),
            priority: None,
            year_start_month: 1,
            rules: vec![
                Rule {
                    carry_over_max_days: 5.0,
                    carry_over_expiry_months: Some(3),
                    ..rule(annual.id, 12.0, "monthly")
                },
                rule(sick.id, 12.0, "upfront"),
            ],
            archived: None,
        },
    )
    .await?;

    // Mid-June 2025: Ann has six months of annual leave (one a month) and all her sick leave.
    let (_, b) = hr.balances(&mut tx, ann, date!(2025 - 06 - 15), None).await?;
    assert_eq!((of(&b, "annual").granted, of(&b, "sick").granted), (6.0, 12.0));
    // Reading again writes nothing new.
    let (_, again) = hr.balances(&mut tx, ann, date!(2025 - 06 - 15), None).await?;
    assert_eq!(of(&again, "annual").entries.len(), 6);
    // Cy is still on probation: nothing yet.
    let (_, c) = hr.balances(&mut tx, cy, date!(2025 - 06 - 15), None).await?;
    assert_eq!((of(&c, "annual").granted, of(&c, "sick").granted), (0.0, 0.0));

    // October 2025: Bo started this month, so a quarter of the sick allowance and one month of
    // annual; Cy's entitlement counts from September (two months; sick pro-rated to four months).
    let (_, b) = hr.balances(&mut tx, bo, date!(2025 - 10 - 01), None).await?;
    assert_eq!((of(&b, "annual").granted, of(&b, "sick").granted), (1.0, 3.0));
    let (_, c) = hr.balances(&mut tx, cy, date!(2025 - 10 - 01), None).await?;
    assert_eq!((of(&c, "annual").granted, of(&c, "sick").granted), (2.0, 4.0));

    // Bo moves to an up-front policy mid-year: topped up to its target (12 a year, pro-rated to
    // three months = 3 days), never granted twice; moving back takes nothing away.
    let upfront = hr
        .create_policy(
            &mut tx,
            &PolicyInput {
                name: "Up front".into(),
                audience: json!({ "all": false, "departments": [], "groups": [], "roles": ["nobody"] }),
                priority: None,
                year_start_month: 1,
                rules: vec![rule(annual.id, 12.0, "upfront")],
                archived: None,
            },
        )
        .await?;
    hr.assign_policy(&mut tx, bo, Some(upfront.id)).await?;
    let (p, b) = hr.balances(&mut tx, bo, date!(2025 - 10 - 01), None).await?;
    assert_eq!((p.map(|p| p.via), of(&b, "annual").granted), (Some("assigned"), 3.0));
    hr.assign_policy(&mut tx, bo, None).await?;
    let (p, b) = hr.balances(&mut tx, bo, date!(2025 - 10 - 01), None).await?;
    assert_eq!((p.map(|p| p.via), of(&b, "annual").granted), (Some("audience"), 3.0));

    // An adjustment with a reason; days must be whole or half.
    hr.adjust(&mut tx, ann, annual.id, -1.0, "Took a day before Procera", bo, date!(2025 - 10 - 01)).await?;
    assert!(hr.adjust(&mut tx, ann, annual.id, 0.25, "x", bo, date!(2025 - 10 - 01)).await.is_err());
    assert!(hr.adjust(&mut tx, ann, annual.id, 1.0, "  ", bo, date!(2025 - 10 - 01)).await.is_err());

    // February 2026: 2025 closed with 11 annual days (12 − 1): 5 carry over, 6 lapse; two months of
    // 2026 accrued. Sick leave has no carry-over: all 12 lapse.
    let (_, b) = hr.balances(&mut tx, ann, date!(2026 - 02 - 10), None).await?;
    let a = of(&b, "annual");
    assert_eq!((a.granted, a.carried, a.available), (2.0, 5.0, 7.0));
    assert_eq!(a.year_start, date!(2026 - 01 - 01));
    let (_, last) = hr.balances(&mut tx, ann, date!(2026 - 02 - 10), Some(date!(2025 - 12 - 31))).await?;
    let a25 = of(&last, "annual");
    assert_eq!(
        (a25.granted, a25.adjusted, a25.expired, a25.carried, a25.available),
        (12.0, -1.0, 6.0, -5.0, 0.0)
    );
    assert_eq!(of(&last, "sick").available, 0.0);
    let s25 = of(&last, "sick");
    assert_eq!(s25.expired, 12.0);

    // 2 April 2026: the carried 5 days expire (none used); four months accrued.
    let (_, b) = hr.balances(&mut tx, ann, date!(2026 - 04 - 02), None).await?;
    let a = of(&b, "annual");
    assert_eq!((a.granted, a.carried, a.expired, a.available), (4.0, 5.0, 5.0, 4.0));
    // Idempotent again after the rollover and expiry.
    let (_, b2) = hr.balances(&mut tx, ann, date!(2026 - 04 - 02), None).await?;
    assert_eq!(of(&b2, "annual").entries.len(), a.entries.len());

    // The ledger is append-only.
    tx.commit().await?;
    let upd = sqlx::query("UPDATE leave_ledger SET halves = 100").execute(&mut admin).await;
    assert!(upd.is_err(), "ledger rows can't be changed");
    let del = sqlx::query("DELETE FROM leave_ledger").execute(&mut admin).await;
    assert!(del.is_err(), "ledger rows can't be deleted");
    clock.advance(time::Duration::days(1));
    Ok(())
}
