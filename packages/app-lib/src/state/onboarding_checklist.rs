use sqlx::{Executor, Sqlite, SqlitePool};

use crate::State;

#[derive(Clone, Debug)]
#[cfg_attr(
    feature = "export-ts",
    derive(ts_rs::TS, postcard_bindgen::PostcardBindings)
)]
#[serde_binhum::serde_binhum]
pub struct OnboardingChecklist {
    pub has_created_instance: bool,
    pub has_logged_into_minecraft: bool,
    pub has_logged_into_modrinth: bool,
    pub show_checklist: bool,
}

pub(crate) enum OnboardingChecklistItem {
    CreatedInstance,
    LoggedIntoMinecraft,
    LoggedIntoModrinth,
}

/// Lightweight row holder for the onboarding_checklist table.
#[derive(Debug, sqlx::FromRow)]
struct OnboardingRow {
    #[sqlx(rename = "has_created_instance")]
    has_created_instance: i64,
    #[sqlx(rename = "has_logged_into_minecraft")]
    has_logged_into_minecraft: i64,
    #[sqlx(rename = "has_logged_into_modrinth")]
    has_logged_into_modrinth: i64,
    #[sqlx(rename = "show_checklist")]
    show_checklist: i64,
}

/// Read the onboarding-checklist row from SQLite and compute
/// `has_created_instance` by scanning the current library config.
///
/// This ensures the flag stays in sync with the JSON-based instance store
/// after migration: if no instances are found but the DB says `true`, we
/// flip it back to `false`; if instances exist but the DB says `false`,
/// we flip it to `true`.
pub(crate) async fn get_onboarding_checklist(
    exec: impl Executor<'_, Database = Sqlite>,
) -> crate::Result<OnboardingChecklist> {
    let row = sqlx::query!(
        "
        SELECT
            has_created_instance,
            has_logged_into_minecraft,
            has_logged_into_modrinth,
            show_checklist
        FROM onboarding_checklist
        WHERE id = 0
        ",
    )
    .fetch_one(exec)
    .await?;

    // Scan libraries for actual instances.
    let has_instances = {
        let state = State::get().await?;
        match crate::state::libraries::list_instances_from_json(&state).await {
            Ok(instances) => !instances.is_empty(),
            Err(e) => {
                tracing::warn!(
                    "get_onboarding_checklist: failed to scan instances: {e}"
                );
                false
            }
        }
    };

    // If the DB says false but instances exist, update the DB so future
    // reads don't need to re-scan.
    if has_instances && row.has_created_instance == 0 {
        let state = State::get().await?;
        let _ = state
            .pool
            .execute(sqlx::query(
                "UPDATE onboarding_checklist SET has_created_instance = TRUE WHERE id = 0",
            ))
            .await;
    }

    Ok(OnboardingChecklist {
        has_created_instance: row.has_created_instance == 1 || has_instances,
        has_logged_into_minecraft: row.has_logged_into_minecraft == 1,
        has_logged_into_modrinth: row.has_logged_into_modrinth == 1,
        show_checklist: row.show_checklist == 1,
    })
}

pub(crate) async fn mark_onboarding_checklist_item(
    item: OnboardingChecklistItem,
    pool: &sqlx::SqlitePool,
) -> crate::Result<Option<OnboardingChecklist>> {
    let result = match item {
        OnboardingChecklistItem::CreatedInstance => {
            sqlx::query!(
                "
                UPDATE onboarding_checklist
                SET has_created_instance = TRUE
                WHERE id = 0 AND has_created_instance = FALSE
                ",
            )
            .execute(pool)
            .await?
        }
        OnboardingChecklistItem::LoggedIntoMinecraft => {
            sqlx::query!(
                "
                UPDATE onboarding_checklist
                SET has_logged_into_minecraft = TRUE
                WHERE id = 0 AND has_logged_into_minecraft = FALSE
                ",
            )
            .execute(pool)
            .await?
        }
        OnboardingChecklistItem::LoggedIntoModrinth => {
            sqlx::query!(
                "
                UPDATE onboarding_checklist
                SET has_logged_into_modrinth = TRUE
                WHERE id = 0 AND has_logged_into_modrinth = FALSE
                ",
            )
            .execute(pool)
            .await?
        }
    };

    if result.rows_affected() == 0 {
        return Ok(None);
    }

    sqlx::query!(
        "
        UPDATE onboarding_checklist
        SET show_checklist = FALSE
        WHERE id = 0
            AND show_checklist = TRUE
            AND has_created_instance = TRUE
            AND has_logged_into_minecraft = TRUE
            AND has_logged_into_modrinth = TRUE
        "
    )
    .execute(pool)
    .await?;

    Ok(Some(get_onboarding_checklist(pool).await?))
}
