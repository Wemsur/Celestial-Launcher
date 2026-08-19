use crate::state::{libraries, State};
use std::path::PathBuf;

#[tracing::instrument]
pub async fn get_full_path(instance_id: &str) -> crate::Result<PathBuf> {
    let state = State::get().await?;

    // First try JSON-backed instances
    if let Some(dir) =
        libraries::find_json_instance(&state, instance_id).await?
    {
        return Ok(dir);
    }

    // Fall back to DB
    if let Some(instance) =
        crate::state::instances::adapters::sqlite::instance_rows::get_instance_by_id(
            instance_id,
            &state.pool,
        )
        .await?
    {
        Ok(libraries::resolve_instance_dir(&state, &instance.path))
    } else {
        Err(crate::ErrorKind::InputError("Unknown instance".to_string()).into())
    }
}

#[tracing::instrument]
pub async fn get_mod_full_path(
    instance_id: &str,
    project_path: &str,
) -> crate::Result<PathBuf> {
    Ok(get_full_path(instance_id).await?.join(project_path))
}
