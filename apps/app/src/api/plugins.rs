//! Plugin management commands.
//!
//! This layer only forwards to `theseus::plugins` and converts paths for the
//! frontend; every rule about what a plugin may declare or do lives in
//! `theseus::plugins`, so the Tauri layer stays a thin shell.
//!
//! Every command carries the `plugin_` prefix because `#[tauri::command]`
//! generates a helper macro named after the function at the crate root, so a
//! bare `get` here would collide with the one `mr_auth` already defines.

use crate::api::Result;
use std::path::PathBuf;
use tauri::plugin::TauriPlugin;
use theseus::plugins::{self, PluginSummary};

pub fn init<R: tauri::Runtime>() -> TauriPlugin<R> {
    tauri::plugin::Builder::new("plugins")
        .invoke_handler(tauri::generate_handler![
            plugin_list,
            plugin_get,
            plugin_install,
            plugin_uninstall,
            plugin_set_enabled,
            plugin_set_granted,
            plugin_grant_permission,
            plugin_revoke_permission,
            plugin_open_folder,
        ])
        .build()
}

#[tauri::command]
pub async fn plugin_list() -> Result<Vec<PluginSummary>> {
    Ok(plugins::list().await?)
}

#[tauri::command]
pub async fn plugin_get(plugin_id: String) -> Result<Option<PluginSummary>> {
    Ok(plugins::get(&plugin_id).await?)
}

/// Install from a folder that already contains a `manifest.json`.
#[tauri::command]
pub async fn plugin_install(
    path: String,
    origin: Option<String>,
) -> Result<PluginSummary> {
    let source = PathBuf::from(path);
    Ok(plugins::install(&source, origin).await?)
}

#[tauri::command]
pub async fn plugin_uninstall(
    plugin_id: String,
    remove_data: Option<bool>,
) -> Result<()> {
    plugins::uninstall(&plugin_id, remove_data.unwrap_or(false)).await?;
    Ok(())
}

#[tauri::command]
pub async fn plugin_set_enabled(
    plugin_id: String,
    enabled: bool,
) -> Result<PluginSummary> {
    Ok(plugins::set_enabled(&plugin_id, enabled).await?)
}

#[tauri::command]
pub async fn plugin_set_granted(
    plugin_id: String,
    granted: Vec<String>,
) -> Result<PluginSummary> {
    Ok(plugins::set_granted(&plugin_id, granted).await?)
}

#[tauri::command]
pub async fn plugin_grant_permission(
    plugin_id: String,
    permission: String,
) -> Result<PluginSummary> {
    Ok(plugins::grant_permission(&plugin_id, &permission).await?)
}

#[tauri::command]
pub async fn plugin_revoke_permission(
    plugin_id: String,
    permission: String,
) -> Result<PluginSummary> {
    Ok(plugins::revoke_permission(&plugin_id, &permission).await?)
}

/// Reveal the plugins folder, or one plugin's own folder, in the file manager.
///
/// The path comes from the plugin summary rather than being built from the
/// argument, so an id from the frontend cannot name anything outside the
/// plugins folder.
#[tauri::command]
pub async fn plugin_open_folder<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    plugin_id: Option<String>,
) -> Result<()> {
    let path = match plugin_id {
        Some(plugin_id) => {
            let unknown: theseus::Error = theseus::ErrorKind::InputError(
                format!("Unknown plugin '{plugin_id}'"),
            )
            .into();
            let summary = plugins::get(&plugin_id).await?.ok_or(unknown)?;
            PathBuf::from(summary.path)
        }
        None => {
            let state = theseus::State::get().await?;
            plugins::plugins_dir(&state)
        }
    };

    tokio::fs::create_dir_all(&path).await?;
    crate::api::utils::open_path(app, path).await;
    Ok(())
}
