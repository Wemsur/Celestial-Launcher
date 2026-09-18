use crate::worlds::{DisplayStatus, WorldType};

#[derive(Debug, Clone, Default)]
pub struct AttachedWorldData {
    pub display_status: DisplayStatus,
    pub project_id: Option<String>,
    pub content_kind: Option<String>,
}

impl AttachedWorldData {
    pub async fn get_for_world(
        instance_id: &str,
        world_type: WorldType,
        world_id: &str,
        exec: impl sqlx::Executor<'_, Database = sqlx::Sqlite>,
    ) -> crate::Result<Option<Self>> {
        let world_type = world_type.as_str();

        let attached_data = sqlx::query!(
            "
			SELECT display_status, project_id, content_kind
			FROM attached_world_data
			WHERE instance_id = ? and world_type = ? and world_id = ?
			",
            instance_id,
            world_type,
            world_id,
        )
        .fetch_optional(exec)
        .await?;

        Ok(attached_data.map(|row| AttachedWorldData {
            display_status: DisplayStatus::from_string(&row.display_status),
            project_id: row.project_id,
            content_kind: row.content_kind,
        }))
    }

    pub async fn remove_for_world(
        instance_id: &str,
        world_type: WorldType,
        world_id: &str,
        exec: impl sqlx::Executor<'_, Database = sqlx::Sqlite>,
    ) -> crate::Result<()> {
        let world_type = world_type.as_str();

        sqlx::query!(
            "
			DELETE FROM attached_world_data
			WHERE instance_id = ? and world_type = ? and world_id = ?
			",
            instance_id,
            world_type,
            world_id,
        )
        .execute(exec)
        .await?;

        Ok(())
    }
}

pub async fn set_display_status(
    instance_id: &str,
    world_type: WorldType,
    world_id: &str,
    display_status: DisplayStatus,
    exec: impl sqlx::Executor<'_, Database = sqlx::Sqlite>,
) -> crate::Result<()> {
    let world_type = world_type.as_str();
    let display_status = display_status.as_str();

    sqlx::query!(
        "
		INSERT INTO attached_world_data (instance_id, world_type, world_id, display_status)
		VALUES (?, ?, ?, ?)
		ON CONFLICT (instance_id, world_type, world_id) DO UPDATE
			SET display_status = excluded.display_status
		",
        instance_id,
        world_type,
        world_id,
        display_status,
    )
    .execute(exec)
    .await?;

    Ok(())
}

pub async fn set_project_id(
    instance_id: &str,
    world_type: WorldType,
    world_id: &str,
    project_id: &str,
    exec: impl sqlx::Executor<'_, Database = sqlx::Sqlite>,
) -> crate::Result<()> {
    let world_type = world_type.as_str();

    sqlx::query!(
        "
		INSERT INTO attached_world_data (instance_id, world_type, world_id, project_id)
		VALUES (?, ?, ?, ?)
		ON CONFLICT (instance_id, world_type, world_id) DO UPDATE
			SET project_id = excluded.project_id
		",
        instance_id,
        world_type,
        world_id,
        project_id,
    )
    .execute(exec)
    .await?;

    Ok(())
}

pub async fn set_content_kind(
    instance_id: &str,
    world_type: WorldType,
    world_id: &str,
    content_kind: &str,
    exec: impl sqlx::Executor<'_, Database = sqlx::Sqlite>,
) -> crate::Result<()> {
    let world_type = world_type.as_str();

    sqlx::query!(
        "
		INSERT INTO attached_world_data (instance_id, world_type, world_id, content_kind)
		VALUES (?, ?, ?, ?)
		ON CONFLICT (instance_id, world_type, world_id) DO UPDATE
			SET content_kind = excluded.content_kind
		",
        instance_id,
        world_type,
        world_id,
        content_kind,
    )
    .execute(exec)
    .await?;

    Ok(())
}
