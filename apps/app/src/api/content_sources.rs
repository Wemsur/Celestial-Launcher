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
use tauri::Manager as _;
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
            content_source_install_content_from_path,
            content_source_open_download_window,
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

/// Install a file the launcher already downloaded to its staging directory.
///
/// The path is checked in `theseus` against the staging directory before
/// anything is read, so the frontend cannot name an arbitrary file.
#[tauri::command]
pub async fn content_source_install_content_from_path(
    instance_id: Option<String>,
    content_type: ContentType,
    staged_path: String,
    meta: Option<InstallMetadata>,
) -> Result<InstalledContent> {
    let instance_id = instance_id.unwrap_or_default();
    let state = theseus::State::get().await?;
    Ok(content_sources::install_content_from_path(
        &instance_id,
        content_type,
        &PathBuf::from(staged_path),
        meta.unwrap_or_default(),
        &state,
    )
    .await?)
}

/// Open a content source's download page in a window, let the user fetch the
/// file themselves, and hand back the path it landed at.
///
/// Some sites gate their downloads behind a captcha, so the launcher cannot
/// fetch the file on the user's behalf. It opens the site's own page in a
/// dedicated window instead — the user solves the captcha and clicks download
/// there — and redirects the resulting download into its own staging directory,
/// which is the only place `install_content_from_path` will read from.
///
/// The URL's host is checked against the source's declared `hosts` before the
/// window opens: a `WebviewWindow` is otherwise outside the network allow-list
/// that every other source request goes through, so this is what keeps a source
/// from aiming a launcher window at an arbitrary host. `None` means the user
/// closed the window without downloading.
#[tauri::command]
pub async fn content_source_open_download_window<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    source_id: String,
    url: String,
    file_name: Option<String>,
) -> Result<Option<String>> {
    let parsed = url::Url::parse(&url).map_err(|error| {
        theseus::Error::from(theseus::ErrorKind::InputError(format!(
            "Invalid download URL: {error}"
        )))
    })?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(theseus::Error::from(theseus::ErrorKind::InputError(
            "Download pages must be http or https".to_string(),
        ))
        .into());
    }
    let host = parsed.host_str().ok_or_else(|| {
        theseus::Error::from(theseus::ErrorKind::InputError(
            "The download URL has no host".to_string(),
        ))
    })?;
    theseus::content_sources::ensure_host_allowed(&source_id, host).await?;

    let state = theseus::State::get().await?;
    // A fresh folder per download, so the site's own file name can survive and
    // the folder is what the install removes afterwards. `prepare` also sweeps
    // folders left by earlier windows the user closed without finishing.
    let folder = content_sources::prepare_staged_download(&state).await?;

    Ok(open_download_window(app, parsed, folder, file_name).await?)
}

/// Build the download window, wire its download handler, and wait for it.
///
/// The handler is what makes the file land in the staging folder instead of the
/// user's Downloads folder with a save prompt: on `Requested` it moves the
/// download into that folder — keeping the site's own file name when it is a
/// safe one, falling back to the suggested name otherwise — and lets the
/// download proceed. The page has done its job at that point, so the window is
/// hidden there and then; the download keeps arriving on the webview behind it,
/// and the window is destroyed once the file has landed.
async fn open_download_window<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    url: url::Url,
    folder: std::path::PathBuf,
    suggested_name: Option<String>,
) -> Result<Option<String>> {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tauri::UserAttentionType;
    use tauri::webview::DownloadEvent;

    const LABEL: &str = "source-download";

    if let Some(window) = app.get_webview_window(LABEL) {
        window.close()?;
    }

    let folder_for_handler = folder.clone();
    let fallback = suggested_name
        .as_deref()
        .map(|name| {
            theseus::content_sources::staged_download_target(&folder_for_handler, Some(name))
        })
        .unwrap_or_else(|| folder_for_handler.join("download"));

    // Set when a download starts, completes, or is reported failed. The handler
    // runs on the webview's own thread, so atomics are the hand-off back to this
    // poll loop — which is where the window is actually hidden and closed, so the
    // handler stays free of side effects.
    let started = Arc::new(AtomicBool::new(false));
    let started_for_handler = started.clone();
    let finished = Arc::new(AtomicBool::new(false));
    let finished_for_handler = finished.clone();
    let failed = Arc::new(AtomicBool::new(false));
    let failed_for_handler = failed.clone();

    tracing::info!(url = %url, folder = %folder.display(), "opening content-source download window");

    let window = tauri::WebviewWindowBuilder::new(
        &app,
        LABEL,
        tauri::WebviewUrl::External(url),
    )
    .title("下载")
    .always_on_top(true)
    .min_inner_size(500.0, 500.0)
    .inner_size(1000.0, 700.0)
    .focused(true)
    .center()
    .on_download(move |_webview, event| {
        match event {
            DownloadEvent::Requested { url, destination } => {
                // Keep the site's own file name when it is safe, so the installed
                // mod is named as the site named it; otherwise use the suggested
                // name. Either way the file lands in the folder we created — the
                // webview must not choose a path of its own.
                let safe_name = destination
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .filter(|name| theseus::content_sources::is_safe_staged_file_name(name));
                *destination = match safe_name {
                    Some(name) => folder_for_handler.join(name),
                    None => fallback.clone(),
                };
                tracing::info!(url = %url, destination = %destination.display(), "download requested");
                started_for_handler.store(true, Ordering::SeqCst);
            }
            DownloadEvent::Finished { url, path, success } => {
                // The path is empty on macOS even on success, so the flag — not
                // the path — is what says the bytes arrived; the folder is read
                // afterwards for the file itself.
                tracing::info!(url = %url, path = ?path, success, "download finished");
                if success {
                    finished_for_handler.store(true, Ordering::SeqCst);
                } else {
                    failed_for_handler.store(true, Ordering::SeqCst);
                }
            }
            _ => {}
        }
        true
    })
    .build()?;

    window.request_user_attention(Some(UserAttentionType::Critical))?;

    // Wait for the download to start, then finish or fail, or the hour to run
    // out. The window is hidden the moment the download begins — the page has
    // done its job — and closed once the file has landed. It is *hidden*, not
    // closed, while the download runs: destroying the webview would cancel it.
    //
    // Completion is watched two ways, because the finish event alone is not
    // dependable: the webview reports it, *and* the staged file is watched for a
    // size that has stopped growing. The size fallback is used only while the
    // event has said nothing at all, so a reported failure is never mistaken for
    // a settled partial file.
    let start = std::time::Instant::now();
    let mut hidden = false;
    let mut stable_size: Option<u64> = None;
    let mut stable_polls = 0u32;
    // Set when the size fallback decides the download is done — a second way to
    // reach "completed", for when the finish event never arrives.
    let mut settled = false;
    loop {
        if !hidden && started.load(Ordering::SeqCst) {
            tracing::info!("download started; hiding window");
            let _ = window.hide();
            hidden = true;
        }
        if finished.load(Ordering::SeqCst) {
            tracing::info!("download reported finished");
            break;
        }
        if failed.load(Ordering::SeqCst) {
            tracing::warn!("download reported failed");
            break;
        }
        if window.title().is_err() {
            // Only reachable before the window was hidden — a download that never
            // started, on a page the user then closed.
            tracing::info!("download window closed by the user");
            break;
        }
        // The size fallback only applies once the download is under way; before
        // that the folder is empty and there is nothing to settle.
        if hidden {
            match theseus::content_sources::staged_download_size(&folder).await {
                Some(size) if Some(size) == stable_size && size > 0 => {
                    stable_polls += 1;
                    if stable_polls >= 5 {
                        tracing::info!(size, "staged download settled");
                        settled = true;
                        break;
                    }
                }
                other => {
                    stable_size = other;
                    stable_polls = 0;
                }
            }
        }
        if start.elapsed() >= std::time::Duration::from_secs(60 * 60) {
            tracing::warn!("download window timed out");
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    let _ = window.close();

    // No completed download — the user closed the window without finishing one,
    // or the download failed. The empty folder is dropped, and this reports no
    // download rather than an error, so cancelling is a normal outcome rather
    // than a failure.
    if !finished.load(Ordering::SeqCst) && !settled {
        let _ = tokio::fs::remove_dir_all(&folder).await;
        return Ok(None);
    }

    // The completion event can land a moment before the bytes are flushed to
    // disk, so the folder is given a short grace period to show the file.
    let mut captured = None;
    for _ in 0..30 {
        if let Some(path) = theseus::content_sources::take_staged_download(&folder).await? {
            captured = Some(path);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    match captured {
        Some(path) => {
            tracing::info!(path = %path.display(), "captured staged download");
            Ok(Some(path.to_string_lossy().into_owned()))
        }
        None => {
            tracing::warn!(
                folder = %folder.display(),
                "download reported finished but no file was staged"
            );
            let _ = tokio::fs::remove_dir_all(&folder).await;
            Ok(None)
        }
    }
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
