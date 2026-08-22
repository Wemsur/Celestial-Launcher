use crate::state::{JavaVersion, State};

pub async fn get_optimal_jre_key(
    instance_id: &str,
) -> crate::Result<Option<JavaVersion>> {
    let state = State::get().await?;
    let context =
        crate::state::instances::commands::get_instance_launch_context(
            instance_id,
            &state.pool,
        )
        .await?
        .ok_or_else(|| {
            crate::ErrorKind::OtherError(format!(
                "尝试解析不存在的实例 {instance_id}!"
            ))
        })?;

    // For .minecraft format instances, determine the instance dir so we can
    // write version info and JAR there during download.
    let inst_dir_for_download =
        if context.instance.library_format == crate::state::libraries::InstanceFormat::Minecraft
        {
            Some(crate::state::libraries::resolve_instance_dir_with_dirs(
                &state.directories, &context.instance.path,
            ))
        } else {
            None
        };

    let (minecraft, version_index) =
        crate::launcher::resolve_minecraft_manifest(
            &context.applied_content_set.game_version,
            &state,
        )
        .await?;
    let version = &minecraft.versions[version_index];
    let loader_version = crate::launcher::get_loader_version_from_profile(
        &context.applied_content_set.game_version,
        context.applied_content_set.loader,
        context.applied_content_set.loader_version.as_deref(),
    )
    .await?;
    let version_info = crate::launcher::download::download_version_info(
        &state,
        version,
        loader_version.as_ref(),
        None,
        None,
        None,
        inst_dir_for_download.as_ref(),
        None,
    )
    .await?;

    crate::launcher::get_java_version_from_launch_context(
        &context,
        &version_info,
    )
    .await
}
