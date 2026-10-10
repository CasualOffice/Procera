//! Throwaway database per test (needs PROCERA_TEST_DATABASE_URL pointing at a superuser connection).

use sqlx::{Connection, Executor, PgConnection};
use uuid::Uuid;

use crate::{Db, DbError, migrate};

#[derive(Debug)]
pub struct TestDb {
    pub db: Db,
    pub admin_url: String,
    pub app_url: String,
    pub name: String,
}

/// Returns `None` when PROCERA_TEST_DATABASE_URL is unset (tests then skip).
pub async fn fresh() -> Option<Result<TestDb, DbError>> {
    let base = std::env::var("PROCERA_TEST_DATABASE_URL").ok()?;
    Some(create(&base).await)
}

fn with_db(url: &str, db: &str, user: Option<&str>) -> String {
    let (scheme_auth, rest) = url.split_once("://").unwrap_or(("postgres", url));
    let (auth_host, _) = rest.split_once('/').unwrap_or((rest, ""));
    let host = auth_host.rsplit_once('@').map(|(_, h)| h).unwrap_or(auth_host);
    match user {
        Some(u) => format!("{scheme_auth}://{u}@{host}/{db}"),
        None => format!("{scheme_auth}://{auth_host}/{db}"),
    }
}

static BOOTSTRAP: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn create(base: &str) -> Result<TestDb, DbError> {
    let _guard = BOOTSTRAP.lock().await; // serialise cluster-wide role bootstrap within a test binary
    let name = format!("procera_test_{}", Uuid::now_v7().simple());
    let mut admin = PgConnection::connect(base).await?;
    admin.execute(format!("CREATE DATABASE {name}").as_str()).await?;
    admin.close().await?;
    let admin_url = with_db(base, &name, None);
    let mut conn = PgConnection::connect(&admin_url).await?;
    conn.execute(include_str!("../../../deploy/sql/00-roles.sql")).await?;
    conn.close().await?;
    // Migrations run as the migrator role (test cluster uses trust auth).
    migrate(&with_db(base, &name, Some("procera_migrator"))).await?;
    let app_url = with_db(base, &name, Some("procera_app"));
    let db = Db::connect(&app_url, 10).await?;
    Ok(TestDb { db, admin_url, app_url, name })
}
