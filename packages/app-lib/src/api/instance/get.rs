use crate::state::libraries;
use crate::state::{Instance, InstanceMetadata, State};
use std::collections::HashSet;
use std::path::PathBuf;

#[tracing::instrument]
pub async fn get(instance_id: &str) -> crate::Result<Option<InstanceMetadata>> {
    let state = State::get().await?;

    // First try JSON-backed instances (multi-library)
    let json_instances =
        crate::state::libraries::list_instances_from_json(&state).await?;
    if let Some(inst) = json_instances.iter().find(|i| i.id == instance_id) {
        return Ok(Some(instance_metadata_from_instance(inst)?));
    }

    // Fall back to DB-backed instances
    if let Some(meta) =
        crate::state::get_instance(instance_id, &state.pool).await?
    {
        return Ok(Some(meta));
    }

    Ok(None)
}

/// Get an instance by ID, checking JSON-backed instances first, then DB.
#[tracing::instrument]
pub async fn get_by_id(
    instance_id: &str,
) -> crate::Result<Option<InstanceMetadata>> {
    let state = State::get().await?;

    // JSON-backed instances take priority
    let json_instances =
        crate::state::libraries::list_instances_from_json(&state).await?;
    if let Some(inst) = json_instances.iter().find(|i| i.id == instance_id) {
        return Ok(Some(instance_metadata_from_instance(inst)?));
    }

    // Fall back to DB-backed instances
    crate::state::get_instance(instance_id, &state.pool).await
}

#[tracing::instrument]
pub async fn get_many(
    instance_ids: &[&str],
) -> crate::Result<Vec<InstanceMetadata>> {
    let state = State::get().await?;

    let mut result: HashSet<String> = HashSet::new();
    let mut out = Vec::new();

    // Collect JSON-backed instances first
    let json_instances =
        crate::state::libraries::list_instances_from_json(&state).await?;
    for instance in &json_instances {
        if !result.contains(&instance.id) {
            result.insert(instance.id.clone());
        }
    }

    // Collect DB-backed instances
    for &id in instance_ids {
        if result.contains(id) {
            continue;
        }
        if let Some(_meta) =
            crate::state::get_instance(id, &state.pool).await?
        {
            result.insert(id.to_string());
        }
    }

    // Build full list
    for &id in instance_ids {
        if result.contains(id) {
            // Try JSON first, then DB
            if let Some(inst) =
                json_instances.iter().find(|i| i.id == id)
            {
                out.push(instance_metadata_for_listing(inst));
            } else if let Some(meta) =
                crate::state::get_instance(id, &state.pool).await?
            {
                out.push(meta);
            }
        }
    }
    Ok(out)
}

#[tracing::instrument]
pub async fn list(library_path: Option<&str>) -> crate::Result<Vec<InstanceMetadata>> {
    let state = State::get().await?;

    // Get DB-backed instances (legacy + migrated)
    let db_instances = crate::state::list_instances(&state.pool).await?;

    // Get JSON-backed instances from multi-library scan
    let json_instances =
        crate::state::libraries::list_instances_from_json(&state).await?;

    // Deduplicate by resolved absolute path
    let mut seen_paths: std::collections::HashSet<PathBuf> =
        std::collections::HashSet::new();
    let mut result: Vec<InstanceMetadata> = Vec::new();

    // First pass: add JSON-backed instances (they have absolute paths)
    for instance in &json_instances {
        let resolved = libraries::resolve_instance_dir(&state, &instance.path);
        // Apply library filter if specified
        if let Some(lib_path) = library_path {
            // Treat empty string as "no filter" to avoid matching all instances
            if lib_path.is_empty() {
                // fall through
            } else if !resolved.starts_with(lib_path) {
                continue;
            }
        }
        if seen_paths.insert(resolved) {
            result.push(instance_metadata_for_listing(instance));
        }
    }

    // Second pass: add DB-backed instances not already covered
    for meta in &db_instances {
        let resolved =
            libraries::resolve_instance_dir(&state, &meta.instance.path);
        // Only include DB-backed instances whose path actually exists.
        // DB instances that were moved to a different library (e.g.
        // Modrinth profiles) would otherwise show up with stale paths.
        if !resolved.exists() {
            continue;
        }
        // Apply library filter consistently
        if let Some(lib_path) = library_path {
            if !lib_path.is_empty() && !resolved.starts_with(lib_path) {
                continue;
            }
        }
        if seen_paths.insert(resolved) {
            result.push(meta.clone());
        }
    }

    Ok(result)
}

pub(crate) fn instance_metadata_from_instance(
	instance: &Instance,
) -> crate::Result<InstanceMetadata> {
	let dir = json_instance_dir(instance);
	let settings = libraries::read_json_instance_settings(&dir, instance)?;
	Ok(instance_metadata_from_settings(instance, &dir, settings))
}

fn json_instance_dir(instance: &Instance) -> PathBuf {
	if let Some(state) = State::get_if_initialized() {
		libraries::resolve_instance_dir_with_dirs(&state.directories, &instance.path)
	} else {
		libraries::resolve_instance_dir_with_dirs(
			&crate::state::DirectoryInfo {
				settings_dir: PathBuf::new(),
				config_dir: PathBuf::new(),
				app_identifier: String::new(),
			},
			&instance.path,
		)
	}
}

/// Keep damaged instances visible without letting their unreadable settings be launched.
fn instance_metadata_for_listing(instance: &Instance) -> InstanceMetadata {
	let dir = json_instance_dir(instance);
	match libraries::read_json_instance_settings(&dir, instance) {
		Ok(settings) => instance_metadata_from_settings(instance, &dir, settings),
		Err(error) => {
			tracing::warn!(instance_id = %instance.id, "Cannot read instance settings: {error}");
			let mut broken = instance.clone();
			broken.install_stage = crate::state::InstanceInstallStage::Broken;
			let settings = libraries::JsonInstanceSettings::empty(&broken);
			instance_metadata_from_settings(&broken, &dir, settings)
		}
	}
}

fn instance_metadata_from_settings(
	instance: &Instance,
	dir: &std::path::Path,
	settings: libraries::JsonInstanceSettings,
) -> InstanceMetadata {
	let game_version = settings.game_version.or_else(|| {
		libraries::detect_game_version_from_dir(dir)
	});
	let loader = settings.loader.or_else(|| {
		if game_version.as_ref().is_some_and(|gv| gv.is_empty()) {
			None
		} else {
			Some(libraries::detect_loader_from_dir(dir).as_str().to_string())
		}
	});
	let content_set = crate::state::ContentSet {
		id: instance.applied_content_set_id.clone().unwrap_or_default(),
		instance_id: instance.id.clone(),
		name: "Applied Content Set".to_string(),
		source_kind: crate::state::ContentSourceKind::Local,
		status: crate::state::ContentSetStatus::Available,
		game_version: game_version.unwrap_or_default(),
		protocol_version: None,
		loader: crate::state::ModLoader::from_string(&loader.unwrap_or_default()),
		loader_version: settings.loader_version,
		created: chrono::Utc::now(),
		modified: chrono::Utc::now(),
	};

	InstanceMetadata {
		instance: Instance {
			update_channel: settings.update_channel,
			..instance.clone()
		},
		icon_config: None,
		applied_content_set: content_set,
		link: settings.link,
		shared_instance: None,
		quarantined: false,
		group_ids: crate::state::instance_groups::group_ids_for(&instance.id),
		synced_options: settings.synced_options,
		launch_overrides: settings.launch_overrides,
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::state::{InstanceLaunchOverridesData, MemorySettings};

	fn instance(dir: &std::path::Path) -> Instance {
		libraries::InstanceJson::default().to_instance_with_format(
			dir.to_string_lossy().as_ref(),
			libraries::InstanceFormat::Minecraft,
		)
	}

	#[test]
	fn lightweight_memory_is_returned_to_instance_settings() {
		let dir = tempfile::tempdir().unwrap();
		let instance = instance(dir.path());
		let mut overrides = InstanceLaunchOverridesData::new(instance.id.clone());
		overrides.memory = Some(MemorySettings { maximum: 8192 });
		libraries::CelestialJson {
			launch_overrides: Some(overrides),
			..libraries::CelestialJson::default()
		}.write_to_dir(dir.path()).unwrap();

		let metadata = instance_metadata_from_instance(&instance).unwrap();
		assert_eq!(metadata.launch_overrides.memory.unwrap().maximum, 8192);
		assert_eq!(
			instance_metadata_for_listing(&instance).launch_overrides.memory.unwrap().maximum,
			8192,
		);
	}

	#[test]
	fn damaged_settings_fail_detail_reads_but_remain_visible_as_broken() {
		let damaged = tempfile::tempdir().unwrap();
		let healthy = tempfile::tempdir().unwrap();
		std::fs::write(damaged.path().join("celestial.json"), "{").unwrap();
		let damaged_instance = instance(damaged.path());
		assert!(instance_metadata_from_instance(&damaged_instance).is_err());
		let damaged_metadata = instance_metadata_for_listing(&damaged_instance);
		assert_eq!(damaged_metadata.instance.install_stage, crate::state::InstanceInstallStage::Broken);
		let healthy_metadata = instance_metadata_for_listing(&instance(healthy.path()));
		assert_eq!(healthy_metadata.instance.install_stage, crate::state::InstanceInstallStage::Installed);
	}
}
