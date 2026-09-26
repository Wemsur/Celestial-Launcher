//! Where plugins live, what state they carry, and the operations that change
//! that state: scan, install, uninstall, enable, and grant permissions.
//!
//! Layout, under the launcher's settings directory:
//!
//! ```text
//! settings_dir/
//!   plugins.json          ← registry: enabled flag + granted permissions
//!   plugins/<id>/         ← the plugin itself, folder name == manifest id
//!   plugin-data/<id>/     ← writable scratch space handed to the plugin
//! ```
//!
//! The registry is a JSON file rather than a database table on purpose: plugin
//! state is launcher-local bookkeeping, and adding a table would mean a schema
//! migration for something that has no referential integrity to enforce.

use super::manifest::{PluginManifest, PluginType, is_valid_plugin_id};
use super::permissions::{PermissionRisk, PluginPermission};
use crate::State;
use crate::util::io;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const PLUGINS_DIR: &str = "plugins";
const PLUGIN_DATA_DIR: &str = "plugin-data";
const STATE_FILE: &str = "plugins.json";

/// An entry bundle is a compiled plugin, so it is small by nature. The cap only
/// exists so a hostile folder cannot make the loader read a huge file into
/// memory before it ever gets a chance to fail validation.
const MAX_ENTRY_BYTES: u64 = 8 * 1024 * 1024;

pub fn plugins_dir(state: &State) -> PathBuf {
    state.directories.settings_dir.join(PLUGINS_DIR)
}

/// The scratch space handed to a plugin. Validated, because the id comes from
/// the frontend and would otherwise be a path-traversal primitive.
pub fn plugin_data_dir(
    state: &State,
    plugin_id: &str,
) -> crate::Result<PathBuf> {
    Ok(state
        .directories
        .settings_dir
        .join(PLUGIN_DATA_DIR)
        .join(checked_id(plugin_id)?))
}

fn state_file(state: &State) -> PathBuf {
    state.directories.settings_dir.join(STATE_FILE)
}

/// Reject an id that could name anything other than a direct child of the
/// plugins directory.
fn checked_id(plugin_id: &str) -> crate::Result<&str> {
    if !is_valid_plugin_id(plugin_id) {
        return Err(crate::ErrorKind::InputError(format!(
            "Invalid plugin id '{plugin_id}'"
        ))
        .into());
    }
    Ok(plugin_id)
}

fn plugin_dir(state: &State, plugin_id: &str) -> crate::Result<PathBuf> {
    Ok(plugins_dir(state).join(checked_id(plugin_id)?))
}

/// What the launcher remembers about a plugin between runs.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PluginRecord {
    pub id: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub granted: Vec<String>,
    #[serde(default)]
    pub installed_at: Option<i64>,
    /// Where the plugin came from, when it was installed by the launcher.
    #[serde(default)]
    pub source: Option<String>,
    /// Route paths of the plugin's sidebar pages the user pinned to the left
    /// nav rail. Empty means every page is collapsed into the apps drawer.
    #[serde(default)]
    pub pinned_pages: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PluginStateFile {
    #[serde(default)]
    plugins: Vec<PluginRecord>,
    /// Whether enable/disable and permission changes are applied to the running
    /// launcher without a restart. Defaults to on so existing installs keep the
    /// behaviour they had before the toggle existed.
    #[serde(default = "default_hot_reload")]
    hot_reload: bool,
}

impl Default for PluginStateFile {
    fn default() -> Self {
        Self {
            plugins: Vec::new(),
            hot_reload: default_hot_reload(),
        }
    }
}

fn default_hot_reload() -> bool {
    true
}

/// A plugin as the plugin page sees it: the manifest, plus what the launcher
/// currently allows it to do.
///
/// `manifest` is `None` for a plugin whose folder exists but whose manifest
/// could not be read; `error` then says why. Those still have to be listed —
/// otherwise a broken plugin would be invisible and impossible to remove from
/// the UI.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginSummary {
    /// The manifest id, or the folder name when the manifest is unusable.
    pub id: String,
    pub dir_name: String,
    pub path: String,
    pub enabled: bool,
    pub granted: Vec<String>,
    /// Declared but not granted. For a low-risk permission this only happens
    /// after the user revoked it; for a high-risk one it means "waiting for the
    /// user to say yes".
    pub pending: Vec<String>,
    pub high_risk_pending: Vec<String>,
    /// Every declared permission that needs the user's explicit approval.
    /// The plugin page marks these, so the classification lives here rather
    /// than being duplicated in the frontend where it could drift.
    pub high_risk: Vec<String>,
    pub manifest: Option<PluginManifest>,
    pub error: Option<String>,
    /// Route paths of this plugin's sidebar pages currently pinned to the nav
    /// rail. Everything else declared as a sidebar page lives in the drawer.
    #[serde(default)]
    pub pinned_pages: Vec<String>,
}

impl PluginSummary {
    /// The declared plugin type, when the manifest is readable.
    pub fn plugin_type(&self) -> Option<PluginType> {
        self.manifest.as_ref().map(|manifest| manifest.r#type)
    }

    /// Whether this plugin may be loaded at all.
    ///
    /// A plugin with unapproved high-risk permissions still loads: the ones the
    /// user has not approved simply do not work, which is easier to explain
    /// than a plugin that refuses to start.
    pub fn loadable(&self) -> bool {
        self.manifest.is_some() && self.enabled
    }
}

pub async fn list() -> crate::Result<Vec<PluginSummary>> {
    let state = State::get().await?;
    let root = plugins_dir(&state);
    if !root.exists() {
        return Ok(Vec::new());
    }

    let registry = read_state(&state).await?;
    let mut plugins = Vec::new();
    let mut entries = io::read_dir(&root).await?;
    while let Some(entry) = entries.next_entry().await? {
        if !entry.file_type().await?.is_dir() {
            continue;
        }
        let dir_name = entry.file_name().to_string_lossy().to_string();
        // Dot-folders are scratch space, not plugins.
        if dir_name.starts_with('.') {
            continue;
        }
        plugins.push(summarize(
            &entry.path(),
            dir_name,
            registry.plugins.as_slice(),
        ));
    }
    plugins.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(plugins)
}

pub async fn get(plugin_id: &str) -> crate::Result<Option<PluginSummary>> {
    let state = State::get().await?;
    let dir = plugin_dir(&state, plugin_id)?;
    if !dir.is_dir() {
        return Ok(None);
    }
    let registry = read_state(&state).await?;
    Ok(Some(summarize(
        &dir,
        plugin_id.to_string(),
        registry.plugins.as_slice(),
    )))
}

/// Copy a plugin folder into the plugins directory.
///
/// `source` must be a directory holding a valid `manifest.json`; the folder is
/// placed under the manifest's id, which is also the invariant `list` relies on
/// to find it again.
pub async fn install(
    source: &Path,
    origin: Option<String>,
) -> crate::Result<PluginSummary> {
    let state = State::get().await?;
    let manifest = PluginManifest::read_from_dir(source)?;
    let destination = plugin_dir(&state, &manifest.id)?;

    if destination.exists() {
        return Err(crate::ErrorKind::InputError(format!(
            "Plugin '{}' is already installed",
            manifest.id
        ))
        .into());
    }

    io::create_dir_all(&destination).await?;
    if let Err(error) = copy_dir(source, &destination).await {
        // A half-copied plugin folder would be picked up by the next scan as a
        // broken plugin, so clean it up rather than leaving the debris behind.
        let _ = io::remove_dir_all(&destination).await;
        return Err(error);
    }

    let mut registry = read_state(&state).await?;
    registry.plugins.retain(|record| record.id != manifest.id);
    registry.plugins.push(PluginRecord {
        id: manifest.id.clone(),
        enabled: true,
        granted: Vec::new(),
        installed_at: Some(chrono::Utc::now().timestamp()),
        source: origin,
        pinned_pages: Vec::new(),
    });
    write_state(&state, &registry).await?;

    tracing::info!(plugin = %manifest.id, "Installed plugin");

    Ok(summarize(
        &destination,
        manifest.id.clone(),
        registry.plugins.as_slice(),
    ))
}

/// Cap on a downloaded plugin archive. A plugin bundle is small; an unbounded
/// download from a store entry is a way to fill the disk.
const MAX_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;

/// Download a zipped plugin from a store entry, unpack it, and install it.
///
/// The archive is untrusted: entries are checked for path traversal, the total
/// size is capped, and the unpacked folder still goes through the same manifest
/// validation as any other install. `origin` is recorded as the plugin's source.
pub async fn install_from_url(
    url: &str,
    origin: Option<String>,
) -> crate::Result<PluginSummary> {
    let parsed = url::Url::parse(url).map_err(|error| {
        crate::ErrorKind::InputError(format!("Invalid download URL: {error}"))
    })?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(crate::ErrorKind::InputError(
            "Plugin downloads must be http or https".to_string(),
        )
        .into());
    }

    let state = State::get().await?;
    let bytes = crate::util::fetch::fetch(
        url,
        None,
        None,
        None,
        &state.fetch_semaphore,
        &state.pool,
    )
    .await?;

    let staging = state
        .directories
        .settings_dir
        .join(PLUGINS_DIR)
        .join(format!(".staging-{}", uuid::Uuid::new_v4()));
    io::create_dir_all(&staging).await?;

    let result =
        unpack_and_install(&bytes, &staging, origin.or_else(|| Some(url.to_string())))
            .await;
    let _ = io::remove_dir_all(&staging).await;
    result
}

/// Extract a plugin zip into `staging`, then install whichever folder holds the
/// manifest.
async fn unpack_and_install(
    bytes: &[u8],
    staging: &Path,
    origin: Option<String>,
) -> crate::Result<PluginSummary> {
    let bytes = bytes.to_vec();
    let target = staging.to_path_buf();

    // Zip reading is synchronous CPU/IO work, so keep it off the async runtime.
    tokio::task::spawn_blocking(move || extract_zip(&bytes, &target)).await??;

    // The manifest may be at the archive root or one level down (a zip made from
    // a folder). Find it rather than assuming a layout.
    let manifest_dir = find_manifest_dir(staging).await?.ok_or_else(|| {
        crate::ErrorKind::InputError(
            "The downloaded archive has no manifest.json".to_string(),
        )
    })?;

    install(&manifest_dir, origin).await
}

/// Replace an already-installed plugin's files from `source`, keeping the
/// plugin's enabled state and granted permissions.
///
/// Unlike `install`, the plugin is expected to already exist: this is the
/// update path. The registry record is preserved so an update never silently
/// re-prompts for permissions the user already approved.
pub async fn update(
    source: &Path,
    origin: Option<String>,
) -> crate::Result<PluginSummary> {
    let state = State::get().await?;
    let manifest = PluginManifest::read_from_dir(source)?;
    let destination = plugin_dir(&state, &manifest.id)?;

    if !destination.exists() {
        return Err(crate::ErrorKind::InputError(format!(
            "Plugin '{}' is not installed, so there is nothing to update",
            manifest.id
        ))
        .into());
    }

    // Swap the folder contents: the bundle (index.js + manifest.json) lives
    // here, while plugin data and storage sit in separate directories, so
    // replacing this folder wholesale keeps user state intact.
    io::remove_dir_all(&destination).await?;
    io::create_dir_all(&destination).await?;
    if let Err(error) = copy_dir(source, &destination).await {
        let _ = io::remove_dir_all(&destination).await;
        return Err(error);
    }

    let mut registry = read_state(&state).await?;
    if let Some(record) =
        registry.plugins.iter_mut().find(|record| record.id == manifest.id)
    {
        record.installed_at = Some(chrono::Utc::now().timestamp());
        record.source = origin;
    } else {
        registry.plugins.push(PluginRecord {
            id: manifest.id.clone(),
            enabled: true,
            granted: Vec::new(),
            installed_at: Some(chrono::Utc::now().timestamp()),
            source: origin,
            pinned_pages: Vec::new(),
        });
    }
    write_state(&state, &registry).await?;

    tracing::info!(plugin = %manifest.id, "Updated plugin");

    Ok(summarize(
        &destination,
        manifest.id.clone(),
        registry.plugins.as_slice(),
    ))
}

/// Download a zipped plugin from a store entry and update the installed copy.
///
/// Same untrusted-archive handling as `install_from_url`; the difference is the
/// installed plugin's enabled/granted state survives the swap.
pub async fn update_from_url(
    url: &str,
    origin: Option<String>,
) -> crate::Result<PluginSummary> {
    let parsed = url::Url::parse(url).map_err(|error| {
        crate::ErrorKind::InputError(format!("Invalid download URL: {error}"))
    })?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(crate::ErrorKind::InputError(
            "Plugin downloads must be http or https".to_string(),
        )
        .into());
    }

    let state = State::get().await?;
    let bytes = crate::util::fetch::fetch(
        url,
        None,
        None,
        None,
        &state.fetch_semaphore,
        &state.pool,
    )
    .await?;

    let staging = state
        .directories
        .settings_dir
        .join(PLUGINS_DIR)
        .join(format!(".staging-{}", uuid::Uuid::new_v4()));
    io::create_dir_all(&staging).await?;

    let result = unpack_and_update(
        &bytes,
        &staging,
        origin.or_else(|| Some(url.to_string())),
    )
    .await;
    let _ = io::remove_dir_all(&staging).await;
    result
}

/// Extract a plugin zip into `staging`, then update whichever folder holds the
/// manifest.
async fn unpack_and_update(
    bytes: &[u8],
    staging: &Path,
    origin: Option<String>,
) -> crate::Result<PluginSummary> {
    let bytes = bytes.to_vec();
    let target = staging.to_path_buf();

    tokio::task::spawn_blocking(move || extract_zip(&bytes, &target)).await??;

    let manifest_dir = find_manifest_dir(staging).await?.ok_or_else(|| {
        crate::ErrorKind::InputError(
            "The downloaded archive has no manifest.json".to_string(),
        )
    })?;

    update(&manifest_dir, origin).await
}

/// Synchronously extract a zip into `dest`, rejecting any entry that would
/// escape it.
fn extract_zip(bytes: &[u8], dest: &Path) -> crate::Result<()> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|error| {
            crate::ErrorKind::InputError(format!("Invalid plugin archive: {error}"))
        })?;

    let mut total: u64 = 0;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| {
            crate::ErrorKind::InputError(format!("Bad archive entry: {error}"))
        })?;

        // `enclosed_name` returns None for anything with `..` or an absolute
        // path — exactly the entries that would write outside `dest`.
        let Some(relative) = entry.enclosed_name() else {
            return Err(crate::ErrorKind::InputError(format!(
                "Archive entry '{}' has an unsafe path",
                entry.name()
            ))
            .into());
        };
        let out_path = dest.join(&relative);

        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)?;
            continue;
        }

        total = total.saturating_add(entry.size());
        if total > MAX_ARCHIVE_BYTES {
            return Err(crate::ErrorKind::InputError(format!(
                "Plugin archive is larger than {} MiB",
                MAX_ARCHIVE_BYTES / (1024 * 1024)
            ))
            .into());
        }

        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&out_path)?;
        std::io::copy(&mut entry, &mut out)?;
    }

    Ok(())
}

/// Locate the folder containing `manifest.json`: the extraction root, or a
/// single top-level subfolder (a zip built from a plugin directory).
async fn find_manifest_dir(root: &Path) -> crate::Result<Option<PathBuf>> {
    if root.join(super::manifest::MANIFEST_FILE).is_file() {
        return Ok(Some(root.to_path_buf()));
    }

    let mut entries = io::read_dir(root).await?;
    let mut only_dir: Option<PathBuf> = None;
    let mut dir_count = 0;
    while let Some(entry) = entries.next_entry().await? {
        if entry.file_type().await?.is_dir() {
            dir_count += 1;
            only_dir = Some(entry.path());
        }
    }

    if dir_count == 1
        && let Some(dir) = only_dir
        && dir.join(super::manifest::MANIFEST_FILE).is_file()
    {
        return Ok(Some(dir));
    }
    Ok(None)
}

pub async fn uninstall(
    plugin_id: &str,
    remove_data: bool,
) -> crate::Result<()> {
    let state = State::get().await?;
    let dir = plugin_dir(&state, plugin_id)?;
    if dir.is_dir() {
        io::remove_dir_all(&dir).await?;
    }

    if remove_data {
        let data = plugin_data_dir(&state, plugin_id)?;
        if data.is_dir() {
            io::remove_dir_all(&data).await?;
        }
    }

    let mut registry = read_state(&state).await?;
    registry.plugins.retain(|record| record.id != plugin_id);
    write_state(&state, &registry).await?;

    tracing::info!(plugin = %plugin_id, "Uninstalled plugin");
    Ok(())
}

pub async fn set_enabled(
    plugin_id: &str,
    enabled: bool,
) -> crate::Result<PluginSummary> {
    let state = State::get().await?;
    // Resolve the effective state *before* touching the registry. A plugin with
    // no entry yet is running on its low-risk defaults, and writing the entry
    // has to carry those over — otherwise merely disabling and re-enabling a
    // plugin would revoke the permissions it started with, and it would fail on
    // the next launch with permission errors instead of working again.
    let current = require_plugin(plugin_id).await?;

    let mut registry = read_state(&state).await?;
    let had_entry =
        registry.plugins.iter().any(|record| record.id == plugin_id);
    let record = record_mut(&mut registry, plugin_id);
    record.enabled = enabled;
    if !had_entry {
        record.granted = current.granted.clone();
    }
    write_state(&state, &registry).await?;

    require_summary(&state, plugin_id, &registry).await
}

/// Whether live application of enable/disable and permission changes is on.
pub async fn get_hot_reload() -> crate::Result<bool> {
    let state = State::get().await?;
    Ok(read_state(&state).await?.hot_reload)
}

/// Turn live application of plugin state changes on or off.
pub async fn set_hot_reload(enabled: bool) -> crate::Result<()> {
    let state = State::get().await?;
    let mut registry = read_state(&state).await?;
    registry.hot_reload = enabled;
    write_state(&state, &registry).await?;
    Ok(())
}
///
/// Every entry is parsed and checked against what the manifest actually
/// declares, so a bad string from the UI is rejected instead of being stored
/// and silently failing to match later.
pub async fn set_granted(
    plugin_id: &str,
    granted: Vec<String>,
) -> crate::Result<PluginSummary> {
    let state = State::get().await?;
    let mut registry = read_state(&state).await?;

    let plugin = require_summary(&state, plugin_id, &registry).await?;
    let manifest = plugin.manifest.as_ref().ok_or_else(|| {
        crate::ErrorKind::InputError(format!(
            "Plugin '{plugin_id}' has no readable manifest"
        ))
    })?;

    let declared = manifest.parsed_permissions()?;
    let mut canonical = Vec::with_capacity(granted.len());
    for raw in &granted {
        let permission = PluginPermission::parse(raw).map_err(|reason| {
            crate::ErrorKind::InputError(format!(
                "Invalid permission '{raw}': {reason}"
            ))
        })?;
        if !declared.contains(&permission) {
            return Err(crate::ErrorKind::InputError(format!(
                "Plugin '{plugin_id}' does not declare the permission \
                 '{permission}'"
            ))
            .into());
        }
        if !canonical.contains(&permission) {
            canonical.push(permission);
        }
    }
    canonical.sort_by_key(|permission| permission.to_string());

    let record = record_mut(&mut registry, plugin_id);
    record.granted = canonical.iter().map(ToString::to_string).collect();
    write_state(&state, &registry).await?;

    require_summary(&state, plugin_id, &registry).await
}

pub async fn grant_permission(
    plugin_id: &str,
    permission: &str,
) -> crate::Result<PluginSummary> {
    let current = require_plugin(plugin_id).await?;
    let mut granted = current.granted.clone();
    granted.push(permission.to_string());
    set_granted(plugin_id, granted).await
}

pub async fn revoke_permission(
    plugin_id: &str,
    permission: &str,
) -> crate::Result<PluginSummary> {
    let current = require_plugin(plugin_id).await?;
    let granted = current
        .granted
        .into_iter()
        .filter(|granted| granted != permission)
        .collect();
    set_granted(plugin_id, granted).await
}

/// Persist which of a plugin's sidebar pages are pinned to the nav rail.
///
/// The paths are route paths declared by the plugin at runtime, so there is
/// nothing in the manifest to validate them against; the list is just
/// de-duplicated and stored. An unknown path harmlessly matches no page.
pub async fn set_pinned_pages(
    plugin_id: &str,
    pages: Vec<String>,
) -> crate::Result<PluginSummary> {
    let state = State::get().await?;
    let mut registry = read_state(&state).await?;

    let mut canonical: Vec<String> = Vec::with_capacity(pages.len());
    for path in pages {
        if !canonical.contains(&path) {
            canonical.push(path);
        }
    }

    let record = record_mut(&mut registry, plugin_id);
    record.pinned_pages = canonical;
    write_state(&state, &registry).await?;

    require_summary(&state, plugin_id, &registry).await
}

/// Check that `plugin_id` is enabled and has been granted `permission`.
///
/// This is the gate every capability behind an IPC boundary goes through. The
/// frontend side of the host API checks the same list before handing a plugin a
/// capability at all, but that check is a convenience: this one is the one that
/// decides, because nothing on the webview side can reach around it.
pub async fn ensure_granted(
    plugin_id: &str,
    permission: &str,
) -> crate::Result<()> {
    let summary = require_plugin(plugin_id).await?;
    if !summary.enabled {
        return Err(crate::ErrorKind::InputError(format!(
            "Plugin '{plugin_id}' is disabled"
        ))
        .into());
    }

    let wanted = PluginPermission::parse(permission).map_err(|reason| {
        crate::ErrorKind::InputError(format!(
            "Invalid permission '{permission}': {reason}"
        ))
    })?;

    // Granted entries carry their scope (`slot:sidebar.top`), so asking for a
    // bare kind has to match any entry of that kind. An exact string comparison
    // silently denies a plugin that declared its permission correctly.
    let granted = summary.granted.iter().any(|entry| {
        PluginPermission::parse(entry).is_ok_and(|entry| {
            entry.kind == wanted.kind
                && (wanted.scope.is_none() || entry.scope == wanted.scope)
        })
    });

    if !granted {
        return Err(crate::ErrorKind::InputError(format!(
            "Plugin '{plugin_id}' has not been granted '{permission}'"
        ))
        .into());
    }
    Ok(())
}

/// The file a UI plugin's `entry` points at, if there is one.
pub async fn resolve_entry(
    plugin_id: &str,
) -> crate::Result<Option<PathBuf>> {
    let Some(summary) = get(plugin_id).await? else {
        return Ok(None);
    };
    let Some(manifest) = summary.manifest else {
        return Ok(None);
    };
    let Some(entry) = manifest.entry else {
        return Ok(None);
    };
    Ok(Some(Path::new(&summary.path).join(entry)))
}

async fn require_plugin(plugin_id: &str) -> crate::Result<PluginSummary> {
    get(plugin_id).await?.ok_or_else(|| {
        crate::ErrorKind::InputError(format!("Unknown plugin '{plugin_id}'"))
            .into()
    })
}

/// A plugin row for a folder we could not make sense of. It is still listed so
/// the plugin page can show the problem and offer to remove it.
fn broken_summary(dir: &Path, dir_name: &str, reason: String) -> PluginSummary {
    PluginSummary {
        id: dir_name.to_string(),
        dir_name: dir_name.to_string(),
        path: dir.to_string_lossy().to_string(),
        enabled: false,
        granted: Vec::new(),
        pending: Vec::new(),
        high_risk_pending: Vec::new(),
        high_risk: Vec::new(),
        manifest: None,
        error: Some(reason),
        pinned_pages: Vec::new(),
    }
}

fn summarize(
    dir: &Path,
    dir_name: String,
    records: &[PluginRecord],
) -> PluginSummary {
    let manifest = match PluginManifest::read_from_dir(dir) {
        Ok(manifest) => manifest,
        Err(error) => {
            tracing::warn!(
                path = %dir.display(),
                "Plugin folder has no usable manifest: {error}"
            );
            return broken_summary(dir, &dir_name, error.to_string());
        }
    };

    // `list` and `get` locate a plugin by its folder name, so a mismatch would
    // make the plugin unreachable through the API while still being listed.
    if manifest.id != dir_name {
        return broken_summary(
            dir,
            &dir_name,
            format!(
                "Plugin id '{}' does not match its folder name '{dir_name}'",
                manifest.id
            ),
        );
    }

    let declared = match manifest.parsed_permissions() {
        Ok(declared) => declared,
        Err(error) => {
            return broken_summary(dir, &dir_name, error.to_string());
        }
    };

    let record = records.iter().find(|record| record.id == manifest.id);
    let enabled = record.map(|record| record.enabled).unwrap_or(true);
    let pinned_pages =
        record.map(|record| record.pinned_pages.clone()).unwrap_or_default();
    let mut granted = record
        .map(|record| record.granted.clone())
        .unwrap_or_default();

    // Declaring a low-risk permission is what grants it, and that is applied on
    // top of whatever the registry holds rather than only when the plugin is
    // new. Otherwise a plugin update that adds a slot or a style would sit
    // unapproved until the user re-approved everything, and would break on
    // launch in the meantime. Revoking a low-risk permission is not expressible
    // yet — that arrives with the plugin page.
    for permission in &declared {
        if permission.risk() != PermissionRisk::Low {
            continue;
        }
        let entry = permission.to_string();
        if !granted.contains(&entry) {
            granted.push(entry);
        }
    }
    granted.sort();

    let pending = declared
        .iter()
        .map(ToString::to_string)
        .filter(|permission| !granted.contains(permission))
        .collect::<Vec<_>>();
    let high_risk = declared
        .iter()
        .filter(|permission| permission.risk() == PermissionRisk::High)
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let high_risk_pending = high_risk
        .iter()
        .filter(|permission| !granted.contains(permission))
        .cloned()
        .collect();

    PluginSummary {
        id: manifest.id.clone(),
        dir_name,
        path: dir.to_string_lossy().to_string(),
        enabled,
        granted,
        pending,
        high_risk_pending,
        high_risk,
        manifest: Some(manifest),
        error: None,
        pinned_pages,
    }
}

async fn require_summary(
    state: &State,
    plugin_id: &str,
    registry: &PluginStateFile,
) -> crate::Result<PluginSummary> {
    let dir = plugin_dir(state, plugin_id)?;
    if !dir.is_dir() {
        return Err(crate::ErrorKind::InputError(format!(
            "Unknown plugin '{plugin_id}'"
        ))
        .into());
    }
    Ok(summarize(&dir, plugin_id.to_string(), &registry.plugins))
}

/// The registry entry for `plugin_id`, created on first use.
fn record_mut<'a>(
    registry: &'a mut PluginStateFile,
    plugin_id: &str,
) -> &'a mut PluginRecord {
    if let Some(index) = registry
        .plugins
        .iter()
        .position(|record| record.id == plugin_id)
    {
        return &mut registry.plugins[index];
    }
    registry.plugins.push(PluginRecord {
        id: plugin_id.to_string(),
        enabled: true,
        granted: Vec::new(),
        installed_at: Some(chrono::Utc::now().timestamp()),
        source: None,
        pinned_pages: Vec::new(),
    });
    let index = registry.plugins.len() - 1;
    &mut registry.plugins[index]
}

async fn read_state(state: &State) -> crate::Result<PluginStateFile> {
    let path = state_file(state);
    let bytes = match io::read(&path).await {
        Ok(bytes) => bytes,
        Err(_) => return Ok(PluginStateFile::default()),
    };
    match serde_json::from_slice(&bytes) {
        Ok(registry) => Ok(registry),
        Err(error) => {
            // Losing the registry only means permissions get reviewed again,
            // which is a far better outcome than refusing to start.
            tracing::warn!(
                path = %path.display(),
                "Plugin registry is unreadable, starting from an empty one: \
                 {error}"
            );
            Ok(PluginStateFile::default())
        }
    }
}

async fn write_state(
    state: &State,
    registry: &PluginStateFile,
) -> crate::Result<()> {
    let path = state_file(state);
    if let Some(parent) = path.parent() {
        io::create_dir_all(parent).await?;
    }
    io::write(&path, serde_json::to_vec_pretty(registry)?).await?;
    Ok(())
}

async fn copy_dir(from: &Path, to: &Path) -> crate::Result<()> {
    let mut entries = io::read_dir(from).await?;
    while let Some(entry) = entries.next_entry().await? {
        let source = entry.path();
        let target = to.join(entry.file_name());
        let file_type = entry.file_type().await?;
        if file_type.is_dir() {
            io::create_dir_all(&target).await?;
            Box::pin(copy_dir(&source, &target)).await?;
        } else if file_type.is_file() {
            io::copy(&source, &target).await?;
        }
        // Symlinks are skipped: plugin folders are untrusted, and following one
        // could copy files from anywhere on the disk into the plugins folder.
    }
    Ok(())
}

/// Read a UI plugin's entry bundle.
///
/// The loader prefers to import the file straight over the asset protocol, but
/// that is a cross-origin module load and can be refused; this is the fallback,
/// which hands back the source so the loader can import it from a blob URL
/// instead.
pub async fn read_entry(plugin_id: &str) -> crate::Result<Option<String>> {
    let Some(path) = resolve_entry(plugin_id).await? else {
        return Ok(None);
    };
    let size = io::metadata(&path).await?.len();
    if size > MAX_ENTRY_BYTES {
        return Err(crate::ErrorKind::InputError(format!(
            "Plugin '{plugin_id}' has an entry bundle of {size} bytes, which \
             is over the {} MiB limit",
            MAX_ENTRY_BYTES / (1024 * 1024)
        ))
        .into());
    }
    let bytes = io::read(&path).await?;
    Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_that_could_escape_the_plugins_directory_are_rejected() {
        for id in ["../../etc", "..", "a/b", "", ".hidden"] {
            assert!(checked_id(id).is_err(), "id {id} was accepted");
        }
        assert_eq!(checked_id("com.example.demo").unwrap(), "com.example.demo");
    }

    #[test]
    fn a_record_is_created_once_per_plugin() {
        let mut registry = PluginStateFile::default();
        record_mut(&mut registry, "a").enabled = false;
        record_mut(&mut registry, "a").granted = vec!["style".to_string()];
        assert_eq!(registry.plugins.len(), 1);
        assert!(!registry.plugins[0].enabled);
        assert_eq!(registry.plugins[0].granted, ["style"]);
    }

    #[test]
    fn the_registry_round_trips_through_json() {
        let registry = PluginStateFile {
            plugins: vec![PluginRecord {
                id: "com.example.demo".to_string(),
                enabled: true,
                granted: vec!["style".to_string()],
                installed_at: Some(1_700_000_000),
                source: Some("https://example.com/plugin.zip".to_string()),
            }],
            hot_reload: true,
        };
        let encoded = serde_json::to_vec(&registry).unwrap();
        let decoded: PluginStateFile =
            serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded.plugins.len(), 1);
        assert_eq!(decoded.plugins[0].granted, ["style"]);
    }

    #[test]
    fn an_empty_registry_file_is_valid() {
        let decoded: PluginStateFile = serde_json::from_slice(b"{}").unwrap();
        assert!(decoded.plugins.is_empty());
    }
}
