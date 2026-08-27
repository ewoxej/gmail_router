//! The multi-user data store: one shared SQLite database (`gmail_router.db`)
//! with `user_id` columns, isolating every user's settings, Google refresh
//! token, and routing table. Modeled on Vessel's flow (a shared user registry +
//! hashed device tokens for Bearer/Authelia auth), but collapsed to a single DB
//! because the background routing loop has to sweep *every* user each interval.

use crate::models::{AddressEntry, ApiTokenRow, RouteAction, RouterStatus};
use anyhow::Result;
use sha2::{Digest, Sha256};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::OnceCell;

pub fn data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("DATA_DIR") {
        if !dir.is_empty() {
            return dir.into();
        }
    }
    let mut base = dirs::config_dir().expect("Cannot find config dir");
    base.push("gmail_router");
    base
}

fn db_path() -> PathBuf {
    data_dir().join("gmail_router.db")
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS app_user (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    username TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS device_tokens (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL REFERENCES app_user(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    token TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    last_used_at TEXT NULL
);

CREATE TABLE IF NOT EXISTS user_settings (
    user_id INTEGER PRIMARY KEY REFERENCES app_user(id) ON DELETE CASCADE,
    domain TEXT NOT NULL DEFAULT '',
    start_date TEXT NOT NULL DEFAULT '1970-01-01T00:00:00Z',
    updated_date TEXT NULL
);

CREATE TABLE IF NOT EXISTS google_auth (
    user_id INTEGER PRIMARY KEY REFERENCES app_user(id) ON DELETE CASCADE,
    client_id TEXT NOT NULL,
    client_secret TEXT NOT NULL,
    refresh_token TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS routes (
    user_id INTEGER NOT NULL REFERENCES app_user(id) ON DELETE CASCADE,
    local_part TEXT NOT NULL,
    action TEXT NOT NULL DEFAULT 'keep',
    forward_to TEXT NULL,
    PRIMARY KEY (user_id, local_part)
);
"#;

static POOL: OnceCell<SqlitePool> = OnceCell::const_new();

async fn init_db() -> Result<SqlitePool> {
    let path = db_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let opts = SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(10));
    let pool = SqlitePoolOptions::new()
        .max_connections(10)
        .connect_with(opts)
        .await?;
    sqlx::raw_sql(SCHEMA).execute(&pool).await?;
    Ok(pool)
}

pub async fn pool() -> Result<SqlitePool> {
    Ok(POOL.get_or_try_init(init_db).await?.clone())
}

#[derive(Clone, Debug)]
pub struct CurrentUser {
    pub user_id: i64,
    pub username: String,
    pub device_token_id: Option<i64>,
}

pub fn current_user() -> CurrentUser {
    dioxus::fullstack::FullstackContext::current()
        .and_then(|ctx| ctx.extension::<CurrentUser>())
        .expect("CurrentUser missing from request context - auth middleware must run first")
}

pub fn is_valid_username(username: &str) -> bool {
    !username.is_empty()
        && username
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-')
}

pub fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    let mut hex = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

pub async fn ensure_user(username: &str) -> Result<i64> {
    if !is_valid_username(username) {
        anyhow::bail!("invalid username: {username:?}");
    }
    let pool = pool().await?;
    sqlx::query("INSERT OR IGNORE INTO app_user (username) VALUES (?)")
        .bind(username)
        .execute(&pool)
        .await?;
    let row = sqlx::query("SELECT id FROM app_user WHERE username = ?")
        .bind(username)
        .fetch_one(&pool)
        .await?;
    Ok(row.get("id"))
}

pub async fn resolve_device_token(token: &str) -> Result<Option<(i64, String, i64)>> {
    let pool = pool().await?;
    let hit = sqlx::query(
        "UPDATE device_tokens SET last_used_at = datetime('now') WHERE token = ? RETURNING user_id, id",
    )
    .bind(hash_token(token))
    .fetch_optional(&pool)
    .await?;
    let Some(row) = hit else { return Ok(None) };
    let user_id: i64 = row.get("user_id");
    let token_id: i64 = row.get("id");
    let user = sqlx::query("SELECT username FROM app_user WHERE id = ?")
        .bind(user_id)
        .fetch_optional(&pool)
        .await?;
    Ok(user.map(|r| (user_id, r.get("username"), token_id)))
}

// ---------------------------------------------------------------------------
// Per-user settings
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Settings {
    pub domain: String,
    /// RFC-3339. Scans before this date are ignored on the very first scan.
    pub start_date: String,
    /// RFC-3339 of the last scan; `None` until the first scan runs.
    pub updated_date: Option<String>,
}

pub async fn get_settings(user_id: i64) -> Result<Settings> {
    let pool = pool().await?;
    sqlx::query("INSERT OR IGNORE INTO user_settings (user_id) VALUES (?)")
        .bind(user_id)
        .execute(&pool)
        .await?;
    let row = sqlx::query("SELECT domain, start_date, updated_date FROM user_settings WHERE user_id = ?")
        .bind(user_id)
        .fetch_one(&pool)
        .await?;
    Ok(Settings {
        domain: row.get("domain"),
        start_date: row.get("start_date"),
        updated_date: row.get("updated_date"),
    })
}

pub async fn has_settings_row(user_id: i64) -> Result<bool> {
    let pool = pool().await?;
    let row = sqlx::query("SELECT 1 FROM user_settings WHERE user_id = ?")
        .bind(user_id)
        .fetch_optional(&pool)
        .await?;
    Ok(row.is_some())
}

pub async fn set_settings(user_id: i64, domain: &str, start_date: &str) -> Result<()> {
    let pool = pool().await?;
    sqlx::query(
        "INSERT INTO user_settings (user_id, domain, start_date) VALUES (?, ?, ?)
         ON CONFLICT(user_id) DO UPDATE SET domain = excluded.domain, start_date = excluded.start_date",
    )
    .bind(user_id)
    .bind(domain)
    .bind(start_date)
    .execute(&pool)
    .await?;
    Ok(())
}

pub async fn set_updated_date(user_id: i64, rfc3339: &str) -> Result<()> {
    let pool = pool().await?;
    sqlx::query("UPDATE user_settings SET updated_date = ? WHERE user_id = ?")
        .bind(rfc3339)
        .bind(user_id)
        .execute(&pool)
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Google auth (per-user refresh token)
// ---------------------------------------------------------------------------

pub async fn has_google_auth(user_id: i64) -> Result<bool> {
    let pool = pool().await?;
    let row = sqlx::query("SELECT 1 FROM google_auth WHERE user_id = ?")
        .bind(user_id)
        .fetch_optional(&pool)
        .await?;
    Ok(row.is_some())
}

pub async fn get_google_auth(user_id: i64) -> Result<Option<(String, String, String)>> {
    let pool = pool().await?;
    let row = sqlx::query("SELECT client_id, client_secret, refresh_token FROM google_auth WHERE user_id = ?")
        .bind(user_id)
        .fetch_optional(&pool)
        .await?;
    Ok(row.map(|r| (r.get("client_id"), r.get("client_secret"), r.get("refresh_token"))))
}

pub async fn set_google_auth(
    user_id: i64,
    client_id: &str,
    client_secret: &str,
    refresh_token: &str,
) -> Result<()> {
    let pool = pool().await?;
    sqlx::query(
        "INSERT INTO google_auth (user_id, client_id, client_secret, refresh_token, updated_at)
         VALUES (?, ?, ?, ?, datetime('now'))
         ON CONFLICT(user_id) DO UPDATE SET
             client_id = excluded.client_id,
             client_secret = excluded.client_secret,
             refresh_token = excluded.refresh_token,
             updated_at = datetime('now')",
    )
    .bind(user_id)
    .bind(client_id)
    .bind(client_secret)
    .bind(refresh_token)
    .execute(&pool)
    .await?;
    Ok(())
}

pub async fn list_user_ids_with_auth() -> Result<Vec<i64>> {
    let pool = pool().await?;
    let rows = sqlx::query("SELECT user_id FROM google_auth")
        .fetch_all(&pool)
        .await?;
    Ok(rows.iter().map(|r| r.get::<i64, _>("user_id")).collect())
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

fn action_to_row(action: &RouteAction) -> (&'static str, Option<String>) {
    match action {
        RouteAction::Forward { to } => ("forward", Some(to.clone())),
        other => (other.kind(), None),
    }
}

fn row_to_action(kind: &str, forward_to: Option<String>) -> RouteAction {
    RouteAction::from_kind(kind, forward_to.as_deref().unwrap_or(""))
}

pub async fn list_routes(user_id: i64) -> Result<Vec<AddressEntry>> {
    let pool = pool().await?;
    let rows = sqlx::query(
        "SELECT local_part, action, forward_to FROM routes WHERE user_id = ? ORDER BY local_part",
    )
    .bind(user_id)
    .fetch_all(&pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| AddressEntry {
            local_part: r.get("local_part"),
            action: row_to_action(r.get::<String, _>("action").as_str(), r.get("forward_to")),
        })
        .collect())
}

pub async fn routes_map(user_id: i64) -> Result<HashMap<String, RouteAction>> {
    let pool = pool().await?;
    let rows = sqlx::query("SELECT local_part, action, forward_to FROM routes WHERE user_id = ?")
        .bind(user_id)
        .fetch_all(&pool)
        .await?;
    Ok(rows
        .iter()
        .map(|r| {
            (
                r.get::<String, _>("local_part"),
                row_to_action(r.get::<String, _>("action").as_str(), r.get("forward_to")),
            )
        })
        .collect())
}

pub async fn add_addresses(user_id: i64, local_parts: impl IntoIterator<Item = String>) -> Result<usize> {
    let pool = pool().await?;
    let mut added = 0;
    for local in local_parts {
        let res = sqlx::query(
            "INSERT OR IGNORE INTO routes (user_id, local_part, action) VALUES (?, ?, 'keep')",
        )
        .bind(user_id)
        .bind(&local)
        .execute(&pool)
        .await?;
        added += res.rows_affected() as usize;
    }
    Ok(added)
}

pub async fn upsert_route(user_id: i64, local_part: &str, action: RouteAction) -> Result<()> {
    let pool = pool().await?;
    let (kind, forward_to) = action_to_row(&action);
    sqlx::query(
        "INSERT INTO routes (user_id, local_part, action, forward_to) VALUES (?, ?, ?, ?)
         ON CONFLICT(user_id, local_part) DO UPDATE SET action = excluded.action, forward_to = excluded.forward_to",
    )
    .bind(user_id)
    .bind(local_part)
    .bind(kind)
    .bind(forward_to)
    .execute(&pool)
    .await?;
    Ok(())
}

pub async fn get_or_create_route_action(user_id: i64, local_part: &str) -> Result<RouteAction> {
    let pool = pool().await?;
    if let Some(row) = sqlx::query("SELECT action, forward_to FROM routes WHERE user_id = ? AND local_part = ?")
        .bind(user_id)
        .bind(local_part)
        .fetch_optional(&pool)
        .await?
    {
        return Ok(row_to_action(row.get::<String, _>("action").as_str(), row.get("forward_to")));
    }
    sqlx::query("INSERT OR IGNORE INTO routes (user_id, local_part, action) VALUES (?, ?, 'keep')")
        .bind(user_id)
        .bind(local_part)
        .execute(&pool)
        .await?;
    Ok(RouteAction::Keep)
}

pub async fn set_route_action(user_id: i64, local_part: &str, action: RouteAction) -> Result<()> {
    if let RouteAction::Forward { to } = &action {
        if to.trim().is_empty() {
            anyhow::bail!("Forward requires a destination address");
        }
    }
    let pool = pool().await?;
    let (kind, forward_to) = action_to_row(&action);
    let res = sqlx::query("UPDATE routes SET action = ?, forward_to = ? WHERE user_id = ? AND local_part = ?")
        .bind(kind)
        .bind(forward_to)
        .bind(user_id)
        .bind(local_part)
        .execute(&pool)
        .await?;
    if res.rows_affected() == 0 {
        anyhow::bail!("Unknown address: {local_part}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// API tokens (Bearer auth for automation)
// ---------------------------------------------------------------------------

pub async fn create_token(user_id: i64, name: &str) -> Result<String> {
    let raw = uuid::Uuid::new_v4().to_string();
    let pool = pool().await?;
    sqlx::query("INSERT INTO device_tokens (user_id, name, token) VALUES (?, ?, ?)")
        .bind(user_id)
        .bind(name)
        .bind(hash_token(&raw))
        .execute(&pool)
        .await?;
    Ok(raw)
}

pub async fn list_tokens(user_id: i64) -> Result<Vec<ApiTokenRow>> {
    let pool = pool().await?;
    let rows = sqlx::query(
        "SELECT id, name, created_at, last_used_at FROM device_tokens WHERE user_id = ? ORDER BY id DESC",
    )
    .bind(user_id)
    .fetch_all(&pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| ApiTokenRow {
            id: r.get("id"),
            name: r.get("name"),
            created_at: r.get("created_at"),
            last_used_at: r.get("last_used_at"),
        })
        .collect())
}

pub async fn revoke_token(user_id: i64, token_id: i64) -> Result<()> {
    let pool = pool().await?;
    sqlx::query("DELETE FROM device_tokens WHERE user_id = ? AND id = ?")
        .bind(user_id)
        .bind(token_id)
        .execute(&pool)
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// UI status
// ---------------------------------------------------------------------------

pub async fn router_status(user_id: i64) -> Result<RouterStatus> {
    let settings = get_settings(user_id).await?;
    Ok(RouterStatus {
        authenticated: has_google_auth(user_id).await?,
        domain: settings.domain,
        last_scan: settings.updated_date,
        mode: crate::config::mode_str().to_string(),
    })
}
