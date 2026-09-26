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
            plugin_install_from_url,
            plugin_update_from_url,
            plugin_uninstall,
            plugin_set_enabled,
            plugin_set_granted,
            plugin_grant_permission,
            plugin_revoke_permission,
            plugin_open_folder,
            plugin_storage_get,
            plugin_storage_set,
            plugin_storage_remove,
            plugin_storage_keys,
            plugin_crash_log,
            plugin_report_crash,
            plugin_read_entry,
            plugin_fetch,
            plugin_settings_get,
            plugin_settings_set,
            plugin_get_hot_reload,
            plugin_set_hot_reload,
            plugin_sidecar_ensure,
            plugin_sidecar_start,
            plugin_sidecar_request,
            plugin_sidecar_stop,
            plugin_sidecar_cleanup,
            plugin_sidecar_status,
            plugin_lan_announce,
            plugin_lan_stop,
            plugin_lan_cleanup,
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

/// Download a zipped plugin from the store and install it.
#[tauri::command]
pub async fn plugin_install_from_url(
    url: String,
    origin: Option<String>,
) -> Result<PluginSummary> {
    Ok(plugins::install_from_url(&url, origin).await?)
}

/// Download a zipped plugin from the store and update an installed copy,
/// keeping its enabled state and granted permissions.
#[tauri::command]
pub async fn plugin_update_from_url(
    url: String,
    origin: Option<String>,
) -> Result<PluginSummary> {
    Ok(plugins::update_from_url(&url, origin).await?)
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

#[tauri::command]
pub async fn plugin_storage_get(
    plugin_id: String,
    key: String,
) -> Result<Option<String>> {
    Ok(plugins::storage_get(&plugin_id, &key).await?)
}

#[tauri::command]
pub async fn plugin_storage_set(
    plugin_id: String,
    key: String,
    value: String,
) -> Result<()> {
    plugins::storage_set(&plugin_id, &key, value).await?;
    Ok(())
}

#[tauri::command]
pub async fn plugin_storage_remove(
    plugin_id: String,
    key: String,
) -> Result<()> {
    plugins::storage_remove(&plugin_id, &key).await?;
    Ok(())
}

#[tauri::command]
pub async fn plugin_storage_keys(plugin_id: String) -> Result<Vec<String>> {
    Ok(plugins::storage_keys(&plugin_id).await?)
}

#[tauri::command]
pub async fn plugin_crash_log(
    plugin_id: String,
) -> Result<Option<String>> {
    Ok(plugins::crash_log(&plugin_id).await?)
}

/// Called by the loader when a plugin throws. Records the detail and disables
/// the plugin so it cannot throw again on the next render.
#[tauri::command]
pub async fn plugin_report_crash(
    plugin_id: String,
    message: String,
    stack: Option<String>,
) -> Result<()> {
    plugins::record_crash(&plugin_id, &message, stack.as_deref()).await?;
    Ok(())
}

/// Fallback source for the loader: the asset protocol can refuse a
/// cross-origin module load, in which case the entry is imported from a blob
/// URL built out of this text.
#[tauri::command]
pub async fn plugin_read_entry(plugin_id: String) -> Result<Option<String>> {
    Ok(plugins::read_entry(&plugin_id).await?)
}

/// Make a network request on a plugin's behalf, gated on its `network:<host>`
/// grant. The webview cannot do this itself — CSP boxes in its `fetch`.
#[tauri::command]
pub async fn plugin_fetch(
    plugin_id: String,
    request: plugins::PluginFetchRequest,
) -> Result<plugins::PluginFetchResponse> {
    Ok(plugins::fetch(&plugin_id, request).await?)
}

/// Read a plugin's declared-settings values (for the plugin page's form).
#[tauri::command]
pub async fn plugin_settings_get(
    plugin_id: String,
) -> Result<std::collections::HashMap<String, String>> {
    Ok(plugins::settings_get_all(&plugin_id).await?)
}

/// Save one declared-settings value from the plugin page (the user's action).
#[tauri::command]
pub async fn plugin_settings_set(
    plugin_id: String,
    key: String,
    value: String,
) -> Result<()> {
    plugins::settings_set(&plugin_id, &key, value).await?;
    Ok(())
}

/// Whether plugin state changes are applied to the running launcher live.
#[tauri::command]
pub async fn plugin_get_hot_reload() -> Result<bool> {
    Ok(plugins::get_hot_reload().await?)
}

/// Turn live application of plugin state changes on or off (the user's action).
#[tauri::command]
pub async fn plugin_set_hot_reload(enabled: bool) -> Result<()> {
    plugins::set_hot_reload(enabled).await?;
    Ok(())
}

/// Download, verify, and unpack a sidecar binary. Gated on `sidecar`.
#[tauri::command]
pub async fn plugin_sidecar_ensure(
    plugin_id: String,
    key: String,
    url: String,
    sha512: Option<String>,
    sha512_url: Option<String>,
    archive: Option<String>,
    version: Option<String>,
) -> Result<()> {
    plugins::sidecar::ensure(
        &plugin_id,
        &key,
        &url,
        sha512.as_deref(),
        sha512_url.as_deref(),
        archive.as_deref().unwrap_or("tar.gz"),
        version.as_deref(),
    )
    .await?;
    Ok(())
}

/// Whether a sidecar is installed for the plugin, and its recorded version.
#[tauri::command]
pub async fn plugin_sidecar_status(
    plugin_id: String,
    key: String,
) -> Result<plugins::SidecarStatus> {
    Ok(plugins::sidecar::status(&plugin_id, &key).await?)
}

/// Spawn an installed sidecar; returns its handle and (if requested) its port.
#[tauri::command]
pub async fn plugin_sidecar_start(
    plugin_id: String,
    key: String,
    args: Vec<String>,
    port_file: Option<bool>,
) -> Result<plugins::SidecarStarted> {
    Ok(plugins::sidecar::start(
        &plugin_id,
        &key,
        args,
        port_file.unwrap_or(false),
    )
    .await?)
}

/// Proxy a control request to a running sidecar over localhost.
#[tauri::command]
pub async fn plugin_sidecar_request(
    plugin_id: String,
    handle: u64,
    request: plugins::SidecarRequest,
) -> Result<plugins::SidecarResponse> {
    Ok(plugins::sidecar::request(&plugin_id, handle, request).await?)
}

#[tauri::command]
pub async fn plugin_sidecar_stop(
    plugin_id: String,
    handle: u64,
) -> Result<()> {
    plugins::sidecar::stop(&plugin_id, handle).await?;
    Ok(())
}

/// Kill every sidecar a plugin started (called when it unloads).
#[tauri::command]
pub async fn plugin_sidecar_cleanup(plugin_id: String) -> Result<()> {
    plugins::sidecar::cleanup(&plugin_id).await;
    Ok(())
}

/// Start announcing a Minecraft LAN world. Gated on `lan`.
#[tauri::command]
pub async fn plugin_lan_announce(
    plugin_id: String,
    motd: String,
    port: u16,
) -> Result<plugins::LanAnnounce> {
    Ok(plugins::lan::announce(&plugin_id, &motd, port).await?)
}

#[tauri::command]
pub async fn plugin_lan_stop(plugin_id: String, handle: u64) -> Result<()> {
    plugins::lan::stop(&plugin_id, handle).await?;
    Ok(())
}

/// Stop every LAN announcer a plugin started (called when it unloads).
#[tauri::command]
pub async fn plugin_lan_cleanup(plugin_id: String) -> Result<()> {
    plugins::lan::cleanup(&plugin_id).await;
    Ok(())
}
