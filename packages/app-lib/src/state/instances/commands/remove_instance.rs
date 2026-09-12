use crate::state::State;
use crate::state::instances::adapters::sqlite::instance_rows;
use crate::state::libraries;

pub(crate) async fn remove_instance(
    instance_id: &str,
    state: &State,
) -> crate::Result<()> {
    let instance =
        instance_rows::get_instance_by_id(instance_id, &state.pool).await?;

    // Held across the whole teardown so a concurrent sync cannot recreate files
    // underneath us. Acquired exactly once — these are real mutexes and a second
    // `lock_instance_content` for the same instance would deadlock.
    let _synced_options_lock = state.lock_synced_options().await;
    let _content_lock = state.lock_instance_content(instance_id).await;
    crate::api::instance::remove_generated_instance_files(instance_id, state)
        .await?;

    match instance {
        Some(instance) => {
            delete_instance_row_and_locks(&instance.id, state).await?;
            // Resolved through the library registry, not `instances_dir()`: a
            // JSON-backed instance can live anywhere the user registered.
            let path = libraries::resolve_instance_dir(state, &instance.path);
            if path.exists() {
                crate::util::io::remove_dir_all(&path).await?;
            }
        }
        None => {
            // JSON-backed instance — find it from libraries
            let json_instances =
                libraries::list_instances_from_json(state).await?;
            if let Some(instance) = json_instances.iter().find(|i| i.id == instance_id)
            {
                let path =
                    libraries::resolve_instance_dir(state, &instance.path);
                if path.exists() {
                    crate::util::io::remove_dir_all(&path).await?;
                }
                let json_path = path.join("instance.json");
                if json_path.exists() {
                    std::fs::remove_file(&json_path)?;
                }
            } else {
                return Err(crate::ErrorKind::InputError(
                    "Unknown instance".to_string(),
                )
                .into());
            }
        }
    }

    // The directory is gone; a cached listing taken before this point would
    // still show it.
    libraries::invalidate_instance_list_cache();

    Ok(())
}

async fn delete_instance_row_and_locks(
    instance_id: &str,
    state: &State,
) -> crate::Result<()> {
    // Keep these together so deleted instances cannot leave stale entries in the per-instance lock maps.
    instance_rows::delete_instance_by_id(instance_id, &state.pool).await?;
    state.remove_instance_locks(instance_id);

    Ok(())
}
