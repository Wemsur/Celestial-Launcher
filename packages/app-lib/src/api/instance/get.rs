use crate::state::libraries;
use crate::state::{Instance, InstanceLaunchOverrides, InstanceMetadata, State};
use std::collections::HashSet;
use std::path::PathBuf;

#[tracing::instrument]
pub async fn get(instance_id: &str) -> crate::Result<Option<InstanceMetadata>> {
    let state = State::get().await?;

    // Try DB first (legacy/migrated instances)
    if let Some(meta) =
        crate::state::get_instance(instance_id, &state.pool).await?
    {
        return Ok(Some(meta));
    }

    // Fall back to JSON-backed instances from libraries
    let json_instances =
        crate::state::libraries::list_instances_from_json(&state).await?;
    for instance in &json_instances {
        if instance.id == instance_id {
            return Ok(Some(instance_metadata_from_instance(instance)));
        }
    }

    Ok(None)
}

#[tracing::instrument]
pub async fn get_many(
    instance_ids: &[&str],
) -> crate::Result<Vec<InstanceMetadata>> {
    let state = State::get().await?;

    let mut result: HashSet<String> = HashSet::new();

    // Collect from DB
    for &id in instance_ids {
        if let Some(_meta) =
            crate::state::get_instance(id, &state.pool).await?
        {
            result.insert(id.to_string());
        }
    }

    // Collect from JSON libraries
    let json_instances =
        crate::state::libraries::list_instances_from_json(&state).await?;
    for instance in &json_instances {
        if !result.contains(&instance.id) {
            result.insert(instance.id.clone());
        }
    }

    // Build full list
    let mut out = Vec::new();
    for &id in instance_ids {
        if result.contains(id) {
            if let Some(meta) =
                crate::state::get_instance(id, &state.pool).await?
            {
                out.push(meta);
            } else {
                // JSON-backed instance
                if let Some(inst) =
                    json_instances.iter().find(|i| i.id == id)
                {
                    out.push(instance_metadata_from_instance(inst));
                }
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
            result.push(instance_metadata_from_instance(instance));
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
) -> InstanceMetadata {
    // Try to read instance.json to get game_version/loader info
    let dir = if let Some(state) = State::get_if_initialized() {
        libraries::resolve_instance_dir_with_dirs(&state.directories, &instance.path)
    } else {
        libraries::resolve_instance_dir_with_dirs(
            &crate::state::DirectoryInfo {
                settings_dir: std::path::PathBuf::new(),
                config_dir: std::path::PathBuf::new(),
                app_identifier: String::new(),
            },
            &instance.path,
        )
    };
    let (game_version, loader, loader_version) =
        if let Ok(Some(json)) = libraries::InstanceJson::read_from_dir(&dir) {
            (json.game_version, json.loader, json.loader_version)
        } else {
            (None, None, None)
        };

    // Fallback: detect version and loader from the filesystem
    let game_version = game_version.or_else(|| {
        libraries::detect_game_version_from_dir(&dir)
    });
    let loader = loader.or_else(|| {
        if game_version.as_ref().is_some_and(|gv| gv.is_empty()) {
            None
        } else {
            Some(libraries::detect_loader_from_dir(&dir).as_str().to_string())
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
        loader_version,
        created: chrono::Utc::now(),
        modified: chrono::Utc::now(),
    };
    InstanceMetadata {
        instance: instance.clone(),
        applied_content_set: content_set,
        link: libraries::InstanceJson::read_from_dir(&dir)
            .ok()
            .flatten()
            .and_then(|j| j.link)
            .unwrap_or(crate::state::InstanceLink::Unmanaged),
        shared_instance: None,
        quarantined: false,
        groups: vec![],
        launch_overrides: InstanceLaunchOverrides::empty(instance.id.clone()),
    }
}
