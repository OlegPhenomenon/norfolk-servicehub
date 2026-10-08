//! Roles and case access — **the only place access rules live** (ARCHITECTURE §4).
//!
//! * [`case_access`] answers "what may this actor see of case X" for single-case endpoints.
//! * [`case_scope_sql`] returns the same rule as a SQL predicate over alias `c` (the `cases` table)
//!   for listings, search, counts, dashboards and exports.
//!
//! The two are tested to agree: a case appears in `case_scope_sql` results exactly when
//! `case_access` is `Applicant` or `Staff { .. }`. `TaskOnly` (field workers) is deliberately
//! *not* part of general case listings — field workers reach cases through their task list.
//!
//! Rules, in order:
//! 1. A `case_access_denials` row for (case, actor) → `None`, overriding everything.
//! 2. Staff roles (staff users only), except on an applicant's own unsubmitted draft (`status = 'draft'` with no
//!    `recorded_by_user_id`): that draft belongs to the applicant alone until they submit it. Assisted drafts
//!    (recorded by staff) stay visible to staff.
//!    * `confidential = 1` → `Staff` only for `complaints_officer` / `manager`; nobody else.
//!    * otherwise `intake`, `manager` → `Staff { can_manage: true }`; `specialist` whose grant is unscoped
//!      or scoped to the case's service → `Staff { can_manage: true }`; `finance` → `Staff { can_manage: false }`
//!      (full staff read view, money actions only). `sysadmin` alone → nothing.
//! 3. Applicant: personal case (`applicant_org_id IS NULL`) owned by the actor; organisation case with an
//!    active membership; or an active `case_representatives` row (any case, confidential included).
//! 4. Field worker with a non-cancelled task on a non-confidential case assigned to them → `TaskOnly`.

use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;

use crate::auth::Actor;
use crate::cases::core::{CaseRow, load_case};
use crate::db::SqlValue;
use crate::error::{AppError, AppResult};

/// Staff capabilities (`role_grants.role`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Intake,
    Specialist,
    Finance,
    FieldWorker,
    Manager,
    Sysadmin,
    ComplaintsOfficer,
}

impl Role {
    pub const ALL: [Role; 7] = [
        Role::Intake,
        Role::Specialist,
        Role::Finance,
        Role::FieldWorker,
        Role::Manager,
        Role::Sysadmin,
        Role::ComplaintsOfficer,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Intake => "intake",
            Role::Specialist => "specialist",
            Role::Finance => "finance",
            Role::FieldWorker => "field_worker",
            Role::Manager => "manager",
            Role::Sysadmin => "sysadmin",
            Role::ComplaintsOfficer => "complaints_officer",
        }
    }

    pub fn parse(s: &str) -> Option<Role> {
        Role::ALL.into_iter().find(|r| r.as_str() == s)
    }
}

/// Result of [`case_access`]. Determines the projection a handler must use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CaseAccess {
    /// Not visible. Handlers must answer `not_found`.
    None,
    /// Applicant projection: no internal notes, no staff-visibility events/documents/comments.
    Applicant,
    /// Staff view. `can_manage = false` for finance-only access (read + money actions).
    Staff { can_manage: bool },
    /// Field worker: task endpoints only (no applicant documents).
    TaskOnly,
}

impl CaseAccess {
    pub fn is_staff(self) -> bool {
        matches!(self, CaseAccess::Staff { .. })
    }
    pub fn can_manage(self) -> bool {
        matches!(self, CaseAccess::Staff { can_manage: true })
    }
}

/// A SQL predicate over alias `c` plus its positional binds. Use with `db::bind_all*`.
#[derive(Debug, Clone, PartialEq)]
pub struct ScopeSql {
    pub sql: String,
    pub binds: Vec<SqlValue>,
}

/// Minimal case facts the rules need.
#[derive(Debug, sqlx::FromRow)]
struct CaseFacts {
    service_id: i64,
    applicant_user_id: Option<i64>,
    applicant_org_id: Option<i64>,
    confidential: i64,
    status: String,
    recorded_by_user_id: Option<i64>,
    submitted_at: Option<String>,
}

/// SQL form of [`CaseFacts::applicant_private`] over alias `c`.
const APPLICANT_PRIVATE_DRAFT: &str =
    "(c.status IN ('draft','withdrawn') AND c.submitted_at IS NULL AND c.recorded_by_user_id IS NULL)";

impl CaseFacts {
    /// The applicant's own never-submitted draft, also after the applicant deleted it: no staff role reaches it.
    fn applicant_private(&self) -> bool {
        matches!(self.status.as_str(), "draft" | "withdrawn")
            && self.submitted_at.is_none()
            && self.recorded_by_user_id.is_none()
    }
}

/// Staff access from roles alone (rule 2), given the case's service and confidentiality.
fn staff_access_from_roles(actor: &Actor, service_id: i64, confidential: bool) -> Option<CaseAccess> {
    if !actor.is_staff() {
        return None;
    }
    if confidential {
        return actor
            .roles_for_service(service_id)
            .iter()
            .any(|r| matches!(r, Role::ComplaintsOfficer | Role::Manager))
            .then_some(CaseAccess::Staff { can_manage: true });
    }
    let roles = actor.roles_for_service(service_id);
    if roles.iter().any(|r| matches!(r, Role::Intake | Role::Manager | Role::Specialist)) {
        return Some(CaseAccess::Staff { can_manage: true });
    }
    if roles.contains(&Role::Finance) {
        return Some(CaseAccess::Staff { can_manage: false });
    }
    None
}

/// Computes the actor's access to a case. Unknown case → `CaseAccess::None`.
pub async fn case_access(conn: &mut SqliteConnection, actor: &Actor, case_id: i64) -> AppResult<CaseAccess> {
    let facts: Option<CaseFacts> = sqlx::query_as(
        "SELECT service_id, applicant_user_id, applicant_org_id, confidential, status, recorded_by_user_id, submitted_at \
         FROM cases WHERE id = ?",
    )
    .bind(case_id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(facts) = facts else { return Ok(CaseAccess::None) };
    let uid = actor.user_id;

    // 1. Explicit denial overrides everything.
    let denied: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM case_access_denials WHERE case_id = ? AND user_id = ?)")
            .bind(case_id)
            .bind(uid)
            .fetch_one(&mut *conn)
            .await?;
    if denied {
        return Ok(CaseAccess::None);
    }

    // 2. Staff by role (never on an applicant's own unsubmitted draft).
    let confidential = facts.confidential == 1;
    if !facts.applicant_private()
        && let Some(a) = staff_access_from_roles(actor, facts.service_id, confidential)
    {
        return Ok(a);
    }

    // 3. Applicant.
    let applicant = match facts.applicant_org_id {
        None => facts.applicant_user_id == Some(uid),
        Some(org_id) => sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM memberships WHERE organisation_id = ? AND user_id = ? AND status = 'active')",
        )
        .bind(org_id)
        .bind(uid)
        .fetch_one(&mut *conn)
        .await?,
    };
    let representative: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM case_representatives WHERE case_id = ? AND user_id = ? AND status = 'active')",
    )
    .bind(case_id)
    .bind(uid)
    .fetch_one(&mut *conn)
    .await?;
    if applicant || representative {
        return Ok(CaseAccess::Applicant);
    }

    // 4. Field worker via an assigned task.
    if actor.is_staff() && !confidential && actor.has_role(Role::FieldWorker) {
        let has_task: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM tasks WHERE case_id = ? AND assigned_to = ? AND status <> 'cancelled')",
        )
        .bind(case_id)
        .bind(uid)
        .fetch_one(&mut *conn)
        .await?;
        if has_task {
            return Ok(CaseAccess::TaskOnly);
        }
    }
    Ok(CaseAccess::None)
}

/// Loads the case and the actor's access; `not_found` when the actor has no access at all.
/// `TaskOnly` is returned as-is — handlers outside the task endpoints must reject it.
pub async fn require_case(
    conn: &mut SqliteConnection,
    actor: &Actor,
    case_id: i64,
) -> AppResult<(CaseRow, CaseAccess)> {
    let access = case_access(conn, actor, case_id).await?;
    if access == CaseAccess::None {
        return Err(AppError::not_found());
    }
    let case = load_case(conn, case_id).await?;
    Ok((case, access))
}

/// Like [`require_case`] but only staff access (`Staff { .. }`) passes; everything else is `not_found`
/// (applicants must not learn that a staff-only endpoint exists for their case).
pub async fn require_staff_case(
    conn: &mut SqliteConnection,
    actor: &Actor,
    case_id: i64,
) -> AppResult<(CaseRow, CaseAccess)> {
    let (case, access) = require_case(conn, actor, case_id).await?;
    if !access.is_staff() {
        return Err(AppError::not_found());
    }
    Ok((case, access))
}

/// SQL predicate (alias `c` = `cases`) selecting exactly the cases where [`case_access`] is
/// `Applicant` or `Staff { .. }`.
///
/// ```ignore
/// let scope = authz::case_scope_sql(&actor);
/// let sql = format!("SELECT c.id FROM cases c WHERE {} ORDER BY c.updated_at DESC", scope.sql);
/// let ids: Vec<i64> = db::bind_all_scalar(sqlx::query_scalar(&sql), &scope.binds).fetch_all(&mut *conn).await?;
/// ```
pub fn case_scope_sql(actor: &Actor) -> ScopeSql {
    let uid = actor.user_id;
    let mut binds = vec![SqlValue::Int(uid)];
    let denial = "NOT EXISTS (SELECT 1 FROM case_access_denials d WHERE d.case_id = c.id AND d.user_id = ?)";

    let mut branches: Vec<String> = Vec::new();

    // Staff branches (rule 2), never covering an applicant's own unsubmitted draft.
    let mut staff: Vec<String> = Vec::new();
    if actor.is_staff() {
        for grant in actor.roles.iter().filter(|g| matches!(g.role, Role::ComplaintsOfficer | Role::Manager)) {
            if let Some(service) = grant.scope_service_id {
                staff.push("(c.confidential = 1 AND c.service_id = ?)".into());
                binds.push(SqlValue::Int(service));
            } else {
                staff.push("c.confidential = 1".into());
            }
        }
        let all_services = actor.roles.iter().any(|g| {
            matches!(g.role, Role::Intake | Role::Manager | Role::Finance | Role::Specialist)
                && g.scope_service_id.is_none()
        });
        if all_services {
            staff.push("c.confidential = 0".into());
        } else {
            let mut scoped: Vec<i64> = actor
                .roles
                .iter()
                .filter(|g| matches!(g.role, Role::Intake | Role::Manager | Role::Finance | Role::Specialist))
                .filter_map(|g| g.scope_service_id)
                .collect();
            scoped.sort_unstable();
            scoped.dedup();
            if !scoped.is_empty() {
                let placeholders = vec!["?"; scoped.len()].join(", ");
                staff.push(format!("(c.confidential = 0 AND c.service_id IN ({placeholders}))"));
                binds.extend(scoped.into_iter().map(SqlValue::Int));
            }
        }
    }
    if !staff.is_empty() {
        let ors = staff.iter().map(|b| format!("({b})")).collect::<Vec<_>>().join(" OR ");
        branches.push(format!("NOT {APPLICANT_PRIVATE_DRAFT} AND ({ors})"));
    }

    // Applicant branches (rule 3).
    branches.push(
        "(c.applicant_org_id IS NULL AND c.applicant_user_id = ?) \
         OR (c.applicant_org_id IS NOT NULL AND EXISTS (SELECT 1 FROM memberships m \
             WHERE m.organisation_id = c.applicant_org_id AND m.user_id = ? AND m.status = 'active')) \
         OR EXISTS (SELECT 1 FROM case_representatives r WHERE r.case_id = c.id AND r.user_id = ? AND r.status = 'active')"
            .into(),
    );
    binds.extend([SqlValue::Int(uid), SqlValue::Int(uid), SqlValue::Int(uid)]);

    let ors = branches.iter().map(|b| format!("({b})")).collect::<Vec<_>>().join(" OR ");
    ScopeSql { sql: format!("({denial} AND ({ors}))"), binds }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{RoleGrant, UserKind};
    use crate::db::bind_all_scalar;
    use sqlx::SqlitePool;

    const NOW: &str = "2026-10-07T00:00:00.000Z";

    async fn exec(pool: &SqlitePool, sql: &str) {
        sqlx::query(sql).execute(pool).await.unwrap_or_else(|e| panic!("{sql}: {e}"));
    }

    async fn user(pool: &SqlitePool, id: i64, kind: &str) {
        exec(
            pool,
            &format!(
                "INSERT INTO users (id, email, display_name, kind, created_at) VALUES ({id}, 'u{id}@t.invalid', 'User {id}', '{kind}', '{NOW}')"
            ),
        )
        .await;
    }

    async fn case(
        pool: &SqlitePool,
        id: i64,
        service: i64,
        applicant: Option<i64>,
        org: Option<i64>,
        confidential: bool,
    ) {
        let a = applicant.map(|v| v.to_string()).unwrap_or("NULL".into());
        let o = org.map(|v| v.to_string()).unwrap_or("NULL".into());
        exec(
            pool,
            &format!(
                "INSERT INTO cases (id, service_id, service_version_id, module, title, status, applicant_user_id, applicant_org_id, \
                 applicant_name, intake_channel, confidential, created_at, updated_at) \
                 VALUES ({id}, {service}, {service}, 'generic', 'Case {id}', 'submitted', {a}, {o}, 'Applicant', 'online', {}, '{NOW}', '{NOW}')",
                confidential as i64
            ),
        )
        .await;
    }

    fn actor(id: i64, kind: UserKind, roles: &[(Role, Option<i64>)]) -> Actor {
        Actor {
            user_id: id,
            kind,
            roles: roles.iter().map(|(r, s)| RoleGrant { role: *r, scope_service_id: *s }).collect(),
            display_name: format!("User {id}"),
            mfa_passed: true,
        }
    }

    async fn scope_ids(pool: &SqlitePool, a: &Actor) -> Vec<i64> {
        let scope = case_scope_sql(a);
        let sql = format!("SELECT c.id FROM cases c WHERE {} ORDER BY c.id", scope.sql);
        bind_all_scalar(sqlx::query_scalar(&sql), &scope.binds).fetch_all(pool).await.unwrap()
    }

    /// Builds the fixture world and checks every (actor, case) pair: the expected access, and agreement
    /// between `case_access` and `case_scope_sql`.
    #[tokio::test]
    async fn access_matrix_and_scope_agree() {
        let (state, _dir) = crate::state::test_support::test_state().await;
        let pool = &state.db;

        // Users.
        // 1 owner applicant, 2 org member (active), 3 org member (revoked), 4 representative (active),
        // 5 representative (revoked), 6 stranger resident.
        for id in 1..=6 {
            user(pool, id, "resident").await;
        }
        // Staff: 10 intake, 11 specialist scoped to service 1, 12 specialist unscoped, 13 finance,
        // 14 field worker (task on case 100), 15 manager, 16 complaints officer, 17 sysadmin,
        // 18 manager who is the subject of complaint 103 (denied), 19 complaints officer denied on 103,
        // 20 intake assigned owner of 103 (assignment does not open confidential cases),
        // 21 field worker with task on confidential case 103, 22 sysadmin + finance,
        // 23 specialist scoped to service 2.
        for id in 10..=23 {
            user(pool, id, "staff").await;
        }

        // Services 1 (generic), 2 (generic), 3 (complaint) with a version each (ids equal service ids).
        for (sid, module) in [(1, "generic"), (2, "generic"), (3, "complaint")] {
            exec(pool, &format!("INSERT INTO services (id, slug, name, category, module, department, created_at) VALUES ({sid}, 's{sid}', 'S{sid}', 'Cat', '{module}', 'Dept', '{NOW}')")).await;
            exec(pool, &format!("INSERT INTO service_versions (id, service_id, version, status, definition_json, created_at) VALUES ({sid}, {sid}, 1, 'published', '{{}}', '{NOW}')")).await;
        }

        // Organisation 1 with members 2 (active) and 3 (revoked).
        exec(pool, &format!("INSERT INTO organisations (id, name, created_at) VALUES (1, 'Island Builders', '{NOW}')"))
            .await;
        exec(pool, &format!("INSERT INTO memberships (organisation_id, user_id, invite_email, role, status, created_at) VALUES (1, 2, 'u2@t.invalid', 'owner', 'active', '{NOW}')")).await;
        exec(pool, &format!("INSERT INTO memberships (organisation_id, user_id, invite_email, role, status, created_at, revoked_at) VALUES (1, 3, 'u3@t.invalid', 'member', 'revoked', '{NOW}', '{NOW}')")).await;

        // Cases.
        case(pool, 100, 1, Some(1), Option::None, false).await; // personal case of user 1, service 1
        case(pool, 101, 2, Some(3), Some(1), false).await; // org case submitted by 3 (now revoked), service 2
        case(pool, 102, 1, Some(6), Option::None, false).await; // personal case of 6 with representatives 4 (active) and 5 (revoked)
        case(pool, 103, 3, Some(1), Option::None, true).await; // confidential complaint by user 1
        case(pool, 104, 2, Some(6), Option::None, false).await; // other case of 6, nobody special

        exec(pool, &format!("INSERT INTO case_representatives (case_id, user_id, basis, status, created_at) VALUES (102, 4, 'Letter', 'active', '{NOW}')")).await;
        exec(pool, &format!("INSERT INTO case_representatives (case_id, user_id, basis, status, created_at, revoked_at) VALUES (102, 5, 'Letter', 'revoked', '{NOW}', '{NOW}')")).await;
        exec(pool, &format!("INSERT INTO tasks (case_id, kind, title, instructions, assigned_to, status, created_at) VALUES (100, 'general', 'T', 'Do it', 14, 'open', '{NOW}')")).await;
        exec(pool, &format!("INSERT INTO tasks (case_id, kind, title, instructions, assigned_to, status, created_at) VALUES (103, 'general', 'T', 'Do it', 21, 'open', '{NOW}')")).await;
        exec(
            pool,
            &format!(
                "INSERT INTO case_assignments (case_id, user_id, role, assigned_at) VALUES (103, 20, 'owner', '{NOW}')"
            ),
        )
        .await;
        exec(pool, &format!("INSERT INTO case_assignments (case_id, user_id, role, assigned_at) VALUES (103, 18, 'collaborator', '{NOW}')")).await;
        exec(pool, "INSERT INTO complaint_subjects (case_id, staff_user_id) VALUES (103, 18)").await;
        exec(pool, &format!("INSERT INTO case_access_denials (case_id, user_id, reason, created_at) VALUES (103, 18, 'Subject of complaint', '{NOW}')")).await;
        exec(pool, &format!("INSERT INTO case_access_denials (case_id, user_id, reason, created_at) VALUES (103, 19, 'Conflict of interest', '{NOW}')")).await;
        // A denial also overrides applicant access: user 6 is denied on their own case 104.
        exec(pool, &format!("INSERT INTO case_access_denials (case_id, user_id, reason, created_at) VALUES (104, 6, 'Test denial', '{NOW}')")).await;

        use CaseAccess::*;
        const M: CaseAccess = Staff { can_manage: true };
        const R: CaseAccess = Staff { can_manage: false };
        let r = UserKind::Resident;
        let s = UserKind::Staff;
        // (name, actor, [access for cases 100..=104])
        let matrix: Vec<(&str, Actor, [CaseAccess; 5])> = vec![
            ("owner applicant", actor(1, r, &[]), [Applicant, None, None, Applicant, None]),
            ("org member active", actor(2, r, &[]), [None, Applicant, None, None, None]),
            ("org member revoked (submitter)", actor(3, r, &[]), [None, None, None, None, None]),
            ("representative active", actor(4, r, &[]), [None, None, Applicant, None, None]),
            ("representative revoked", actor(5, r, &[]), [None, None, None, None, None]),
            ("applicant denied on own case", actor(6, r, &[]), [None, None, Applicant, None, None]),
            ("intake", actor(10, s, &[(Role::Intake, Option::None)]), [M, M, M, None, M]),
            ("specialist scoped to service 1", actor(11, s, &[(Role::Specialist, Some(1))]), [M, None, M, None, None]),
            ("specialist unscoped", actor(12, s, &[(Role::Specialist, Option::None)]), [M, M, M, None, M]),
            ("finance", actor(13, s, &[(Role::Finance, Option::None)]), [R, R, R, None, R]),
            ("scoped intake", actor(10, s, &[(Role::Intake, Some(1))]), [M, None, M, None, None]),
            ("scoped finance", actor(13, s, &[(Role::Finance, Some(1))]), [R, None, R, None, None]),
            ("scoped manager", actor(15, s, &[(Role::Manager, Some(1))]), [M, None, M, None, None]),
            ("scoped complaint manager", actor(15, s, &[(Role::Manager, Some(3))]), [None, None, None, M, None]),
            (
                "field worker with task",
                actor(14, s, &[(Role::FieldWorker, Option::None)]),
                [TaskOnly, None, None, None, None],
            ),
            ("manager", actor(15, s, &[(Role::Manager, Option::None)]), [M, M, M, M, M]),
            (
                "complaints officer",
                actor(16, s, &[(Role::ComplaintsOfficer, Option::None)]),
                [None, None, None, M, None],
            ),
            ("sysadmin alone", actor(17, s, &[(Role::Sysadmin, Option::None)]), [None, None, None, None, None]),
            ("manager subject of complaint", actor(18, s, &[(Role::Manager, Option::None)]), [M, M, M, None, M]),
            (
                "complaints officer denied",
                actor(19, s, &[(Role::ComplaintsOfficer, Option::None)]),
                [None, None, None, None, None],
            ),
            ("intake assignee of confidential case", actor(20, s, &[(Role::Intake, Option::None)]), [M, M, M, None, M]),
            (
                "field worker task on confidential",
                actor(21, s, &[(Role::FieldWorker, Option::None)]),
                [None, None, None, None, None],
            ),
            (
                "sysadmin + finance",
                actor(22, s, &[(Role::Sysadmin, Option::None), (Role::Finance, Option::None)]),
                [R, R, R, None, R],
            ),
            ("specialist scoped to service 2", actor(23, s, &[(Role::Specialist, Some(2))]), [None, M, None, None, M]),
        ];

        let case_ids = [100, 101, 102, 103, 104];
        for (name, a, expected) in &matrix {
            let mut conn = pool.acquire().await.unwrap();
            let listed = scope_ids(pool, a).await;
            for (i, case_id) in case_ids.iter().enumerate() {
                let got = case_access(&mut conn, a, *case_id).await.unwrap();
                assert_eq!(got, expected[i], "{name}: case_access for case {case_id}");
                let should_list = matches!(got, Applicant | Staff { .. });
                assert_eq!(
                    listed.contains(case_id),
                    should_list,
                    "{name}: case_scope_sql disagrees with case_access ({got:?}) for case {case_id}"
                );
            }
        }

        // require_case: not_found for None, CaseRow for visible.
        let mut conn = pool.acquire().await.unwrap();
        let err = require_case(&mut conn, &actor(17, s, &[(Role::Sysadmin, Option::None)]), 100).await.unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::NotFound);
        let (row, acc) = require_case(&mut conn, &actor(1, r, &[]), 100).await.unwrap();
        assert_eq!((row.id, acc), (100, Applicant));
        assert_eq!(case_access(&mut conn, &actor(1, r, &[]), 999).await.unwrap(), None);
        // Revoking the representative takes effect immediately.
        sqlx::query("UPDATE case_representatives SET status = 'revoked' WHERE case_id = 102 AND user_id = 4")
            .execute(&mut *conn)
            .await
            .unwrap();
        assert_eq!(case_access(&mut conn, &actor(4, r, &[]), 102).await.unwrap(), None);
        assert!(scope_ids(pool, &actor(4, r, &[])).await.is_empty());
        // The system actor sees nothing through authz.
        assert!(scope_ids(pool, &Actor::system()).await.is_empty());
    }

    /// An applicant's own draft is theirs alone until submitted; an assisted draft stays visible to staff.
    #[tokio::test]
    async fn applicant_drafts_are_private_until_submitted() {
        let (state, _dir) = crate::state::test_support::test_state().await;
        let pool = &state.db;
        user(pool, 1, "resident").await;
        user(pool, 10, "staff").await;
        exec(pool, &format!("INSERT INTO services (id, slug, name, category, module, department, created_at) VALUES (1, 's1', 'S1', 'Cat', 'generic', 'Dept', '{NOW}')")).await;
        exec(pool, &format!("INSERT INTO service_versions (id, service_id, version, status, definition_json, created_at) VALUES (1, 1, 1, 'published', '{{}}', '{NOW}')")).await;
        case(pool, 200, 1, Some(1), Option::None, false).await;
        exec(pool, "UPDATE cases SET status = 'draft' WHERE id = 200").await;
        case(pool, 201, 1, Option::None, Option::None, false).await;
        exec(
            pool,
            "UPDATE cases SET status = 'draft', intake_channel = 'phone', recorded_by_user_id = 10 WHERE id = 201",
        )
        .await;
        let owner = actor(1, UserKind::Resident, &[]);
        let mut conn = pool.acquire().await.unwrap();
        for a in [
            actor(10, UserKind::Staff, &[(Role::Intake, Option::None)]),
            actor(10, UserKind::Staff, &[(Role::Manager, Option::None)]),
            actor(10, UserKind::Staff, &[(Role::Finance, Option::None)]),
        ] {
            assert_eq!(case_access(&mut conn, &a, 200).await.unwrap(), CaseAccess::None);
            assert!(case_access(&mut conn, &a, 201).await.unwrap().is_staff());
            assert_eq!(scope_ids(pool, &a).await, vec![201]);
        }
        assert_eq!(case_access(&mut conn, &owner, 200).await.unwrap(), CaseAccess::Applicant);
        assert_eq!(scope_ids(pool, &owner).await, vec![200]);
        // Once submitted, staff roles apply again.
        exec(pool, "UPDATE cases SET status = 'submitted' WHERE id = 200").await;
        let intake = actor(10, UserKind::Staff, &[(Role::Intake, Option::None)]);
        assert!(case_access(&mut conn, &intake, 200).await.unwrap().can_manage());
        assert_eq!(scope_ids(pool, &intake).await, vec![200, 201]);
    }

    #[test]
    fn role_names_roundtrip() {
        for r in Role::ALL {
            assert_eq!(Role::parse(r.as_str()), Some(r));
        }
        assert_eq!(serde_json::to_string(&Role::FieldWorker).unwrap(), "\"field_worker\"");
    }
}
