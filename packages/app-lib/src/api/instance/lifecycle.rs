use crate::event::InstancePayloadType;
use crate::event::emit::emit_instance;
use crate::state::instances::adapters::sqlite::instance_rows;
use crate::state::{
    CreateInstance, EditInstance, Instance, InstanceLink, InstanceMetadata,
    ModLoader, State,
    InstanceIconConfig,
};

#[tracing::instrument]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn create(
    name: String,
    game_version: String,
    modloader: ModLoader,
    loader_version: Option<String>,
    icon_path: Option<String>,
    icon_config: Option<InstanceIconConfig>,
    link: InstanceLink,
    library_path: Option<String>,
) -> crate::Result<InstanceMetadata> {
    let state = State::get().await?;
    if let Some(icon_config) = &icon_config {
        super::icon::validate_generated_icon_config(icon_config)?;
    }
    let instance = crate::state::create_instance(
        CreateInstance {
            name,
            path: None,
            game_version,
            loader: modloader,
            loader_version,
            icon_path,
            icon_config,
            link,
            library_path,
        },
        &state,
    )
    .await?;

    emit_instance(&instance.id, InstancePayloadType::Created).await?;

    // DB-backed: verify it was written to the database
    if instance.applied_content_set_id.is_some() {
        crate::state::get_instance(&instance.id, &state.pool)
            .await?
            .ok_or_else(|| {
                crate::ErrorKind::InputError(
                    "Created instance could not be loaded".to_string(),
                )
                .into()
            })
    } else {
        // JSON-backed: build metadata from the instance directly
        Ok(crate::api::instance::get::instance_metadata_from_instance(
            &instance,
        ))
    }
}

pub async fn edit(
    instance_id: &str,
    patch: EditInstance,
) -> crate::Result<InstanceMetadata> {
    let state = State::get().await?;

    // JSON-backed instances (primary path)
    let json_instances =
        crate::state::libraries::list_instances_from_json(&state).await?;
    if json_instances.iter().any(|i| i.id == instance_id) {
        let mut found = false;
        for inst in &json_instances {
            if inst.id == instance_id {
                let dir =
                    crate::state::libraries::resolve_instance_dir(
                        &state,
                        &inst.path,
                    );
                let library_format = inst.library_format.clone();
                let mut saved = false;

                // Try instance.json first (Modrinth format, or .minecraft with sidecar)
                if let Some(mut instance_json) =
                    crate::state::libraries::InstanceJson::read_from_dir(&dir)?
                {
                    if let Some(name) = &patch.name {
                        instance_json.name = Some(name.clone());
                    }
                    if let Some(icon_path) = &patch.icon_path {
                        instance_json.icon_path = icon_path.clone();
                    }
                    if let Some(last_played) = &patch.last_played {
                        instance_json.last_played = *last_played;
                    }
                    if let Some(stpt) = patch.submitted_time_played {
                        instance_json.submitted_time_played = stpt;
                    }
                    if let Some(rtp) = patch.recent_time_played {
                        instance_json.recent_time_played = rtp;
                    }
                    if let Some(link) = &patch.link {
                        instance_json.link = Some(link.clone());
                    }
                    if let Some(launch_patch) = &patch.launch_overrides {
                        let overrides =
                            instance_json.launch_overrides.get_or_insert_with(|| {
                                crate::state::InstanceLaunchOverridesData::new(
                                    instance_id.to_string(),
                                )
                            });
                        if let Some(java_path) = &launch_patch.java_path {
                            overrides.java_path = java_path.clone();
                        }
                        if let Some(extra_launch_args) = &launch_patch.extra_launch_args {
                            overrides.extra_launch_args = extra_launch_args.clone();
                        }
                        if let Some(custom_env_vars) = &launch_patch.custom_env_vars {
                            overrides.custom_env_vars = custom_env_vars.clone();
                        }
                        if let Some(memory) = &launch_patch.memory {
                            overrides.memory = *memory;
                        }
                        if let Some(force_fullscreen) = &launch_patch.force_fullscreen {
                            overrides.force_fullscreen = *force_fullscreen;
                        }
                        if let Some(game_resolution) = &launch_patch.game_resolution {
                            overrides.game_resolution = *game_resolution;
                        }
                        if launch_patch.hooks.is_some() {
                            overrides.hooks = launch_patch.hooks.clone().unwrap_or_default();
                        }
                    }
                    if let Some(update_channel) = patch.update_channel {
                        instance_json.update_channel = update_channel;
                    }
                    if let Some(group_ids) = &patch.group_ids {
                        instance_json.groups = group_ids.clone();
                    }
                    if let Some(cs_patch) = &patch.content_set_patch {
                        if let Some(ref gv) = cs_patch.game_version {
                            instance_json.game_version = Some(gv.clone());
                        }
                        if let Some(ref ld) = cs_patch.loader {
                            instance_json.loader = Some(ld.as_str().to_string());
                        }
                        if let Some(ref lv) = cs_patch.loader_version {
                            instance_json.loader_version = lv.clone();
                        }
                    }
                    instance_json.write_to_dir(&dir)?;
                    saved = true;
                }

                // Fall back to celestial.json for .minecraft instances without instance.json
                if !saved && library_format == crate::state::libraries::InstanceFormat::Minecraft {
                    let mut celestial =
                        crate::state::libraries::CelestialJson::read_from_dir(&dir)?
                            .unwrap_or_default();

                    if let Some(name) = &patch.name {
                        celestial.name = Some(name.clone());
                    }
                    if let Some(icon_path) = &patch.icon_path {
                        celestial.icon_path = icon_path.clone();
                    }
                    if let Some(last_played) = &patch.last_played {
                        celestial.last_played = *last_played;
                    }
                    if let Some(stpt) = patch.submitted_time_played {
                        celestial.submitted_time_played = stpt;
                    }
                    if let Some(rtp) = patch.recent_time_played {
                        celestial.recent_time_played = rtp;
                    }
                    if let Some(link) = &patch.link {
                        celestial.link = Some(link.clone());
                    }
                    if let Some(launch_patch) = &patch.launch_overrides {
                        let overrides =
                            celestial.launch_overrides.get_or_insert_with(|| {
                                crate::state::InstanceLaunchOverridesData::new(
                                    instance_id.to_string(),
                                )
                            });
                        if let Some(java_path) = &launch_patch.java_path {
                            overrides.java_path = java_path.clone();
                        }
                        if let Some(extra_launch_args) = &launch_patch.extra_launch_args {
                            overrides.extra_launch_args = extra_launch_args.clone();
                        }
                        if let Some(custom_env_vars) = &launch_patch.custom_env_vars {
                            overrides.custom_env_vars = custom_env_vars.clone();
                        }
                        if let Some(memory) = &launch_patch.memory {
                            overrides.memory = *memory;
                        }
                        if let Some(force_fullscreen) = &launch_patch.force_fullscreen {
                            overrides.force_fullscreen = *force_fullscreen;
                        }
                        if let Some(game_resolution) = &launch_patch.game_resolution {
                            overrides.game_resolution = *game_resolution;
                        }
                        if launch_patch.hooks.is_some() {
                            overrides.hooks = launch_patch.hooks.clone().unwrap_or_default();
                        }
                    }
                    if let Some(update_channel) = patch.update_channel {
                        celestial.update_channel = update_channel;
                    }
                    if let Some(group_ids) = &patch.group_ids {
                        celestial.groups = group_ids.clone();
                    }
                    celestial.write_to_dir(&dir)?;
                    saved = true;
                }

                if saved {
                    found = true;
                    break;
                }
            }
        }
        if !found {
            return Err(
                crate::ErrorKind::InputError("Unknown instance".to_string())
                    .into(),
            );
        }

        // Re-scan to pick up any changes
        let inst = json_instances
            .into_iter()
            .find(|i| i.id == instance_id)
            .ok_or_else(|| -> crate::Error {
                crate::ErrorKind::InputError("Unknown instance".to_string()).into()
            })?;
        emit_instance(&inst.id, InstancePayloadType::Edited).await?;
        return Ok(crate::api::instance::get::instance_metadata_from_instance(
            &inst,
        ));
    }

    // DB-backed instances (fallback for legacy + migrated)
    crate::state::edit_instance(instance_id, patch, &state.pool).await?;
    let meta = crate::state::get_instance(instance_id, &state.pool).await?
        .ok_or_else(|| {
            crate::Error::from(crate::ErrorKind::InputError(
                "Unknown instance".to_string(),
            ))
        })?;
    emit_instance(&meta.instance.id, InstancePayloadType::Edited).await?;
    Ok(meta)
}

#[tracing::instrument]
pub async fn remove(instance_id: &str) -> crate::Result<()> {
    let state = State::get().await?;
    let _instance =
        instance_rows::get_instance_display_info(instance_id, &state.pool)
            .await?;
    crate::install::runner::cancel_jobs_for_instance_deletion(
        instance_id,
        &state,
    )
    .await?;
    crate::state::remove_instance(instance_id, &state).await?;

    emit_instance(instance_id, InstancePayloadType::Removed).await?;

    Ok(())
}

/// Rename a `.minecraft`-format instance directory and its version files.
/// Returns the new instance metadata.
#[tracing::instrument]
pub async fn rename(
    instance_id: &str,
    new_name: String,
) -> crate::Result<InstanceMetadata> {
    tracing::info!("rename: starting for instance_id={} new_name={}", instance_id, new_name);
    let state = State::get().await?;

    let json_instances =
        crate::state::libraries::list_instances_from_json(&state).await?;
    let inst = match json_instances.iter().find(|i| i.id == instance_id) {
        Some(i) => {
            tracing::info!("rename: found instance: path={} format={:?}", i.path, i.library_format);
            i.clone()
        },
        None => {
            tracing::error!("rename: instance not found by id={}", instance_id);
            return Err(
                crate::ErrorKind::InputError("Unknown instance".to_string())
                    .into(),
            );
        }
    };

    let dir =
        crate::state::libraries::resolve_instance_dir(&state, &inst.path);
    tracing::info!("rename: resolved dir={:?}", dir);

    if inst.library_format != crate::state::libraries::InstanceFormat::Minecraft {
        return Err(
            crate::ErrorKind::InputError(
                "Rename is only supported for .minecraft format instances".to_string(),
            )
            .into(),
        );
    }

    tracing::info!("rename: calling rename_minecraft_instance");
    let new_dir =
        crate::state::libraries::rename_minecraft_instance(&dir, &new_name)?;
    tracing::info!("rename: rename_minecraft_instance returned {:?}", new_dir);

    // Update the celestial.json (or create one if missing) name field
    tracing::info!("rename: reading celestial.json from {:?}", new_dir);
    let celestial =
        crate::state::libraries::CelestialJson::read_from_dir(&new_dir)?
            .unwrap_or_default();
    let mut updated_celestial = celestial.clone();
    updated_celestial.name = Some(new_name.clone());
    tracing::info!("rename: writing updated celestial.json");
    updated_celestial.write_to_dir(&new_dir)?;
    tracing::info!("rename: celestial.json written successfully");

    // Emit event so the frontend knows the instance moved
    tracing::info!("rename: emitting Edited event");
    emit_instance(instance_id, InstancePayloadType::Edited).await?;
    tracing::info!("rename: event emitted");

    // Return fresh metadata — the scan will pick up the new path from
    // the sidecar name, and the path itself must be refreshed. We rebuild
    // from the new directory name directly.
    let id =
        crate::state::libraries::instance_id_from_path(
            new_dir.to_string_lossy().as_ref(),
        );
    Ok(crate::api::instance::get::instance_metadata_from_instance(
        &Instance {
            id,
            path: new_dir.to_string_lossy().to_string(),
            applied_content_set_id: None,
            install_stage: crate::state::InstanceInstallStage::Installed,
            launcher_feature_version:
                crate::state::LauncherFeatureVersion::MOST_RECENT,
            update_channel: updated_celestial.update_channel,
            name: new_name,
            icon_path: updated_celestial.icon_path.clone(),
            created: inst.created,
            modified: chrono::Utc::now(),
            last_played: updated_celestial.last_played,
            submitted_time_played: updated_celestial.submitted_time_played,
            recent_time_played: updated_celestial.recent_time_played,
            library_format: crate::state::libraries::InstanceFormat::Minecraft,
        },
    ))
}
