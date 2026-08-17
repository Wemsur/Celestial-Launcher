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
        },
        &state,
    )
    .await?;

    let result = async {
        emit_instance(&instance.id, InstancePayloadType::Created).await?;

        crate::state::get_instance(&instance.id, &state.pool)
            .await?
            .ok_or_else(|| {
                crate::ErrorKind::InputError(
                    "Created instance could not be loaded".to_string(),
                )
                .into()
            })
    }
    .await;

    if result.is_err() {
        let _ = crate::state::remove_instance(&instance.id, &state).await;
    }

    result
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
        // JSON-backed instances: only editable fields are name, icon_path, last_played, time played
        if patch.link.is_some()
            || patch.content_set_patch.is_some()
            || patch.groups.is_some()
            || patch.launch_overrides.is_some()
            || patch.install_stage.is_some()
            || patch.launcher_feature_version.is_some()
            || patch.update_channel.is_some()
        {
            return Err(
                crate::ErrorKind::InputError(
                    "Cannot modify linked fields of JSON-backed instances".to_string(),
                )
                .into(),
            );
        }
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
    let instance =
        instance_rows::get_instance_display_info(instance_id, &state.pool)
            .await?;
    crate::state::remove_instance(instance_id, &state).await?;

    if let Some(instance) = instance {
        emit_instance(&instance.id, InstancePayloadType::Removed).await?;
    }

    Ok(())
}
