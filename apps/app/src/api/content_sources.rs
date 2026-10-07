//! Content-source plugin commands.
//!
//! Like the plugin commands, this layer only forwards to
//! `theseus::content_sources` and converts paths for the frontend; every rule
//! about what a source may declare or do lives in the app-lib module, so the
//! Tauri layer stays a thin shell.
//!
//! Commands carry the `content_source_` prefix because `#[tauri::command]`
//! generates a helper macro named after the function at the crate root, so a
//! bare `get` here would collide with the one `mr_auth` already defines.

use crate::api::Result;
use std::path::PathBuf;
use tauri::plugin::TauriPlugin;
use theseus::content_sources::{
    self, ContentType, InstallDescriptor, InstallMetadata, InstalledContent,
    SourceFetchRequest, SourceFetchResponse, SourceSummary,
};
pub fn init<R: tauri::Runtime>() -> TauriPlugin<R> {
    tauri::plugin::Builder::new("content-sources")
        .invoke_handler(tauri::generate_handler![
            content_source_list,
            content_source_get,
            content_source_install,
            content_source_install_from_url,
            content_source_update_from_url,
            content_source_latest_release,
            content_source_uninstall,
            content_source_set_enabled,
            content_source_open_folder,
            content_source_storage_get,
            content_source_storage_set,
            content_source_storage_remove,
            content_source_storage_keys,
            content_source_settings_get,
            content_source_settings_set,
            content_source_crash_log,
            content_source_report_crash,
            content_source_read_entry,
            content_source_fetch,
            content_source_install_content,
            content_source_list_installed,
            content_source_forget_installed,
            content_source_list_downloaded_skins,
            content_source_stage_modpack,
        ])
        .build()
}

#[tauri::command]
pub async fn content_source_list() -> Result<Vec<SourceSummary>> {
    Ok(content_sources::list().await?)
}

#[tauri::command]
pub async fn content_source_get(
    source_id: String,
) -> Result<Option<SourceSummary>> {
    Ok(content_sources::get(&source_id).await?)
}

#[tauri::command]
pub async fn content_source_install(
    path: String,
    origin: Option<String>,
) -> Result<SourceSummary> {
    Ok(content_sources::install(&PathBuf::from(path), origin).await?)
}

#[tauri::command]
pub async fn content_source_install_from_url(
    url: String,
    origin: Option<String>,
) -> Result<SourceSummary> {
    Ok(content_sources::install_from_url(&url, origin).await?)
}

#[tauri::command]
pub async fn content_source_update_from_url(
    url: String,
    origin: Option<String>,
) -> Result<SourceSummary> {
    Ok(content_sources::update_from_url(&url, origin).await?)
}

/// The tag of a source's latest GitHub release, for update detection.
#[tauri::command]
pub async fn content_source_latest_release(
    repo: String,
) -> Result<Option<String>> {
    Ok(content_sources::latest_release_tag(&repo).await?)
}

#[tauri::command]
pub async fn content_source_uninstall(
    source_id: String,
    remove_data: Option<bool>,
) -> Result<()> {
    content_sources::uninstall(&source_id, remove_data.unwrap_or(false))
        .await?;
    Ok(())
}

#[tauri::command]
pub async fn content_source_set_enabled(
    source_id: String,
    enabled: bool,
) -> Result<SourceSummary> {
    Ok(content_sources::set_enabled(&source_id, enabled).await?)
}

/// Reveal the content-sources folder, or one source's own folder, in the file
/// manager.
///
/// The path comes from the source summary rather than being built from the
/// argument, so an id from the frontend cannot name anything outside the
/// content-sources folder.
#[tauri::command]
pub async fn content_source_open_folder<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    source_id: Option<String>,
) -> Result<()> {
    let path = match source_id {
        Some(source_id) => {
            let unknown: theseus::Error = theseus::ErrorKind::InputError(
                format!("Unknown content source '{source_id}'"),
            )
            .into();
            let summary =
                content_sources::get(&source_id).await?.ok_or(unknown)?;
            PathBuf::from(summary.path)
        }
        None => {
            let state = theseus::State::get().await?;
            content_sources::content_sources_dir(&state)
        }
    };

    tokio::fs::create_dir_all(&path).await?;
    crate::api::utils::open_path(app, path).await;
    Ok(())
}

#[tauri::command]
pub async fn content_source_storage_get(
    source_id: String,
    key: String,
) -> Result<Option<String>> {
    Ok(content_sources::storage_get(&source_id, &key).await?)
}

#[tauri::command]
pub async fn content_source_storage_set(
    source_id: String,
    key: String,
    value: String,
) -> Result<()> {
    content_sources::storage_set(&source_id, &key, value).await?;
    Ok(())
}

#[tauri::command]
pub async fn content_source_storage_remove(
    source_id: String,
    key: String,
) -> Result<()> {
    content_sources::storage_remove(&source_id, &key).await?;
    Ok(())
}

#[tauri::command]
pub async fn content_source_storage_keys(
    source_id: String,
) -> Result<Vec<String>> {
    Ok(content_sources::storage_keys(&source_id).await?)
}

#[tauri::command]
pub async fn content_source_settings_get(
    source_id: String,
) -> Result<std::collections::HashMap<String, String>> {
    Ok(content_sources::settings_get_all(&source_id).await?)
}

#[tauri::command]
pub async fn content_source_settings_set(
    source_id: String,
    key: String,
    value: String,
) -> Result<()> {
    content_sources::settings_set(&source_id, &key, value).await?;
    Ok(())
}

#[tauri::command]
pub async fn content_source_crash_log(
    source_id: String,
) -> Result<Option<String>> {
    Ok(content_sources::crash_log(&source_id).await?)
}

/// Called by the loader when a source throws. Records the detail and disables
/// the source so it cannot throw again on the next browse.
#[tauri::command]
pub async fn content_source_report_crash(
    source_id: String,
    message: String,
    stack: Option<String>,
) -> Result<()> {
    content_sources::record_crash(&source_id, &message, stack.as_deref())
        .await?;
    Ok(())
}

/// Fallback source for the loader: the asset protocol can refuse a cross-origin
/// module load, in which case the entry is imported from a blob URL built out
/// of this text.
#[tauri::command]
pub async fn content_source_read_entry(
    source_id: String,
) -> Result<Option<String>> {
    Ok(content_sources::read_entry(&source_id).await?)
}

/// Network request proxied through Rust; the host is checked there against the
/// hosts the source declared in its manifest.
#[tauri::command]
pub async fn content_source_fetch(
    source_id: String,
    request: SourceFetchRequest,
) -> Result<SourceFetchResponse> {
    Ok(content_sources::fetch(&source_id, request).await?)
}

/// Install one file a content source produced.
///
/// The source supplies only a descriptor (URL, headers, name, optional hash);
/// where the file lands is decided here, by content type. `instance_id` is
/// unused for a skin, which belongs to the launcher rather than an instance.
#[tauri::command]
pub async fn content_source_install_content(
    instance_id: Option<String>,
    content_type: ContentType,
    descriptor: InstallDescriptor,
    meta: Option<InstallMetadata>,
) -> Result<InstalledContent> {
    let instance_id = instance_id.unwrap_or_default();
    let state = theseus::State::get().await?;
    Ok(content_sources::install_content(
        &instance_id,
        content_type,
        descriptor,
        meta.unwrap_or_default(),
        &state,
    )
    .await?)
}

/// Everything installed from a content source into an instance.
#[tauri::command]
pub async fn content_source_list_installed(
    instance_id: String,
) -> Result<Vec<content_sources::SourceContentRecord>> {
    let state = theseus::State::get().await?;
    Ok(content_sources::list_installed(&instance_id, &state).await?)
}

/// Forget an installed record, leaving the file on disk alone.
#[tauri::command]
pub async fn content_source_forget_installed(
    instance_id: String,
    record_id: String,
) -> Result<()> {
    let state = theseus::State::get().await?;
    content_sources::forget_install(&instance_id, &record_id, &state).await?;
    Ok(())
}

/// Skins downloaded through content sources, shown on the skins page.
#[tauri::command]
pub async fn content_source_list_downloaded_skins(
) -> Result<Vec<content_sources::DownloadedSkin>> {
    let state = theseus::State::get().await?;
    Ok(content_sources::list_downloaded_skins(&state).await?)
}

/// Download a source's modpack archive and read its manifest, so the frontend
/// can have the source's provider resolve the pack's files before installing.
/// The archive stays on disk under the launcher's metadata directory.
#[tauri::command]
pub async fn content_source_stage_modpack(
    source_id: String,
    descriptor: InstallDescriptor,
) -> Result<content_sources::StagedModpack> {
    let state = theseus::State::get().await?;
    Ok(content_sources::stage_modpack(&source_id, &descriptor, &state).await?)
}
