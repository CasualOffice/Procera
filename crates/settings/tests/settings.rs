//! BPMS-027: floors, profiles, resolution and history against a real database, plus the
//! "never looser than the floor" property over every floor-bound catalogue key.

use procera_db::{SYSTEM_WORKSPACE, WsCtx, test_support};
use procera_kernel::{SystemClock, UuidV7Gen};
use procera_settings::catalogue::{CATALOGUE, Comparator, Kind};
use procera_settings::{Settings, SettingsError, at_least_as_strict, resolve, validate};
use proptest::prelude::*;
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

fn svc() -> Settings {
    Settings::new(Arc::new(SystemClock), Arc::new(UuidV7Gen))
}

/// A random valid value for a floor-bound key.
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

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, failure_persistence: None, ..ProptestConfig::default() })]

    /// For every floor-bound key: the resolved value is never looser than the floor, whatever was
    /// stored, and a looser stored value is reported as clamped.
    #[test]
    fn effective_value_is_never_looser_than_the_floor(
        (idx, floor, stored) in (0..CATALOGUE.iter().filter(|d| d.floor.is_some()).count()).prop_flat_map(|i| {
            let d = CATALOGUE.iter().filter(|d| d.floor.is_some()).nth(i).expect("index in range");
            (Just(i), value_for(d.kind), proptest::option::of(value_for(d.kind)))
        })
    ) {
        let d = CATALOGUE.iter().filter(|d| d.floor.is_some()).nth(idx).expect("index in range");
        let cmp = d.floor.expect("floor-bound");
        let floor = validate(d, &floor).expect("generated floor is valid");
        let stored = stored.map(|v| validate(d, &v).expect("generated value is valid"));
        let e = resolve(d, Some(&floor), stored.as_ref().map(|v| (v, "workspace", 1)));
        prop_assert!(at_least_as_strict(cmp, &e.value, &floor), "{}: {} looser than floor {}", d.key, e.value, floor);
        if let Some(v) = &stored {
            prop_assert_eq!(e.clamped, !at_least_as_strict(cmp, v, &floor));
            if !e.clamped { prop_assert_eq!(&e.value, v); }
        }
    }

    /// Comparators are reflexive: a value is always at least as strict as itself.
    #[test]
    fn comparators_are_reflexive(n in 0i64..1_000_000, b in any::<bool>()) {
        for cmp in [Comparator::Smaller, Comparator::Larger] { prop_assert!(at_least_as_strict(cmp, &json!(n), &json!(n))); }
        prop_assert!(at_least_as_strict(Comparator::True, &json!(b), &json!(b)));
    }
}

#[tokio::test]
async fn profiles_floors_clamping_and_history() -> anyhow::Result<()> {
    let Some(t) = test_support::fresh().await else { return Ok(()) };
    let t = t?;
    let s = svc();
    let ws = WsCtx { workspace_id: Uuid::now_v7() };
    let user = Uuid::now_v7();
    let mut admin = <sqlx::PgConnection as sqlx::Connection>::connect(&t.admin_url).await?;
    sqlx::query(
        "INSERT INTO users (id, email, display_name, subject_ref) VALUES ($1, 'a@acme.test', 'A', $2)",
    )
    .bind(user)
    .bind(Uuid::new_v4())
    .execute(&mut admin)
    .await?;
    let actor = (user, Uuid::new_v4());
    let c = (None, None);

    let mut tx = t.db.begin_ws(ws).await?;
    s.apply_profile(&mut tx, "starter").await?;
    let idle = s.effective(&mut tx, "auth.session.idle_timeout").await?;
    assert_eq!((idle.value.clone(), idle.source), (json!(28_800), "profile"));
    assert_eq!(s.effective(&mut tx, "locale.week_start").await?.source, "default");

    // Looser than the platform floor (8 h) is rejected; stricter works; ⚡ keys need a reason.
    assert!(matches!(
        s.set_workspace(
            &mut tx,
            actor,
            "auth.session.idle_timeout",
            &json!(86_400),
            Some("longer"),
            None,
            c.clone()
        )
        .await,
        Err(SettingsError::BelowFloor { .. })
    ));
    assert!(matches!(
        s.set_workspace(&mut tx, actor, "auth.session.idle_timeout", &json!(900), None, None, c.clone())
            .await,
        Err(SettingsError::ReasonRequired(_))
    ));
    let e = s
        .set_workspace(
            &mut tx,
            actor,
            "auth.session.idle_timeout",
            &json!(3_600),
            Some("policy"),
            Some(1),
            c.clone(),
        )
        .await?;
    assert_eq!((e.value, e.source, e.revision), (json!(3_600), "workspace", 2));
    assert!(matches!(
        s.set_workspace(
            &mut tx,
            actor,
            "auth.session.idle_timeout",
            &json!(1_800),
            Some("x"),
            Some(1),
            c.clone()
        )
        .await,
        Err(SettingsError::StaleRevision { current: 2 })
    ));
    assert!(matches!(
        s.set_workspace(
            &mut tx,
            actor,
            "auth.password.breach_check",
            &json!(false),
            Some("x"),
            None,
            c.clone()
        )
        .await,
        Err(SettingsError::Locked(_))
    ));
    assert!(matches!(
        s.set_workspace(&mut tx, actor, "branding.accent", &json!("blue"), None, None, c.clone()).await,
        Err(SettingsError::Invalid { .. })
    ));
    let accent =
        s.set_workspace(&mut tx, actor, "branding.accent", &json!("#0f766e"), None, None, c.clone()).await?;
    assert_eq!(accent.value, json!("#0F766E"));
    tx.commit().await?;

    // Raising the floor below the stored value clamps it at resolution time without rewriting it.
    let mut sys = t.db.begin_system().await?;
    s.set_floor(
        &mut sys,
        Uuid::new_v4(),
        user,
        "auth.session.idle_timeout",
        &json!(1_200),
        "tighten",
        c.clone(),
    )
    .await?;
    assert!(
        s.platform_values(&mut sys)
            .await?
            .iter()
            .any(|p| p.key == "auth.session.idle_timeout" && p.value == json!(1_200))
    );
    sys.commit().await?;
    let mut tx = t.db.begin_ws(ws).await?;
    let e = s.effective(&mut tx, "auth.session.idle_timeout").await?;
    assert_eq!((e.value, e.source, e.clamped), (json!(1_200), "platform_floor", true));
    let layers: Vec<&str> = e.layers.iter().map(|l| l.layer).collect();
    assert_eq!(layers, vec!["default", "platform_floor", "workspace"]);
    let h = s.history(&mut tx, "auth.session.idle_timeout").await?;
    assert_eq!(h.iter().map(|r| r.layer.as_str()).collect::<Vec<_>>(), vec!["workspace", "profile"]);
    assert_eq!(s.effective_all(&mut tx).await?.len(), CATALOGUE.len());
    tx.rollback().await?;

    // The platform chain records the floor change and the workspace chain the setting changes.
    let report = procera_audit::verify(&t.db, WsCtx { workspace_id: SYSTEM_WORKSPACE }).await?;
    assert_eq!(report.first_broken, None);
    let report = procera_audit::verify(&t.db, ws).await?;
    assert_eq!((report.first_broken, report.checked), (None, 2));
    Ok(())
}
