//! Saved views shared with a role (A-35, E2E-R1-13) and XLSX grid exports (FR-V07).

use std::io::Cursor;

use calamine::{Data, Reader, Xlsx, open_workbook_from_rs};
use procera_db::{WsCtx, WsTx, test_support};
use serde_json::{Value, json};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use super::{check_audience, next_audience, row_by_id, visible_rows, xlsx};

fn read(bytes: Vec<u8>) -> (calamine::Range<Data>, calamine::Range<String>) {
    let mut wb: Xlsx<_> = open_workbook_from_rs(Cursor::new(bytes)).expect("a readable workbook");
    let cells = wb.worksheet_range("Export").expect("the Export sheet");
    let formulas = wb.worksheet_formula("Export").expect("formulas");
    (cells, formulas)
}

#[test]
fn xlsx_has_exactly_the_cells_in_order_and_no_formulas() {
    let header = vec!["Title".to_owned(), "Owner".to_owned(), "Version".to_owned(), "Published".to_owned()];
    let cells = vec![
        vec![json!("=HYPERLINK(\"http://x\")"), json!("Ana"), json!(3), json!(true)],
        vec![json!("-1+2"), Value::Null, json!(1.5), json!(false)],
        vec![json!("Plain"), json!("@Ben"), json!(-4), Value::Null],
    ];
    let bytes = xlsx::workbook(&header, &cells, 4, 3).expect("built");
    let (got, formulas) = read(bytes);
    assert_eq!(got.get_size(), (4, 4));
    let at = |r: u32, c: u32| got.get_value((r, c)).cloned().unwrap_or(Data::Empty);
    assert_eq!(at(0, 0), Data::String("Title".into()));
    assert_eq!(at(0, 3), Data::String("Published".into()));
    assert_eq!(at(1, 0), Data::String("=HYPERLINK(\"http://x\")".into()), "formula-like text stays text");
    assert_eq!(at(1, 2), Data::Float(3.0));
    assert_eq!(at(1, 3), Data::Bool(true));
    assert_eq!(at(2, 0), Data::String("-1+2".into()));
    assert_eq!(at(2, 1), Data::Empty);
    assert_eq!(at(2, 2), Data::Float(1.5));
    assert_eq!(at(3, 1), Data::String("@Ben".into()));
    assert_eq!(at(3, 2), Data::Float(-4.0));
    assert!(formulas.used_cells().all(|(_, _, f)| f.is_empty()), "no cell is a formula");
}

#[test]
fn xlsx_marks_formula_like_text_with_the_quote_prefix() {
    let bytes =
        xlsx::workbook(&["=A".to_owned()], &[vec![json!("+1")], vec![json!("ok")]], 1, 2).expect("built");
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut styles = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("xl/styles.xml").expect("styles"), &mut styles)
        .expect("utf-8");
    assert!(styles.contains("quotePrefix=\"1\""), "a quote-prefix style exists");
}

#[test]
fn xlsx_rejects_a_request_that_is_not_the_grid() {
    let h = vec!["A".to_owned()];
    assert!(xlsx::workbook(&h, &[vec![json!(1)]], 2, 1).is_err(), "header must match the columns");
    assert!(xlsx::workbook(&h, &[vec![json!(1)]], 1, 2).is_err(), "rows must match the cells");
    assert!(xlsx::workbook(&h, &[vec![json!(1), json!(2)]], 1, 1).is_err(), "one value per column");
    assert!(xlsx::workbook(&h, &[vec![json!({"x": 1})]], 1, 1).is_err(), "objects are not cells");
    assert!(xlsx::workbook(&h, &[vec![json!("x".repeat(40_000))]], 1, 1).is_err(), "cell size");
    let many = vec![vec![json!(1)]; xlsx::MAX_ROWS + 1];
    assert!(xlsx::workbook(&h, &many, 1, u32::try_from(many.len()).unwrap_or(0)).is_err(), "row cap");
    assert!(xlsx::workbook(&[], &[], 0, 0).is_err(), "at least one column");
}

#[test]
fn patching_the_audience() {
    let role = |k: &str| Some(k.to_owned());
    // Naming a role unshares from everyone; sharing with everyone drops the role.
    assert_eq!(next_audience((true, None), None, Some(role("auditor"))), (false, role("auditor")));
    assert_eq!(next_audience((false, role("auditor")), Some(true), None), (true, None));
    // `null` clears the role; `shared: false` alone keeps it; nothing named keeps both.
    assert_eq!(next_audience((false, role("auditor")), None, Some(None)), (false, None));
    assert_eq!(next_audience((false, role("auditor")), Some(false), None), (false, role("auditor")));
    assert_eq!(next_audience((true, None), None, None), (true, None));
    // A blank key is no role; an explicit conflicting pair is passed on for check_audience to refuse.
    assert_eq!(next_audience((false, None), None, Some(role("  "))), (false, None));
    assert_eq!(next_audience((false, None), Some(true), Some(role("auditor"))), (true, role("auditor")));
}

async fn member(tx: &mut WsTx<'_>, admin: &mut sqlx::PgConnection) -> anyhow::Result<Uuid> {
    let user = Uuid::now_v7();
    sqlx::query("INSERT INTO users (id, email, display_name, subject_ref) VALUES ($1, $2, 'U', $3)")
        .bind(user)
        .bind(format!("{user}@acme.test"))
        .bind(Uuid::new_v4())
        .execute(&mut *admin)
        .await?;
    let m = Uuid::now_v7();
    sqlx::query("INSERT INTO memberships (workspace_id, id, user_id, status) VALUES ($1, $2, $3, 'active')")
        .bind(tx.ctx.workspace_id)
        .bind(m)
        .bind(user)
        .execute(tx.conn())
        .await?;
    Ok(m)
}

async fn role(tx: &mut WsTx<'_>, key: &str) -> anyhow::Result<Uuid> {
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO roles (workspace_id, id, key, name, permissions) VALUES ($1, $2, $3, $4, '{}')")
        .bind(tx.ctx.workspace_id)
        .bind(id)
        .bind(key)
        .bind(format!("Role {key}"))
        .execute(tx.conn())
        .await?;
    Ok(id)
}

async fn grant(
    tx: &mut WsTx<'_>,
    role: Uuid,
    membership: Option<Uuid>,
    group: Option<Uuid>,
    expires: Option<OffsetDateTime>,
) -> anyhow::Result<()> {
    sqlx::query("INSERT INTO role_grants (workspace_id, id, role_id, membership_id, group_id, scope_kind, expires_at) VALUES ($1, $2, $3, $4, $5, 'workspace', $6)")
        .bind(tx.ctx.workspace_id)
        .bind(Uuid::now_v7())
        .bind(role)
        .bind(membership)
        .bind(group)
        .bind(expires)
        .execute(tx.conn())
        .await?;
    Ok(())
}

async fn view(
    tx: &mut WsTx<'_>,
    owner: Uuid,
    name: &str,
    shared: bool,
    role: Option<&str>,
) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO saved_views (workspace_id, id, owner_membership_id, list, name, state, shared, shared_role, created_at, updated_at) VALUES ($1, $2, $3, 'library.documents', $4, '{}', $5, $6, now(), now())")
        .bind(tx.ctx.workspace_id)
        .bind(id)
        .bind(owner)
        .bind(name)
        .bind(shared)
        .bind(role)
        .execute(tx.conn())
        .await?;
    Ok(id)
}

async fn names(tx: &mut WsTx<'_>, me: Uuid, now: OffsetDateTime) -> anyhow::Result<Vec<String>> {
    Ok(visible_rows(tx, me, now, "library.documents")
        .await
        .map_err(|e| anyhow::anyhow!("{e:?}"))?
        .into_iter()
        .map(|r| r.3)
        .collect())
}

#[tokio::test]
async fn a_view_shared_with_a_role_reaches_its_holders_only() -> anyhow::Result<()> {
    let Some(t) = test_support::fresh().await else { return Ok(()) };
    let t = t?;
    let mut admin = <sqlx::PgConnection as sqlx::Connection>::connect(&t.admin_url).await?;
    let mut tx = t.db.begin_ws(WsCtx { workspace_id: Uuid::now_v7() }).await?;
    let now = OffsetDateTime::now_utc();
    let (owner, direct, via_group, expired, other) = (
        member(&mut tx, &mut admin).await?,
        member(&mut tx, &mut admin).await?,
        member(&mut tx, &mut admin).await?,
        member(&mut tx, &mut admin).await?,
        member(&mut tx, &mut admin).await?,
    );
    let auditor = role(&mut tx, "auditor").await?;
    role(&mut tx, "analyst").await?;
    let g = Uuid::now_v7();
    sqlx::query("INSERT INTO groups (workspace_id, id, name) VALUES ($1, $2, 'Audit team')")
        .bind(tx.ctx.workspace_id)
        .bind(g)
        .execute(tx.conn())
        .await?;
    sqlx::query("INSERT INTO group_members (workspace_id, group_id, membership_id) VALUES ($1, $2, $3)")
        .bind(tx.ctx.workspace_id)
        .bind(g)
        .bind(via_group)
        .execute(tx.conn())
        .await?;
    grant(&mut tx, auditor, Some(direct), None, None).await?;
    grant(&mut tx, auditor, None, Some(g), Some(now + Duration::days(1))).await?;
    grant(&mut tx, auditor, Some(expired), None, Some(now - Duration::days(1))).await?;

    let audit_view = view(&mut tx, owner, "For auditors", false, Some("auditor")).await?;
    view(&mut tx, owner, "Everyone", true, None).await?;
    view(&mut tx, owner, "Mine", false, None).await?;

    assert_eq!(
        names(&mut tx, owner, now).await?,
        ["Everyone", "For auditors", "Mine"],
        "the owner sees all of theirs"
    );
    assert_eq!(names(&mut tx, direct, now).await?, ["Everyone", "For auditors"], "a direct grant");
    assert_eq!(
        names(&mut tx, via_group, now).await?,
        ["Everyone", "For auditors"],
        "a grant through a group"
    );
    assert_eq!(names(&mut tx, expired, now).await?, ["Everyone"], "an expired grant does not count");
    assert_eq!(names(&mut tx, other, now).await?, ["Everyone"], "no grant");
    assert_eq!(
        names(&mut tx, via_group, now + Duration::days(2)).await?,
        ["Everyone"],
        "visibility follows the grant's expiry"
    );

    // Single-view reads carry the role name and the caller's visibility (403 vs 404 in the handlers).
    let r = row_by_id(&mut tx, direct, now, audit_view, false)
        .await
        .map_err(|e| anyhow::anyhow!("{e:?}"))?
        .expect("row");
    assert_eq!((r.9.as_deref(), r.10.as_deref(), r.11), (Some("auditor"), Some("Role auditor"), true));
    let r = row_by_id(&mut tx, other, now, audit_view, false)
        .await
        .map_err(|e| anyhow::anyhow!("{e:?}"))?
        .expect("row");
    assert!(!r.11, "hidden from a member without the role");

    // Audience checks: an unknown role and "everyone and a role" are refused.
    assert!(check_audience(&mut tx, false, Some("auditor")).await.is_ok());
    assert!(check_audience(&mut tx, false, Some("nobody")).await.is_err());
    assert!(check_audience(&mut tx, true, Some("auditor")).await.is_err());
    assert!(check_audience(&mut tx, true, None).await.is_ok());
    tx.commit().await?;
    Ok(())
}

#[tokio::test]
async fn the_schema_keeps_one_audience_and_survives_role_deletion() -> anyhow::Result<()> {
    let Some(t) = test_support::fresh().await else { return Ok(()) };
    let t = t?;
    let mut admin = <sqlx::PgConnection as sqlx::Connection>::connect(&t.admin_url).await?;
    let ws = Uuid::now_v7();
    let mut tx = t.db.begin_ws(WsCtx { workspace_id: ws }).await?;
    let owner = member(&mut tx, &mut admin).await?;
    let r = role(&mut tx, "auditor").await?;
    let v = view(&mut tx, owner, "For auditors", false, Some("auditor")).await?;
    tx.commit().await?;

    // Shared with everyone and a role at once, or with a role that does not exist: refused.
    let mut tx = t.db.begin_ws(WsCtx { workspace_id: ws }).await?;
    assert!(view(&mut tx, owner, "Both", true, Some("auditor")).await.is_err());
    tx.rollback().await?;
    let mut tx = t.db.begin_ws(WsCtx { workspace_id: ws }).await?;
    assert!(view(&mut tx, owner, "Ghost", false, Some("ghost")).await.is_err());
    tx.rollback().await?;

    // Deleting the role makes the view personal.
    let mut tx = t.db.begin_ws(WsCtx { workspace_id: ws }).await?;
    sqlx::query("DELETE FROM roles WHERE id = $1").bind(r).execute(tx.conn()).await?;
    let role_now: Option<String> = sqlx::query_scalar("SELECT shared_role FROM saved_views WHERE id = $1")
        .bind(v)
        .fetch_one(tx.conn())
        .await?;
    assert_eq!(role_now, None);
    tx.commit().await?;

    // Another workspace never sees the row (forced RLS).
    let mut tx = t.db.begin_ws(WsCtx { workspace_id: Uuid::now_v7() }).await?;
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM saved_views").fetch_one(tx.conn()).await?;
    assert_eq!(n, 0);
    tx.rollback().await?;
    t.db.assert_rls_guard().await?;
    Ok(())
}
