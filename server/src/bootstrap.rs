//! Explicit local bootstrap commands; these do not seed fictional people or requests.
use crate::{
    AppResult, AppState,
    auth::{
        UserKind, password,
        users::{self, NewUser},
    },
    authz::Role,
    db,
};
use rand::RngCore;

pub async fn create_admin(state: &AppState, email: &str, name: &str) -> AppResult<String> {
    let email = email.trim().to_lowercase();
    if !email.contains('@') || name.trim().is_empty() {
        return Err(crate::AppError::validation_msg("An email address and name are required."));
    }
    let mut bytes = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut bytes);
    let password = hex::encode(bytes);
    let mut tx = db::write_tx(&state.db).await?;
    let first: bool =
        sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM role_grants WHERE role='manager' AND revoked_at IS NULL)")
            .fetch_one(&mut *tx)
            .await?;
    let id = users::create_user(
        &mut tx,
        NewUser {
            email,
            display_name: name.trim().into(),
            phone: None,
            kind: UserKind::Staff,
            password_hash: Some(password::hash(&password)?),
            totp_secret: None,
            totp_enabled: false,
            persona_key: None,
            job_title: Some("Council administrator".into()),
        },
    )
    .await?;
    sqlx::query("UPDATE users SET must_change_password=1 WHERE id=?").bind(id).execute(&mut *tx).await?;
    users::grant_role(&mut tx, id, Role::Sysadmin, None, None).await?;
    if first {
        users::grant_role(&mut tx, id, Role::Manager, None, None).await?;
    }
    crate::audit::record(
        &mut tx,
        None,
        "bootstrap.create_admin",
        "user",
        Some(id),
        serde_json::json!({"initial_manager":first}),
    )
    .await?;
    tx.commit().await?;
    Ok(password)
}

pub async fn seed_catalogue(state: &AppState) -> AppResult<()> {
    let mut tx = db::write_tx(&state.db).await?;
    crate::services::seed(&mut tx, state).await?;
    crate::documents::seed(&mut tx, state).await?;
    crate::operations::seed(&mut tx, state).await?;
    crate::finance::seed(&mut tx, state).await?;
    crate::records::seed(&mut tx, state).await?;
    crate::seed::load_holidays(&mut tx, &state.cfg.seed_data_dir.join("holidays.csv")).await?;
    tx.commit().await?;
    Ok(())
}

pub async fn seed_fresh_demo(state: &AppState) -> AppResult<()> {
    if state.cfg.demo_mode {
        let fresh: bool =
            sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM users) AND NOT EXISTS(SELECT 1 FROM cases)")
                .fetch_one(&state.db)
                .await?;
        if fresh {
            crate::seed::seed_demo(state).await?;
        }
    }
    Ok(())
}
