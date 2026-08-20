use crate::event::InstancePayloadType;
use crate::event::emit::emit_instance;
use crate::state::instances::adapters::sqlite::instance_rows;
use crate::state::{
    CreateInstance, EditInstance, InstanceLink, InstanceMetadata, ModLoader,
    State,
};

#[tracing::instrument]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn create(
    name: String,
    game_version: String,
    modloader: ModLoader,
    loader_version: Option<String>,
    icon_path: Option<String>,
    link: InstanceLink,
    library_path: Option<String>,
) -> crate::Result<InstanceMetadata> {
    let state = State::get().await?;
    let instance = crate::state::create_instance(
        CreateInstance {
            name,
            path: None,
            game_version,
            loader: modloader,
            loader_version,
            icon_path,
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

    // DB-backed instances
    if instance_rows::get_instance_by_id(instance_id, &state.pool).await?.is_some()
    {
        crate::state::edit_instance(instance_id, patch, &state.pool).await?;
    } else {
        // JSON-backed instances
        let mut found = false;
        let json_instances =
            crate::state::libraries::list_instances_from_json(&state)
                .await?;
        for inst in &json_instances {
            if inst.id == instance_id {
                let dir =
                    crate::state::libraries::resolve_instance_dir(
                        &state,
                        &inst.path,
                    );
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
                    // Update launch overrides
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
                    // Update update_channel
                    if let Some(update_channel) = patch.update_channel {
                        instance_json.update_channel = update_channel;
                    }
                    // Update groups
                    if let Some(groups) = &patch.groups {
                        instance_json.groups = groups.clone();
                    }
                    instance_json.write_to_dir(&dir)?;
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
    }

    let instance = if let Some(meta) =
        crate::state::get_instance(instance_id, &state.pool).await?
    {
        meta
    } else {
        // JSON-backed: build metadata from instance
        let json_instances =
            crate::state::libraries::list_instances_from_json(&state).await?;
        let inst = json_instances
            .into_iter()
            .find(|i| i.id == instance_id)
            .ok_or_else(|| -> crate::Error {
                crate::ErrorKind::InputError("Unknown instance".to_string()).into()
            })?;
        crate::api::instance::get::instance_metadata_from_instance(&inst)
    };

    emit_instance(&instance.instance.id, InstancePayloadType::Edited).await?;

    Ok(instance)
}

#[tracing::instrument]
pub async fn remove(instance_id: &str) -> crate::Result<()> {
    let state = State::get().await?;
    let _instance =
        instance_rows::get_instance_display_info(instance_id, &state.pool)
            .await?;
    crate::state::remove_instance(instance_id, &state).await?;

    emit_instance(instance_id, InstancePayloadType::Removed).await?;

    Ok(())
}
