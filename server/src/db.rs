//! SQLite pool and transaction helpers.
//!
//! Conventions for every module:
//! * Functions that touch the database take `conn: &mut SqliteConnection`. Callers holding a
//!   transaction pass `&mut tx` (deref coercion turns `&mut Transaction` into `&mut SqliteConnection`).
//! * Every write goes through [`write_tx`] (`BEGIN IMMEDIATE`) so concurrent writers queue on the
//!   SQLite write lock up-front instead of failing with `SQLITE_BUSY` on upgrade.
//! * Never open a second connection (or call a function that takes `&AppState` and touches the pool)
//!   while holding a `write_tx` — SQLite has one writer and you would deadlock until `busy_timeout`.

use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::error::AppResult;

/// A write transaction started with `BEGIN IMMEDIATE`.
pub type Tx = Transaction<'static, Sqlite>;

/// Opens (creating if missing) the database file with the project pragmas.
pub async fn connect(path: &Path) -> AppResult<SqlitePool> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let opts = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Full)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));
    let pool =
        SqlitePoolOptions::new().max_connections(8).acquire_timeout(Duration::from_secs(10)).connect_with(opts).await?;
    Ok(pool)
}

/// Applies `server/migrations/*.sql` (embedded at compile time).
pub async fn migrate(pool: &SqlitePool) -> AppResult<()> {
    sqlx::migrate!("./migrations").run(pool).await?;
    Ok(())
}

/// Begins a write transaction (`BEGIN IMMEDIATE`). Commit with `tx.commit().await?`;
/// dropping without commit rolls back.
pub async fn write_tx(pool: &SqlitePool) -> AppResult<Tx> {
    Ok(pool.begin_with("BEGIN IMMEDIATE").await?)
}

/// Bind value for dynamically built SQL (see `authz::case_scope_sql`).
#[derive(Debug, Clone, PartialEq)]
pub enum SqlValue {
    Int(i64),
    Text(String),
    Null,
}

/// Binds a slice of [`SqlValue`]s onto a `query`/`query_as`/`query_scalar` in order.
///
/// ```ignore
/// let scope = authz::case_scope_sql(&actor);
/// let sql = format!("SELECT c.id FROM cases c WHERE {}", scope.sql);
/// let ids: Vec<i64> = db::bind_all_scalar(sqlx::query_scalar(&sql), &scope.binds).fetch_all(&mut *conn).await?;
/// ```
pub fn bind_all<'q>(
    mut q: sqlx::query::Query<'q, Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    binds: &'q [SqlValue],
) -> sqlx::query::Query<'q, Sqlite, sqlx::sqlite::SqliteArguments<'q>> {
    for b in binds {
        q = match b {
            SqlValue::Int(i) => q.bind(*i),
            SqlValue::Text(s) => q.bind(s.as_str()),
            SqlValue::Null => q.bind(Option::<i64>::None),
        };
    }
    q
}

/// [`bind_all`] for `query_as`.
pub fn bind_all_as<'q, O>(
    mut q: sqlx::query::QueryAs<'q, Sqlite, O, sqlx::sqlite::SqliteArguments<'q>>,
    binds: &'q [SqlValue],
) -> sqlx::query::QueryAs<'q, Sqlite, O, sqlx::sqlite::SqliteArguments<'q>> {
    for b in binds {
        q = match b {
            SqlValue::Int(i) => q.bind(*i),
            SqlValue::Text(s) => q.bind(s.as_str()),
            SqlValue::Null => q.bind(Option::<i64>::None),
        };
    }
    q
}

/// [`bind_all`] for `query_scalar`.
pub fn bind_all_scalar<'q, O>(
    mut q: sqlx::query::QueryScalar<'q, Sqlite, O, sqlx::sqlite::SqliteArguments<'q>>,
    binds: &'q [SqlValue],
) -> sqlx::query::QueryScalar<'q, Sqlite, O, sqlx::sqlite::SqliteArguments<'q>> {
    for b in binds {
        q = match b {
            SqlValue::Int(i) => q.bind(*i),
            SqlValue::Text(s) => q.bind(s.as_str()),
            SqlValue::Null => q.bind(Option::<i64>::None),
        };
    }
    q
}
