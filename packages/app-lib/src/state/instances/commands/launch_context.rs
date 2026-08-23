use crate::state::InstanceInstallStage;
use crate::state::instances::{
    InstanceLaunchContext, adapters::sqlite::instance_rows,
    playtime_to_storage,
};
use crate::state::{ModLoader, State};
use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

pub(crate) async fn get_instance_launch_context(
    instance_id: &str,
    pool: &SqlitePool,
) -> crate::Result<Option<InstanceLaunchContext>> {
    if let Some(context) =
        instance_rows::get_instance_launch_context(instance_id, pool).await?
    {
        return Ok(Some(context));
    }

    let state = State::get().await?;
    for instance in
        crate::state::libraries::list_instances_from_json(&state).await?
    {
        if instance.id == instance_id {
            return Ok(Some(json_backed_launch_context(&instance)));
        }
    }

    Ok(None)
}

fn json_backed_launch_context(
    instance: &crate::state::Instance,
) -> InstanceLaunchContext {
    use crate::state::{
        ContentSet, ContentSetStatus, ContentSourceKind, InstanceLaunchOverrides,
        InstanceLink,
    };
    // Try to read version info from instance.json
    let dir = if let Some(state) = State::get_if_initialized() {
        crate::state::libraries::resolve_instance_dir_with_dirs(&state.directories, &instance.path)
    } else {
        crate::state::libraries::resolve_instance_dir_with_dirs(
            &crate::state::DirectoryInfo {
                settings_dir: std::path::PathBuf::new(),
                config_dir: std::path::PathBuf::new(),
                app_identifier: String::new(),
            },
            &instance.path,
        )
    };
    let (game_version, loader, loader_version) =
        if let Ok(Some(json)) = crate::state::libraries::InstanceJson::read_from_dir(&dir) {
            (json.game_version, json.loader, json.loader_version)
        } else {
            (None, None, None)
        };

    // Fallback: detect version and loader from the filesystem
    let game_version = game_version.or_else(|| {
        crate::state::libraries::detect_game_version_from_dir(&dir)
    });
    let loader = loader.or_else(|| {
        if game_version.as_ref().is_some_and(|gv| gv.is_empty()) {
            None
        } else {
            Some(crate::state::libraries::detect_loader_from_dir(&dir).as_str().to_string())
        }
    });

    InstanceLaunchContext {
        instance: instance.clone(),
        applied_content_set: ContentSet {
            id: instance.applied_content_set_id.clone().unwrap_or_default(),
            instance_id: instance.id.clone(),
            name: "Applied Content Set".to_string(),
            source_kind: ContentSourceKind::Local,
            status: ContentSetStatus::Available,
            game_version: game_version.unwrap_or_default(),
            protocol_version: None,
            loader: ModLoader::from_string(&loader.unwrap_or_default()),
            loader_version,
            created: chrono::Utc::now(),
            modified: chrono::Utc::now(),
        },
        link: InstanceLink::Unmanaged,
        launch_overrides: InstanceLaunchOverrides::empty(instance.id.clone()),
    }
}

// ─── DB-backed operations ────────────────────────────────────────────────────

pub(crate) async fn set_instance_install_stage(
    instance_id: &str,
    install_stage: InstanceInstallStage,
    pool: &SqlitePool,
) -> crate::Result<()> {
    let install_stage = install_stage.as_str();
    let modified = Utc::now().timestamp();

    sqlx::query!(
        "
		UPDATE instances
		SET install_stage = ?, modified = ?
		WHERE id = ?
		",
        install_stage,
        modified,
        instance_id,
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub(crate) async fn set_applied_content_set_loader_version(
    instance_id: &str,
    loader_version: Option<&str>,
    pool: &SqlitePool,
) -> crate::Result<()> {
    let modified = Utc::now().timestamp();

    sqlx::query!(
        "
		UPDATE instance_content_sets
		SET loader_version = ?, modified = ?
		WHERE id = (
			SELECT applied_content_set_id
			FROM instances
			WHERE id = ?
		)
		",
        loader_version,
        modified,
        instance_id,
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub(crate) async fn set_applied_content_set_protocol_version(
    instance_id: &str,
    protocol_version: Option<u32>,
    pool: &SqlitePool,
) -> crate::Result<()> {
    let protocol_version = protocol_version.map(i64::from);
    let modified = Utc::now().timestamp();

    sqlx::query!(
        "
		UPDATE instance_content_sets
		SET protocol_version = ?, modified = ?
		WHERE id = (
			SELECT applied_content_set_id
			FROM instances
			WHERE id = ?
		)
		",
        protocol_version,
        modified,
        instance_id,
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub(crate) async fn set_instance_last_played(
    instance_id: &str,
    last_played: DateTime<Utc>,
    pool: &SqlitePool,
) -> crate::Result<()> {
    let last_played = last_played.timestamp();
    let modified = Utc::now().timestamp();

    sqlx::query!(
        "
		UPDATE instances
		SET last_played = ?, modified = ?
		WHERE id = ?
		",
        last_played,
        modified,
        instance_id,
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub(crate) async fn add_instance_recent_playtime(
    instance_id: &str,
    seconds: u64,
    pool: &SqlitePool,
) -> crate::Result<()> {
    if seconds == 0 {
        return Ok(());
    }

    let seconds = playtime_to_storage(seconds, "recent_time_played")?;
    let max_playtime = i64::MAX;
    let max_playtime_before_increment = max_playtime - seconds;
    let modified = Utc::now().timestamp();

    sqlx::query!(
        "
		UPDATE instances
		SET
			recent_time_played = CASE
				WHEN recent_time_played < 0 THEN ?
				WHEN recent_time_played > ? THEN ?
				ELSE recent_time_played + ?
			END,
			modified = ?
		WHERE id = ?
		",
        seconds,
        max_playtime_before_increment,
        max_playtime,
        seconds,
        modified,
        instance_id,
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub(crate) async fn mark_instance_playtime_submitted(
    instance_id: &str,
    recent_time_played: u64,
    pool: &SqlitePool,
) -> crate::Result<()> {
    if recent_time_played == 0 {
        return Ok(());
    }

    let recent_time_played =
        playtime_to_storage(recent_time_played, "recent_time_played")?;
    let max_playtime = i64::MAX;
    let max_playtime_before_increment = max_playtime - recent_time_played;
    let modified = Utc::now().timestamp();

    sqlx::query!(
        "
		UPDATE instances
		SET
			submitted_time_played = CASE
				WHEN submitted_time_played < 0 THEN ?
				WHEN submitted_time_played > ? THEN ?
				ELSE submitted_time_played + ?
			END,
			recent_time_played = 0,
			modified = ?
		WHERE id = ?
		",
        recent_time_played,
        max_playtime_before_increment,
        max_playtime,
        recent_time_played,
        modified,
        instance_id,
    )
    .execute(pool)
    .await?;

    Ok(())
}

// ─── JSON-backed operations ─────────────────────────────────────────────────

/// Update install_stage in the JSON sidecar for a JSON-backed instance.
pub(crate) async fn set_instance_install_stage_json(
    instance_path: &str,
    install_stage: InstanceInstallStage,
) -> crate::Result<()> {
    let dir = crate::state::libraries::resolve_instance_dir(
        &*State::get().await?,
        instance_path,
    );
    if let Some(mut json) =
        crate::state::libraries::InstanceJson::read_from_dir(&dir)?
    {
        json.install_stage = install_stage.as_str().to_string();
        json.write_to_dir(&dir)?;
    }
    Ok(())
}

/// Update last_played in the JSON sidecar for a JSON-backed instance.
/// Writes to InstanceJson if present; otherwise falls back to CelestialJson
/// for .minecraft instances without an instance.json sidecar.
pub(crate) async fn set_instance_last_played_json(
    instance_path: &str,
    last_played: DateTime<Utc>,
) -> crate::Result<()> {
    let dir = crate::state::libraries::resolve_instance_dir(
        &*State::get().await?,
        instance_path,
    );
    if let Some(mut json) =
        crate::state::libraries::InstanceJson::read_from_dir(&dir)?
    {
        json.last_played = Some(last_played);
        json.write_to_dir(&dir)?;
        return Ok(());
    }
    // Fall back to CelestialJson for .minecraft instances without instance.json
    if let Some(mut celestial) =
        crate::state::libraries::CelestialJson::read_from_dir(&dir)?
    {
        celestial.last_played = Some(last_played);
        celestial.write_to_dir(&dir)?;
    }
    Ok(())
}

/// Add recent playtime to the JSON sidecar for a JSON-backed instance.
/// Writes to InstanceJson if present; otherwise falls back to CelestialJson.
pub(crate) async fn add_instance_recent_playtime_json(
    instance_path: &str,
    seconds: u64,
) -> crate::Result<()> {
    if seconds == 0 {
        return Ok(());
    }
    let seconds = playtime_to_storage(seconds, "recent_time_played")?;
    let max_playtime = i64::MAX;
    let max_playtime_before_increment = max_playtime - seconds;

    let dir = crate::state::libraries::resolve_instance_dir(
        &*State::get().await?,
        instance_path,
    );
    if let Some(mut json) =
        crate::state::libraries::InstanceJson::read_from_dir(&dir)?
    {
        json.recent_time_played = match json.recent_time_played {
            v if v < seconds as u64 => seconds as u64,
            v if v > max_playtime_before_increment as u64 => {
                max_playtime as u64
            }
            v => v + seconds as u64,
        };
        json.write_to_dir(&dir)?;
        return Ok(());
    }
    // Fall back to CelestialJson for .minecraft instances without instance.json
    if let Some(mut celestial) =
        crate::state::libraries::CelestialJson::read_from_dir(&dir)?
    {
        celestial.recent_time_played = match celestial.recent_time_played {
            v if v < seconds as u64 => seconds as u64,
            v if v > max_playtime_before_increment as u64 => {
                max_playtime as u64
            }
            v => v + seconds as u64,
        };
        celestial.write_to_dir(&dir)?;
    }
    Ok(())
}

/// Mark playtime as submitted in the JSON sidecar for a JSON-backed instance.
pub(crate) async fn mark_instance_playtime_submitted_json(
    instance_path: &str,
    recent_time_played: u64,
) -> crate::Result<()> {
    if recent_time_played == 0 {
        return Ok(());
    }
    let recent_time_played =
        playtime_to_storage(recent_time_played, "recent_time_played")?;
    let max_playtime = i64::MAX;
    let max_playtime_before_increment = max_playtime - recent_time_played;

    let dir = crate::state::libraries::resolve_instance_dir(
        &*State::get().await?,
        instance_path,
    );
    if let Some(mut json) =
        crate::state::libraries::InstanceJson::read_from_dir(&dir)?
    {
        json.submitted_time_played = match json.submitted_time_played {
            v if v < recent_time_played as u64 => recent_time_played as u64,
            v if v > max_playtime_before_increment as u64 => {
                max_playtime as u64
            }
            v => v + recent_time_played as u64,
        };
        json.recent_time_played = 0;
        json.write_to_dir(&dir)?;
        return Ok(());
    }
    // Fall back to CelestialJson for .minecraft instances without instance.json
    if let Some(mut celestial) =
        crate::state::libraries::CelestialJson::read_from_dir(&dir)?
    {
        celestial.submitted_time_played = match celestial.submitted_time_played {
            v if v < recent_time_played as u64 => recent_time_played as u64,
            v if v > max_playtime_before_increment as u64 => {
                max_playtime as u64
            }
            v => v + recent_time_played as u64,
        };
        celestial.recent_time_played = 0;
        celestial.write_to_dir(&dir)?;
    }
    Ok(())
}
