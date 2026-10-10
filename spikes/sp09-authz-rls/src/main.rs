//! SP-09: Cedar + Postgres RLS + sqlx pool (ADR-0011, docs/12 §12, docs/03 §7). Throwaway spike.
//! Needs a scratch Postgres at PG_URL (superuser) — it creates role `procera_app` and schema `sp09`.

use std::collections::HashSet;
use std::str::FromStr;
use std::time::{Duration, Instant};

use anyhow::Result;
use cedar_policy::{Authorizer, Context, Decision, Entities, EntityUid, PolicySet, Request};
use rand::Rng;
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

const ROWS_PER_WS: i64 = 200_000;

fn pct(mut v: Vec<Duration>, p: f64) -> Duration {
    v.sort();
    v[((v.len() as f64 - 1.0) * p) as usize]
}

#[tokio::main]
async fn main() -> Result<()> {
    let admin_url = std::env::var("PG_URL").unwrap_or_else(|_| "postgres://postgres@127.0.0.1:55432/postgres".into());
    let admin = PgPoolOptions::new().max_connections(4).connect(&admin_url).await?;
    let ws: Vec<Uuid> = (0..3).map(|_| Uuid::now_v7()).collect();

    println!("## A. RLS through a pooled connection\n");
    for stmt in [
        "DROP SCHEMA IF EXISTS sp09 CASCADE",
        "DO $$ BEGIN IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname='procera_app') THEN CREATE ROLE procera_app LOGIN NOSUPERUSER NOBYPASSRLS; END IF; END $$",
        "CREATE SCHEMA sp09",
        "CREATE TABLE sp09.requests (workspace_id uuid NOT NULL, id uuid NOT NULL, department text NOT NULL, title text NOT NULL, created_at timestamptz NOT NULL, PRIMARY KEY (workspace_id, id))",
        "CREATE INDEX ON sp09.requests (workspace_id, department, created_at DESC)",
        "ALTER TABLE sp09.requests ENABLE ROW LEVEL SECURITY",
        "ALTER TABLE sp09.requests FORCE ROW LEVEL SECURITY",
        "CREATE POLICY ws_isolation ON sp09.requests USING (workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::uuid) WITH CHECK (workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::uuid)",
        "GRANT USAGE ON SCHEMA sp09 TO procera_app",
        "GRANT SELECT, INSERT, UPDATE, DELETE ON sp09.requests TO procera_app",
    ] {
        sqlx::query(stmt).execute(&admin).await?;
    }
    let t = Instant::now();
    for w in &ws {
        // Superuser bypasses RLS for seeding (as the migrator would).
        sqlx::query(
            "INSERT INTO sp09.requests SELECT $1, gen_random_uuid(), (ARRAY['fin','hr','it','ops'])[1 + (g % 4)], 'req ' || g, now() - (g || ' minutes')::interval FROM generate_series(1, $2) g",
        )
        .bind(w)
        .bind(ROWS_PER_WS)
        .execute(&admin)
        .await?;
    }
    sqlx::query("ANALYZE sp09.requests").execute(&admin).await?;
    println!("Seeded {} rows across 3 workspaces in {:?}", ROWS_PER_WS * 3, t.elapsed());
    let sample_ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM sp09.requests TABLESAMPLE SYSTEM (1) LIMIT 300").fetch_all(&admin).await?;

    let app_url = admin_url.replacen("postgres@", "procera_app@", 1);
    let pool = PgPoolOptions::new().max_connections(20).connect(&app_url).await?;

    // A1: randomized concurrent leak test with transaction-local context.
    let mut handles = vec![];
    for i in 0..5_000u32 {
        let pool = pool.clone();
        let ws = ws.clone();
        let ids = sample_ids.clone();
        handles.push(tokio::spawn(async move {
            let choice = rand::rng().random_range(0..10);
            let mut tx = pool.begin().await?;
            let expected = if choice < 9 {
                let w = ws[(i as usize) % 3];
                sqlx::query("SELECT set_config('app.workspace_id', $1, true)").bind(w.to_string()).execute(&mut *tx).await?;
                Some(w)
            } else {
                None // forgot to set context
            };
            let rows: Vec<(Uuid,)> = sqlx::query_as("SELECT workspace_id FROM sp09.requests WHERE id = ANY($1)").bind(&ids).fetch_all(&mut *tx).await?;
            tx.commit().await?;
            let leaked = rows.iter().filter(|(w,)| Some(*w) != expected).count();
            anyhow::Ok((expected.is_some(), rows.len(), leaked))
        }));
    }
    let (mut with_ctx, mut no_ctx_rows, mut leaks, mut visible) = (0, 0, 0, 0);
    for h in handles {
        let (has, n, leaked) = h.await??;
        if has { with_ctx += 1; visible += n; } else { no_ctx_rows += n; }
        leaks += leaked;
    }
    println!("A1 transaction-local set_config: {with_ctx} tx with context saw {visible} own rows; tx without context saw {no_ctx_rows} rows; cross-workspace rows leaked: {leaks}");

    // A2: demonstrate why session-level SET is forbidden on pooled connections.
    let pool1 = PgPoolOptions::new().max_connections(1).connect(&app_url).await?;
    {
        let mut c = pool1.acquire().await?;
        sqlx::query("SELECT set_config('app.workspace_id', $1, false)").bind(ws[0].to_string()).execute(&mut *c).await?;
    }
    let leaked: i64 = sqlx::query_scalar("SELECT count(*) FROM sp09.requests").fetch_one(&pool1).await?;
    println!("A2 session-level set_config on a pooled connection: next borrower (no context set) sees {leaked} rows of workspace A  ← the leak the rule prevents");

    // A3: list query cost with RLS (app role) vs without (superuser bypass).
    let mut with_rls = vec![];
    let mut without = vec![];
    let q = "SELECT id, title FROM sp09.requests WHERE workspace_id = $1 AND department = $2 ORDER BY created_at DESC LIMIT 50";
    for i in 0..2_000 {
        let w = ws[i % 3];
        let dept = ["fin", "hr", "it", "ops"][i % 4];
        let t = Instant::now();
        let mut tx = pool.begin().await?;
        sqlx::query("SELECT set_config('app.workspace_id', $1, true)").bind(w.to_string()).execute(&mut *tx).await?;
        let r: Vec<(Uuid, String)> = sqlx::query_as(q).bind(w).bind(dept).fetch_all(&mut *tx).await.unwrap_or_default();
        tx.commit().await?;
        with_rls.push(t.elapsed());
        assert!(r.len() == 50 || r.is_empty());
        let t = Instant::now();
        let _r: Vec<(Uuid,)> = sqlx::query_as("SELECT id FROM sp09.requests WHERE workspace_id = $1 AND department = $2 ORDER BY created_at DESC LIMIT 50").bind(w).bind(dept).fetch_all(&admin).await?;
        without.push(t.elapsed());
    }
    println!("A3 list (50 rows, 600k-row table): with RLS + tx + set_config P50 {:?} P95 {:?}; superuser no-RLS P50 {:?} P95 {:?}",
        pct(with_rls.clone(), 0.5), pct(with_rls, 0.95), pct(without.clone(), 0.5), pct(without, 0.95));

    println!("\n## B. Cedar\n");
    cedar_part()?;
    Ok(())
}

/// Builds the Cedar policies a workspace's roles compile to (docs/12 §5–6, §10, §12).
fn compiled_policies() -> String {
    // Generated from: role "HR Designer" = [process:view, process:edit_draft] scoped to collection hr;
    // role "Workspace Admin" = all process actions, scope workspace; plus fixed invariants.
    r#"
    // role: hr-designer (custom, cloned from Process Designer) — scope collection:hr
    @id("role:hr-designer")
    permit(principal in Role::"ws1/hr-designer",
           action in [Action::"process:view", Action::"process:edit_draft"],
           resource in Collection::"ws1/hr");

    // role: workspace-admin — scope workspace
    @id("role:workspace-admin")
    permit(principal in Role::"ws1/workspace-admin",
           action in [Action::"process:view", Action::"process:edit_draft", Action::"process:approve", Action::"process:publish"],
           resource in Workspace::"ws1");

    // invariant: tenant isolation (cannot be overridden by any permit)
    @id("invariant:isolation")
    forbid(principal, action, resource) unless { principal.workspace == resource.workspace };

    // invariant: SoD — nobody approves their own controlled change
    @id("invariant:sod-author-approver")
    forbid(principal, action == Action::"process:approve", resource)
      when { resource.criticality == "controlled" && resource.author == principal };
    "#
    .into()
}

fn entities_json() -> String {
    r#"[
      {"uid":{"type":"Workspace","id":"ws1"},"attrs":{},"parents":[]},
      {"uid":{"type":"Workspace","id":"ws2"},"attrs":{},"parents":[]},
      {"uid":{"type":"Collection","id":"ws1/hr"},"attrs":{},"parents":[{"type":"Workspace","id":"ws1"}]},
      {"uid":{"type":"Collection","id":"ws1/hr/onboarding"},"attrs":{},"parents":[{"type":"Collection","id":"ws1/hr"}]},
      {"uid":{"type":"Collection","id":"ws1/fin"},"attrs":{},"parents":[{"type":"Workspace","id":"ws1"}]},
      {"uid":{"type":"Role","id":"ws1/hr-designer"},"attrs":{},"parents":[]},
      {"uid":{"type":"Role","id":"ws1/workspace-admin"},"attrs":{},"parents":[]},
      {"uid":{"type":"Member","id":"alice"},"attrs":{"workspace":{"__entity":{"type":"Workspace","id":"ws1"}}},"parents":[{"type":"Role","id":"ws1/hr-designer"}]},
      {"uid":{"type":"Member","id":"bob"},"attrs":{"workspace":{"__entity":{"type":"Workspace","id":"ws1"}}},"parents":[{"type":"Role","id":"ws1/workspace-admin"}]},
      {"uid":{"type":"Member","id":"mallory"},"attrs":{"workspace":{"__entity":{"type":"Workspace","id":"ws2"}}},"parents":[{"type":"Role","id":"ws1/workspace-admin"}]},
      {"uid":{"type":"Process","id":"p-onb"},"attrs":{"workspace":{"__entity":{"type":"Workspace","id":"ws1"}},"criticality":"controlled","author":{"__entity":{"type":"Member","id":"bob"}}},"parents":[{"type":"Collection","id":"ws1/hr/onboarding"}]},
      {"uid":{"type":"Process","id":"p-po"},"attrs":{"workspace":{"__entity":{"type":"Workspace","id":"ws1"}},"criticality":"standard","author":{"__entity":{"type":"Member","id":"alice"}}},"parents":[{"type":"Collection","id":"ws1/fin"}]}
    ]"#
    .into()
}

fn uid(s: &str) -> Result<EntityUid> {
    Ok(EntityUid::from_str(s)?)
}

fn decide(auth: &Authorizer, ps: &PolicySet, ents: &Entities, p: &str, a: &str, r: &str) -> Result<(Decision, Vec<String>)> {
    let req = Request::new(uid(p)?, uid(a)?, uid(r)?, Context::empty(), None)?;
    let resp = auth.is_authorized(&req, ps, ents);
    Ok((resp.decision(), resp.diagnostics().reason().map(|id| id.to_string()).collect()))
}

fn cedar_part() -> Result<()> {
    let ps = PolicySet::from_str(&compiled_policies())?;
    let ents = Entities::from_json_str(&entities_json(), None)?;
    let auth = Authorizer::new();
    let cases = [
        ("B1 HR designer edits a draft in hr/onboarding (subtree scope)", r#"Member::"alice""#, r#"Action::"process:edit_draft""#, r#"Process::"p-onb""#, Decision::Allow),
        ("B2 HR designer edits outside scope (fin)", r#"Member::"alice""#, r#"Action::"process:edit_draft""#, r#"Process::"p-po""#, Decision::Deny),
        ("B3 HR designer cannot approve (not in role)", r#"Member::"alice""#, r#"Action::"process:approve""#, r#"Process::"p-onb""#, Decision::Deny),
        ("B4 Admin approves standard process", r#"Member::"bob""#, r#"Action::"process:approve""#, r#"Process::"p-po""#, Decision::Allow),
        ("B5 Admin cannot approve own controlled change (forbid beats permit)", r#"Member::"bob""#, r#"Action::"process:approve""#, r#"Process::"p-onb""#, Decision::Deny),
        ("B6 Member of ws2 with a ws1 role grant is still isolated", r#"Member::"mallory""#, r#"Action::"process:view""#, r#"Process::"p-po""#, Decision::Deny),
    ];
    println!("| Case | Expected | Got | Matched policies |\n|---|---|---|---|");
    let mut ok = true;
    for (name, p, a, r, want) in cases {
        let (got, why) = decide(&auth, &ps, &ents, p, a, r)?;
        ok &= got == want;
        println!("| {name} | {want:?} | {got:?} | {} |", if why.is_empty() { "—".into() } else { why.join(", ") });
    }
    println!("\nAll Cedar cases correct: {ok}");

    // Latency.
    let req = Request::new(uid(r#"Member::"alice""#)?, uid(r#"Action::"process:edit_draft""#)?, uid(r#"Process::"p-onb""#)?, Context::empty(), None)?;
    let n = 100_000;
    let mut samples = Vec::with_capacity(n);
    for _ in 0..n {
        let t = Instant::now();
        std::hint::black_box(auth.is_authorized(&req, &ps, &ents));
        samples.push(t.elapsed());
    }
    println!("Decision latency over {n}: P50 {:?}, P99 {:?}", pct(samples.clone(), 0.5), pct(samples, 0.99));

    // Partial evaluation for list filters: resource unknown.
    let preq = cedar_policy::RequestBuilder::default()
        .principal(uid(r#"Member::"alice""#)?)
        .action(uid(r#"Action::"process:view""#)?)
        .context(Context::empty())
        .build();
    let presp = auth.is_authorized_partial(&preq, &ps, &ents);
    println!("\nPartial evaluation (alice, process:view, resource = ?): definitely-decision = {:?}", presp.decision());
    let mut seen = HashSet::new();
    for p in presp.nontrivial_residuals() {
        if seen.insert(p.id().to_string()) {
            println!("  residual {}: {}", p.id(), p.to_string().split_whitespace().collect::<Vec<_>>().join(" "));
        }
    }
    Ok(())
}
