//! BPMS-027 remainder: department overrides (layer 4), profile switch plan/apply (CFG-017) and the
//! effective-settings cache (CFG-004). Properties extend "never looser than the floor" to the
//! department layer; the database tests run against real Postgres (and Valkey when configured).

use procera_cache::{Cache, MemoryCache};
use procera_db::{WsCtx, test_support};
use procera_kernel::{SystemClock, UuidV7Gen};
use procera_settings::cache::{Outcome, SettingsCache};
use procera_settings::catalogue::{CATALOGUE, Def, Kind};
use procera_settings::{
    DeptOverride, Settings, SettingsError, at_least_as_strict, resolve, resolve_scoped, validate,
};
use proptest::prelude::*;
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

fn value_for(kind: Kind) -> BoxedStrategy<Value> {
    match kind {
        Kind::Bool => any::<bool>().prop_map(Value::Bool).boxed(),
        Kind::Int { min, max } | Kind::Duration { min, max } | Kind::Size { min, max } => {
            (min..=max).prop_map(|n| json!(n)).boxed()
        }
        Kind::Set { values, allow_all } => {
            let all = proptest::sample::subsequence(values.to_vec(), 0..=values.len()).prop_map(|v| json!(v));
            if allow_all { prop_oneof![9 => all, 1 => Just(json!(["*"]))].boxed() } else { all.boxed() }
        }
        _ => Just(Value::Null).boxed(),
    }
}

fn floor_bound(i: usize) -> &'static Def {
    CATALOGUE.iter().filter(|d| d.floor.is_some()).collect::<Vec<_>>()[i]
}

fn ids(n: usize) -> Vec<Uuid> {
    (0..n).map(|i| Uuid::from_u128(i as u128 + 1)).collect()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, failure_persistence: None, ..ProptestConfig::default() })]

    /// Department overrides never get past the floor. No floor-bound key is department-scopable
    /// today, so the property is checked as if each one were: resolution must hold regardless.
    #[test]
    fn department_overrides_are_never_looser_than_the_floor(
        (idx, floor, stored, chain) in (0..CATALOGUE.iter().filter(|d| d.floor.is_some()).count()).prop_flat_map(|i| {
            let d = floor_bound(i);
            (Just(i), value_for(d.kind), proptest::option::of(value_for(d.kind)), proptest::collection::vec(value_for(d.kind), 0..4))
        })
    ) {
        let base = floor_bound(idx);
        let d: &'static Def = Box::leak(Box::new(Def { department: true, ..*base }));
        let cmp = d.floor.expect("floor-bound");
        let floor = validate(d, &floor).expect("valid floor");
        let stored = stored.map(|v| validate(d, &v).expect("valid value"));
        let chain: Vec<Value> = chain.iter().map(|v| validate(d, v).expect("valid override")).collect();
        let path = ids(chain.len());
        let overrides: Vec<DeptOverride<'_>> = chain.iter().zip(&path).map(|(value, id)| DeptOverride { department_id: *id, value }).collect();
        let e = resolve_scoped(d, Some(&floor), stored.as_ref().map(|v| (v, "workspace", 1)), &overrides);
        prop_assert!(at_least_as_strict(cmp, &e.value, &floor), "{}: {} looser than floor {}", d.key, e.value, floor);
        if let Some(nearest) = chain.last() {
            if at_least_as_strict(cmp, nearest, &floor) {
                prop_assert_eq!(&e.value, nearest);
                prop_assert_eq!(e.source, "department");
            } else {
                prop_assert!(e.clamped);
                prop_assert_eq!(e.source, "platform_floor");
            }
            prop_assert_eq!(e.layers.iter().filter(|l| l.layer == "department").count(), chain.len());
        }
    }

    /// Overrides are ignored for keys the catalogue does not mark department-scopable.
    #[test]
    fn overrides_never_apply_to_keys_that_are_not_department_scopable(
        (idx, value) in (0..CATALOGUE.iter().filter(|d| !d.department).count()).prop_flat_map(|i| {
            let d = CATALOGUE.iter().filter(|d| !d.department).nth(i).expect("index in range");
            (Just(i), value_for(d.kind))
        })
    ) {
        let d = CATALOGUE.iter().filter(|d| !d.department).nth(idx).expect("index in range");
        let floor = (d.default)();
        let overrides = [DeptOverride { department_id: Uuid::from_u128(9), value: &value }];
        prop_assert_eq!(resolve_scoped(d, Some(&floor), None, &overrides), resolve(d, Some(&floor), None));
    }
}

/// A workspace on the Standard profile with one user, in a fresh database.
async fn fixture() -> anyhow::Result<Option<(test_support::TestDb, WsCtx, (Uuid, Uuid))>> {
    let Some(t) = test_support::fresh().await else { return Ok(None) };
    let t = t?;
    let ws = WsCtx { workspace_id: Uuid::now_v7() };
    let user = Uuid::now_v7();
    let mut admin = <sqlx::PgConnection as sqlx::Connection>::connect(&t.admin_url).await?;
    sqlx::query(
        "INSERT INTO users (id, email, display_name, subject_ref) VALUES ($1, 'd@acme.test', 'D', $2)",
    )
    .bind(user)
    .bind(Uuid::new_v4())
    .execute(&mut admin)
    .await?;
    let mut tx = t.db.begin_ws(ws).await?;
    svc().apply_profile(&mut tx, "standard").await?;
    tx.commit().await?;
    Ok(Some((t, ws, (user, Uuid::new_v4()))))
}

fn c() -> (Option<String>, Option<String>) {
    (None, None)
}

fn svc() -> Settings {
    Settings::new(Arc::new(SystemClock), Arc::new(UuidV7Gen))
}

async fn department(
    tx: &mut procera_db::WsTx<'_>,
    name: &str,
    parent: Option<(Uuid, Vec<Uuid>)>,
) -> anyhow::Result<(Uuid, Vec<Uuid>)> {
    let id = Uuid::now_v7();
    let (parent_id, mut path) = parent.map_or((None, vec![]), |(p, path)| (Some(p), path));
    path.push(id);
    sqlx::query(
        "INSERT INTO departments (workspace_id, id, parent_id, name, path) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(tx.ctx.workspace_id)
    .bind(id)
    .bind(parent_id)
    .bind(name)
    .bind(&path)
    .execute(tx.conn())
    .await?;
    Ok((id, path))
}

#[tokio::test]
async fn department_overrides_inherit_and_clear() -> anyhow::Result<()> {
    let Some((t, ws, actor)) = fixture().await? else { return Ok(()) };
    let s = svc();
    let tz = "locale.timezone";
    let mut tx = t.db.begin_ws(ws).await?;
    let (fin, fin_path) = department(&mut tx, "Finance", None).await?;
    let (pay, _) = department(&mut tx, "Payroll", Some((fin, fin_path))).await?;

    // Finance overrides; Payroll inherits it; Payroll's own override wins over its parent's.
    let f = s.set_department(&mut tx, actor, fin, tz, &json!("Europe/London"), None, Some(0), c()).await?;
    assert_eq!(
        (f.effective.value.clone(), f.effective.source, f.override_revision),
        (json!("Europe/London"), "department", 1)
    );
    let view = s.department_effective(&mut tx, pay).await?;
    let p = view.iter().find(|e| e.effective.key == tz).expect("key");
    assert_eq!(
        (p.effective.value.clone(), p.inherited_from, p.override_value.clone()),
        (json!("Europe/London"), Some(fin), None)
    );
    let p = s
        .set_department(&mut tx, actor, pay, tz, &json!("Europe/Paris"), Some("local office"), None, c())
        .await?;
    assert_eq!((p.effective.value.clone(), p.inherited_from), (json!("Europe/Paris"), None));
    assert_eq!(p.effective.layers.iter().filter(|l| l.layer == "department").count(), 2);

    // The catalogue decides what can be overridden; unknown departments and stale If-Match fail.
    assert!(matches!(
        s.set_department(&mut tx, actor, fin, "auth.session.idle_timeout", &json!(900), Some("x"), None, c())
            .await,
        Err(SettingsError::NotDepartmentScopable(_))
    ));
    assert!(matches!(
        s.set_department(&mut tx, actor, Uuid::now_v7(), tz, &json!("UTC"), None, None, c()).await,
        Err(SettingsError::UnknownDepartment)
    ));
    assert!(matches!(
        s.clear_department(&mut tx, actor, pay, tz, None, Some(7), c()).await,
        Err(SettingsError::StaleRevision { current: 1 })
    ));
    let cleared = s.clear_department(&mut tx, actor, pay, tz, None, Some(1), c()).await?;
    assert_eq!(
        (cleared.effective.value.clone(), cleared.inherited_from),
        (json!("Europe/London"), Some(fin))
    );
    assert!(matches!(
        s.clear_department(&mut tx, actor, pay, tz, None, None, c()).await,
        Err(SettingsError::NoOverride(_))
    ));
    // A new override after a clear continues the revision count, so the old If-Match is stale.
    let again = s.set_department(&mut tx, actor, pay, tz, &json!("Asia/Tokyo"), None, Some(0), c()).await?;
    assert_eq!(again.override_revision, 3);
    // The workspace level is unaffected.
    assert_eq!(s.effective(&mut tx, tz).await?.value, json!("UTC"));
    let h = s.history(&mut tx, tz).await?;
    assert_eq!(h.iter().filter(|r| r.layer == "department" && r.scope_id == Some(pay)).count(), 3);

    tx.commit().await?;
    let report = procera_audit::verify(&t.db, ws).await?;
    assert_eq!((report.first_broken, report.checked), (None, 4), "one event per set or clear");
    Ok(())
}

#[tokio::test]
async fn profile_switch_plans_then_applies() -> anyhow::Result<()> {
    let Some((t, ws, actor)) = fixture().await? else { return Ok(()) };
    let s = svc();
    let mut tx = t.db.begin_ws(ws).await?;
    // Profile switch: plan, refuse a stale plan, apply, and nothing left to change afterwards.
    let plan = s.plan_profile_switch(&mut tx, "standard", "regulated").await?;
    let idle = plan.items.iter().find(|i| i.key == "auth.session.idle_timeout").expect("idle in plan");
    assert_eq!((idle.current.clone(), idle.after.clone(), idle.clamped), (json!(1800), json!(900), false));
    assert!(matches!(
        s.apply_profile_switch(&mut tx, actor, "standard", "regulated", "audit finding", Some("stale"), c())
            .await,
        Err(SettingsError::PlanChanged { .. })
    ));
    assert!(matches!(
        s.apply_profile_switch(&mut tx, actor, "standard", "regulated", " ", Some(&plan.basis), c()).await,
        Err(SettingsError::ReasonRequired(_))
    ));
    let applied = s
        .apply_profile_switch(
            &mut tx,
            actor,
            "standard",
            "regulated",
            "audit finding",
            Some(&plan.basis),
            c(),
        )
        .await?;
    assert_eq!(applied.items.len(), plan.items.len());
    let e = s.effective(&mut tx, "auth.session.idle_timeout").await?;
    assert_eq!((e.value, e.source), (json!(900), "profile"));
    assert!(s.plan_profile_switch(&mut tx, "regulated", "regulated").await?.items.is_empty());

    tx.commit().await?;
    let report = procera_audit::verify(&t.db, ws).await?;
    assert_eq!((report.first_broken, report.checked), (None, 1), "one event for the whole switch");
    Ok(())
}

#[tokio::test]
async fn cached_snapshots_are_invalidated_by_generation() -> anyhow::Result<()> {
    let Some((t, ws, actor)) = fixture().await? else { return Ok(()) };
    let s = svc();
    // Cache: memory always; Valkey too when the suite provides one.
    let mut caches = vec![SettingsCache::new(Cache::Memory(MemoryCache::default()))];
    if let Ok(url) = std::env::var("PROCERA_TEST_CACHE_URL") {
        caches.push(SettingsCache::new(Cache::from_url(Some(&url)).await?));
    }
    for cache in caches {
        let mut tx = t.db.begin_ws(ws).await?;
        let (first, o1) = cache.snapshot(&mut tx).await?;
        let (_, o2) = cache.snapshot(&mut tx).await?;
        assert_eq!((o1, o2), (Outcome::Miss, Outcome::Hit));
        let week = first.effective(procera_settings::def("locale.week_start")?, &[]).value;
        let next = if week == json!("sun") { "mon" } else { "sun" };
        s.set_workspace(&mut tx, actor, "locale.week_start", &json!(next), None, None, c()).await?;
        tx.commit().await?;
        cache.invalidate_workspace(ws.workspace_id).await;
        let mut tx = t.db.begin_ws(ws).await?;
        let (fresh, o3) = cache.snapshot(&mut tx).await?;
        assert_eq!(o3, Outcome::Miss, "a change is never served from the old entry");
        assert_eq!(fresh.effective(procera_settings::def("locale.week_start")?, &[]).value, json!(next));
        cache.invalidate_floors().await;
        assert_eq!(cache.snapshot(&mut tx).await?.1, Outcome::Miss, "a floor change drops every workspace");
        tx.rollback().await?;
    }

    Ok(())
}
