//! The authenticated person behind a request.

use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;

use crate::authz::Role;
use crate::error::{AppError, AppResult};

/// `users.kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserKind {
    Resident,
    Staff,
}

impl UserKind {
    pub fn as_str(self) -> &'static str {
        match self {
            UserKind::Resident => "resident",
            UserKind::Staff => "staff",
        }
    }

    pub fn parse(s: &str) -> AppResult<UserKind> {
        match s {
            "resident" => Ok(UserKind::Resident),
            "staff" => Ok(UserKind::Staff),
            other => Err(AppError::internal(format!("unknown user kind {other}"))),
        }
    }
}

/// One active row of `role_grants`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleGrant {
    pub role: Role,
    /// `None` = the role applies to all services.
    pub scope_service_id: Option<i64>,
}

/// The acting user (from the session) — or the system actor for background work.
///
/// When persisting "who did it" columns use [`Actor::db_id`], which is `None` for the system actor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Actor {
    pub user_id: i64,
    pub kind: UserKind,
    /// Active role grants only.
    pub roles: Vec<RoleGrant>,
    pub display_name: String,
    /// Staff sessions: TOTP verified in this session. Always true for residents.
    pub mfa_passed: bool,
}

/// `user_id` of the [`Actor::system`] actor. Never a real `users.id`.
pub const SYSTEM_USER_ID: i64 = 0;

impl Actor {
    /// The "ServiceHub (automatic)" actor used by background jobs, webhooks and automatic workflow
    /// advances. Holds no roles, so `authz` grants it nothing; system code paths skip access checks.
    pub fn system() -> Actor {
        Actor {
            user_id: SYSTEM_USER_ID,
            kind: UserKind::Staff,
            roles: Vec::new(),
            display_name: "ServiceHub (automatic)".into(),
            mfa_passed: true,
        }
    }

    pub fn is_system(&self) -> bool {
        self.user_id == SYSTEM_USER_ID
    }

    /// Value for `*_by` / `actor_user_id` columns: `None` for the system actor.
    pub fn db_id(&self) -> Option<i64> {
        if self.is_system() { None } else { Some(self.user_id) }
    }

    pub fn is_staff(&self) -> bool {
        self.kind == UserKind::Staff
    }

    /// Holds `role` (with any scope).
    pub fn has_role(&self, role: Role) -> bool {
        self.roles.iter().any(|g| g.role == role)
    }

    /// Holds any of `roles`.
    pub fn has_any_role(&self, roles: &[Role]) -> bool {
        roles.iter().any(|r| self.has_role(*r))
    }

    /// Roles that apply to service `service_id` (unscoped grants + grants scoped to that service), deduplicated.
    pub fn roles_for_service(&self, service_id: i64) -> Vec<Role> {
        let mut out: Vec<Role> = Vec::new();
        for g in &self.roles {
            if (g.scope_service_id.is_none() || g.scope_service_id == Some(service_id)) && !out.contains(&g.role) {
                out.push(g.role);
            }
        }
        out
    }

    /// Distinct role names (for `/api/me`).
    pub fn role_names(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for g in &self.roles {
            let name = g.role.as_str();
            if !out.contains(&name) {
                out.push(name);
            }
        }
        out
    }

    /// `forbidden` unless the actor holds one of `roles` (any scope).
    pub fn require_any_role(&self, roles: &[Role]) -> AppResult<()> {
        if self.has_any_role(roles) { Ok(()) } else { Err(AppError::forbidden()) }
    }

    /// Loads an active user and their active role grants.
    pub async fn load(conn: &mut SqliteConnection, user_id: i64, mfa_passed: bool) -> AppResult<Actor> {
        let row: Option<(String, String)> =
            sqlx::query_as("SELECT kind, display_name FROM users WHERE id = ? AND is_active = 1")
                .bind(user_id)
                .fetch_optional(&mut *conn)
                .await?;
        let (kind, display_name) = row.ok_or_else(AppError::unauthorized)?;
        let kind = UserKind::parse(&kind)?;
        let grants: Vec<(String, Option<i64>)> = sqlx::query_as(
            "SELECT role, scope_service_id FROM role_grants WHERE user_id = ? AND revoked_at IS NULL ORDER BY id",
        )
        .bind(user_id)
        .fetch_all(&mut *conn)
        .await?;
        let roles = grants
            .into_iter()
            .filter_map(|(r, scope)| Role::parse(&r).map(|role| RoleGrant { role, scope_service_id: scope }))
            .collect();
        Ok(Actor { user_id, kind, roles, display_name, mfa_passed: mfa_passed || kind == UserKind::Resident })
    }
}
