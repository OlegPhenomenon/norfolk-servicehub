//! User, role and decision-authority helpers (platform tables). The records slice's admin screens
//! write `users` / `role_grants` / `decision_authorities` only through these functions.

use serde::Serialize;
use sqlx::SqliteConnection;

use crate::auth::UserKind;
use crate::authz::Role;
use crate::error::{AppError, AppResult};
use crate::time;

/// Input for [`create_user`].
#[derive(Debug, Clone)]
pub struct NewUser {
    pub email: String,
    pub display_name: String,
    pub phone: Option<String>,
    pub kind: UserKind,
    /// argon2id PHC string (`auth::password::hash`), or `None` = cannot log in with a password.
    pub password_hash: Option<String>,
    pub totp_secret: Option<String>,
    pub totp_enabled: bool,
    pub persona_key: Option<String>,
    pub job_title: Option<String>,
}

/// Public profile shown in `/api/me` and admin lists.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct UserPublic {
    pub id: i64,
    #[sqlx(rename = "display_name")]
    pub name: String,
    pub email: String,
    pub kind: String,
    pub persona_key: Option<String>,
    pub job_title: Option<String>,
    #[serde(serialize_with = "ser_bool")]
    pub totp_enabled: i64,
}

fn ser_bool<S: serde::Serializer>(v: &i64, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_bool(*v != 0)
}

/// Loads a user's public profile.
pub async fn load_public(conn: &mut SqliteConnection, user_id: i64) -> AppResult<UserPublic> {
    sqlx::query_as::<_, UserPublic>(
        "SELECT id, display_name, email, kind, persona_key, job_title, totp_enabled FROM users WHERE id = ?",
    )
    .bind(user_id)
    .fetch_optional(&mut *conn)
    .await?
    .ok_or_else(AppError::not_found)
}

/// Inserts a user; duplicate email → `conflict`.
pub async fn create_user(conn: &mut SqliteConnection, u: NewUser) -> AppResult<i64> {
    let id = sqlx::query_scalar(
        "INSERT INTO users (email, display_name, phone, kind, password_hash, totp_secret, totp_enabled, persona_key, job_title, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(u.email.trim())
    .bind(u.display_name.trim())
    .bind(&u.phone)
    .bind(u.kind.as_str())
    .bind(&u.password_hash)
    .bind(&u.totp_secret)
    .bind(i64::from(u.totp_enabled))
    .bind(&u.persona_key)
    .bind(&u.job_title)
    .bind(time::now_str())
    .fetch_one(&mut *conn)
    .await
    .map_err(|e| match AppError::from(e) {
        err if err.code == crate::error::ErrorCode::Conflict => AppError::conflict("An account with this email already exists."),
        err => err,
    })?;
    Ok(id)
}

/// Deactivation and session revocation share the caller's transaction.
pub async fn deactivate_user(conn: &mut SqliteConnection, user_id: i64) -> AppResult<()> {
    let count =
        sqlx::query("UPDATE users SET is_active=0 WHERE id=?").bind(user_id).execute(&mut *conn).await?.rows_affected();
    if count == 0 {
        return Err(AppError::not_found());
    }
    sqlx::query("DELETE FROM sessions WHERE user_id=?").bind(user_id).execute(conn).await?;
    Ok(())
}

/// Grants a role (optionally scoped to one service). Only staff users may hold roles.
pub async fn grant_role(
    conn: &mut SqliteConnection,
    user_id: i64,
    role: Role,
    scope_service_id: Option<i64>,
    granted_by: Option<i64>,
) -> AppResult<i64> {
    let kind: Option<String> =
        sqlx::query_scalar("SELECT kind FROM users WHERE id = ?").bind(user_id).fetch_optional(&mut *conn).await?;
    match kind.as_deref() {
        None => return Err(AppError::not_found()),
        Some("staff") => {}
        Some(_) => return Err(AppError::conflict("Only staff accounts can hold staff roles.")),
    }
    Ok(sqlx::query_scalar(
        "INSERT INTO role_grants (user_id, role, scope_service_id, granted_by, granted_at) VALUES (?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(user_id)
    .bind(role.as_str())
    .bind(scope_service_id)
    .bind(granted_by)
    .bind(time::now_str())
    .fetch_one(&mut *conn)
    .await?)
}

/// Revokes a role grant by id.
pub async fn revoke_role(conn: &mut SqliteConnection, grant_id: i64) -> AppResult<()> {
    let n = sqlx::query("UPDATE role_grants SET revoked_at = ? WHERE id = ? AND revoked_at IS NULL")
        .bind(time::now_str())
        .bind(grant_id)
        .execute(&mut *conn)
        .await?
        .rows_affected();
    if n == 0 { Err(AppError::not_found()) } else { Ok(()) }
}

/// Grants authority to issue `decision_type` decisions. Invariant: `granted_by` holds an active
/// unscoped-or-scoped `manager` role and is not the grantee. `sysadmin` never implies authority.
pub async fn grant_decision_authority(
    conn: &mut SqliteConnection,
    user_id: i64,
    decision_type: &str,
    service_id: Option<i64>,
    granted_by: i64,
) -> AppResult<i64> {
    if user_id == granted_by {
        return Err(AppError::forbidden_msg("Decision authority must be granted by someone else."));
    }
    let is_manager: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM role_grants WHERE user_id = ? AND role = 'manager' AND revoked_at IS NULL)",
    )
    .bind(granted_by)
    .fetch_one(&mut *conn)
    .await?;
    if !is_manager {
        return Err(AppError::forbidden_msg("Only a manager can grant decision authority."));
    }
    Ok(sqlx::query_scalar(
        "INSERT INTO decision_authorities (user_id, decision_type, service_id, granted_by, granted_at) VALUES (?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(user_id)
    .bind(decision_type)
    .bind(service_id)
    .bind(granted_by)
    .bind(time::now_str())
    .fetch_one(&mut *conn)
    .await?)
}

/// Does `user_id` hold active authority for `decision_type` (for `service_id`, or any service)?
pub async fn has_decision_authority(
    conn: &mut SqliteConnection,
    user_id: i64,
    decision_type: &str,
    service_id: i64,
) -> AppResult<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM decision_authorities WHERE user_id = ? AND decision_type = ? \
         AND (service_id IS NULL OR service_id = ?) AND revoked_at IS NULL)",
    )
    .bind(user_id)
    .bind(decision_type)
    .bind(service_id)
    .fetch_one(&mut *conn)
    .await?)
}
