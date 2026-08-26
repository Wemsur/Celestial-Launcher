use crate::state::instances::{InstanceLaunchOverridesData, adapters::sqlite::instance_rows};
use crate::state::{ContentSet, Instance, InstanceLink, ReleaseChannel, State};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tokio::fs as tokio_fs;
use tracing::{debug, info, warn};

/// Retry a closure on `std::io::Error`, with 200ms delay between attempts.
/// Modeled after PCL2's Retrier: handles Windows file locks that clear quickly.
fn retry_io<F, T>(mut f: F, max_attempts: u32) -> crate::Result<T>
where
    F: FnMut() -> Result<T, std::io::Error>,
{
    let mut attempt = 0;
    loop {
        match f() {
            Ok(v) => return Ok(v),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied || e.raw_os_error() == Some(5) => {
                attempt += 1;
                if attempt >= max_attempts {
                    return Err(e.into());
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            Err(e) => return Err(e.into()),
        }
    }
}

pub const LIBRARIES_FILE_NAME: &str = "libraries.json";

/// Current `libraries.json` schema version.
///
/// Bump this when a sidecar field is added that existing instances need
/// backfilled, and extend `run_schema_backfills` with the new step. The
/// `migrated` flag only tracks the one-time SQLite import, so it cannot express
/// "already imported, but predates field X".
pub const LIBRARIES_SCHEMA_VERSION: u32 = 1;

/// Best-effort creation timestamp for an instance directory.
///
/// Instances imported from SQLite predate the `created` field in the JSON
/// sidecars, so their creation time has to be recovered from the filesystem.
/// Falls back to the modification time on filesystems that do not record a
/// birth time. Returns `None` only when the directory cannot be stat'd.
pub(crate) fn dir_created_time(dir: &Path) -> Option<DateTime<Utc>> {
    let metadata = fs::metadata(dir).ok()?;
    let time = metadata
        .created()
        .or_else(|_| metadata.modified())
        .ok()?;
    Some(DateTime::<Utc>::from(time))
}

/// Modification timestamp for an instance directory.
pub(crate) fn dir_modified_time(dir: &Path) -> Option<DateTime<Utc>> {
    let metadata = fs::metadata(dir).ok()?;
    Some(DateTime::<Utc>::from(metadata.modified().ok()?))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LibraryInfo {
    #[serde(default)]
    pub name: String,
    pub path: String,
    #[serde(rename = "type")]
    pub format: InstanceFormat,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum InstanceFormat {
    #[default]
    Modrinth,
    Minecraft,
}

/// Check whether a library path represents .minecraft format.
pub fn is_minecraft_format_path(path: &str) -> bool {
    InstanceFormat::from_path(path) == InstanceFormat::Minecraft
}

impl From<&str> for InstanceFormat {
    fn from(s: &str) -> Self {
        match s {
            "minecraft" => Self::Minecraft,
            "modrinth" => Self::Modrinth,
            _ => Self::default(),
        }
    }
}

impl InstanceFormat {
    /// Detect format from a library path string.
    pub(crate) fn from_path(path: &str) -> Self {
        if path.contains(".minecraft") {
            Self::Minecraft
        } else {
            Self::default()
        }
    }
}

impl From<InstanceFormat> for &str {
    fn from(format: InstanceFormat) -> Self {
        match format {
            InstanceFormat::Modrinth => "modrinth",
            InstanceFormat::Minecraft => "minecraft",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LibrariesConfig {
    pub libraries: Vec<LibraryInfo>,
    #[serde(default)]
    pub migrated: bool,
    /// Schema version of the sidecar data, used to drive one-time backfills.
    #[serde(default)]
    pub schema_version: u32,
    /// Path of the library that was last active on the home page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_library_path: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct InstanceJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_played: Option<DateTime<Utc>>,
    /// When the instance was first created (stored in instance.json).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub game_version: Option<String>,
    pub loader: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loader_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<InstanceLink>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch_overrides: Option<InstanceLaunchOverridesData>,
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(default)]
    pub update_channel: ReleaseChannel,
    #[serde(default)]
    pub submitted_time_played: u64,
    #[serde(default)]
    pub recent_time_played: u64,
    #[serde(default = "default_install_stage")]
    pub install_stage: String,
    #[serde(default)]
    pub quarantined: bool,
    /// Format used when this instance was created.
    /// Derived from the parent library's format when missing.
    #[serde(default)]
    pub library_format: InstanceFormat,
}

fn default_install_stage() -> String {
    crate::state::InstanceInstallStage::Installed.as_str().to_string()
}

impl InstanceJson {
    pub(crate) fn read_from_dir(dir: &Path) -> crate::Result<Option<Self>> {
        let json_path = dir.join("instance.json");
        if !json_path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&json_path)?;
        let instance: Self = serde_json::from_str(&content)?;
        Ok(Some(instance))
    }

    pub(crate) fn write_to_dir(&self, dir: &Path) -> crate::Result<()> {
        fs::create_dir_all(dir)?;
        let json_path = dir.join("instance.json");
        let content = serde_json::to_string_pretty(self)?;
        fs::write(&json_path, content)?;
        Ok(())
    }

    pub(crate) fn from_instance_and_content_set(
        instance: &Instance,
        content_set: &ContentSet,
        _absolute_path: &str,
        link: Option<InstanceLink>,
    ) -> Self {
        Self {
            name: Some(instance.name.clone()),
            icon_path: instance.icon_path.clone(),
            created: Some(instance.created),
            last_played: instance.last_played,
            game_version: Some(content_set.game_version.clone()),
            loader: Some(content_set.loader.as_str().to_string()),
            loader_version: content_set.loader_version.clone(),
            link,
            submitted_time_played: instance.submitted_time_played,
            recent_time_played: instance.recent_time_played,
            install_stage: default_install_stage(),
            quarantined: false,
            library_format: instance.library_format.clone(),
            groups: vec![],
            update_channel: instance.update_channel,
            launch_overrides: None,
        }
    }

    pub(crate) fn from_instance(instance: &Instance) -> Self {
        Self {
            name: Some(instance.name.clone()),
            icon_path: instance.icon_path.clone(),
            created: Some(instance.created),
            last_played: instance.last_played,
            submitted_time_played: instance.submitted_time_played,
            recent_time_played: instance.recent_time_played,
            ..Default::default()
        }
    }

    /// Derive an Instance from this JSON sidecar and a known absolute path.
    pub(crate) fn to_instance(
        &self,
        absolute_path: &str,
    ) -> Instance {
        self.to_instance_with_format(absolute_path, self.library_format.clone())
    }

    /// Derive an Instance from this JSON sidecar and a known absolute path,
    /// overriding the library format.
    pub(crate) fn to_instance_with_format(
        &self,
        absolute_path: &str,
        format: InstanceFormat,
    ) -> Instance {
        let id = instance_id_from_path(absolute_path);
        let dir = Path::new(absolute_path);
        // Prefer stored name, then fall back to directory name
        let name = self
            .name
            .clone()
            .or_else(|| Path::new(absolute_path).file_name().and_then(|n| n.to_str()).map(|s| s.to_string()));
        Instance {
            id: id.clone(),
            path: absolute_path.to_string(),
            applied_content_set_id: None,
            install_stage: crate::state::InstanceInstallStage::from_str(&self.install_stage),
            launcher_feature_version:
                crate::state::LauncherFeatureVersion::MOST_RECENT,
            update_channel: self.update_channel,
            name: name.unwrap_or_default(),
            icon_path: self.icon_path.clone(),
            // Never substitute `Utc::now()` here: this runs on every scan, so a
            // fabricated timestamp would make the instance look freshly created
            // each time and outrank genuinely recent instances when callers sort
            // by `last_played` with a `created` fallback.
            created: self
                .created
                .or_else(|| dir_created_time(dir))
                .unwrap_or_else(Utc::now),
            modified: dir_modified_time(dir).unwrap_or_else(Utc::now),
            last_played: self.last_played,
            submitted_time_played: self.submitted_time_played,
            recent_time_played: self.recent_time_played,
            library_format: format,
        }
    }
    pub(crate) fn launch_overrides(&self, instance_id: &str) -> crate::state::InstanceLaunchOverrides {
        match self.launch_overrides.as_ref() {
            Some(data) => crate::state::InstanceLaunchOverrides {
                instance_id: instance_id.to_string(),
                java_path: data.java_path.clone(),
                extra_launch_args: data.extra_launch_args.clone(),
                custom_env_vars: data.custom_env_vars.clone(),
                memory: data.memory,
                force_fullscreen: data.force_fullscreen,
                game_resolution: data.game_resolution,
                hooks: data.hooks.clone(),
            },
            None => crate::state::InstanceLaunchOverrides::empty(instance_id.to_string()),
        }
    }

    pub(crate) fn groups(&self) -> &[String] {
        &self.groups
    }

    pub(crate) fn update_channel(&self) -> ReleaseChannel {
        self.update_channel
    }
}

/// Lightweight sidecar for `.minecraft`-format instances that lack an `instance.json`.
/// Stores only launcher-managed settings (no game_version/loader/loader_version).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct CelestialJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_played: Option<DateTime<Utc>>,
    /// When the instance was first created. Recovered from the instance
    /// directory when absent, so ordering by creation time stays stable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<InstanceLink>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch_overrides: Option<InstanceLaunchOverridesData>,
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(default)]
    pub update_channel: ReleaseChannel,
    #[serde(default)]
    pub submitted_time_played: u64,
    #[serde(default)]
    pub recent_time_played: u64,
}

impl CelestialJson {
    pub(crate) fn read_from_dir(dir: &Path) -> crate::Result<Option<Self>> {
        let json_path = dir.join("celestial.json");
        if !json_path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&json_path)?;
        let instance: Self = serde_json::from_str(&content)?;
        Ok(Some(instance))
    }

    pub(crate) fn write_to_dir(&self, dir: &Path) -> crate::Result<()> {
        fs::create_dir_all(dir)?;
        let json_path = dir.join("celestial.json");
        let content = serde_json::to_string_pretty(self)?;
        fs::write(&json_path, content)?;
        Ok(())
    }

    pub(crate) fn launch_overrides(
        &self,
        instance_id: &str,
    ) -> crate::state::InstanceLaunchOverrides {
        match self.launch_overrides.as_ref() {
            Some(data) => crate::state::InstanceLaunchOverrides {
                instance_id: instance_id.to_string(),
                java_path: data.java_path.clone(),
                extra_launch_args: data.extra_launch_args.clone(),
                custom_env_vars: data.custom_env_vars.clone(),
                memory: data.memory,
                force_fullscreen: data.force_fullscreen,
                game_resolution: data.game_resolution,
                hooks: data.hooks.clone(),
            },
            None => crate::state::InstanceLaunchOverrides::empty(
                instance_id.to_string(),
            ),
        }
    }

    pub(crate) fn groups(&self) -> &[String] {
        &self.groups
    }

    pub(crate) fn update_channel(&self) -> ReleaseChannel {
        self.update_channel
    }
}

/// Prefix of every instance ID derived from a filesystem path.
///
/// These identify JSON-backed instances, which deliberately have no row in the
/// `instances` table — their metadata lives in an `instance.json` /
/// `celestial.json` sidecar next to the instance.
pub const JSON_BACKED_ID_PREFIX: &str = "local:";

/// Whether an instance ID belongs to a JSON-backed instance.
///
/// Use this before writing an instance ID into any column with a foreign key to
/// `instances(id)`: for these IDs there is no such row, so the write would fail
/// the constraint.
pub fn is_json_backed_id(instance_id: &str) -> bool {
    instance_id.starts_with(JSON_BACKED_ID_PREFIX)
}

pub(crate) fn instance_id_from_path(path: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(path.as_bytes());
    let result = hasher.finalize();
    let hex: String = result
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("{JSON_BACKED_ID_PREFIX}{}", &hex[..32])
}

/// Returns the default Modrinth library path (`<home>/Minecraft/Modrinth/profiles`).
///
/// This mirrors the default app data directory shown in settings
/// (`<home>/Minecraft/Modrinth`) with the `profiles` subfolder appended.
pub fn default_library_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join("Minecraft")
        .join("Modrinth")
        .join("profiles")
}

/// Build the initial `libraries.json` contents for a fresh install: a single
/// "默认库" Modrinth library rooted at [`default_library_path`], marked as
/// migrated and at the current schema version so no further backfills run.
fn default_libraries_config() -> LibrariesConfig {
    let path = default_library_path().to_string_lossy().to_string();
    LibrariesConfig {
        libraries: vec![LibraryInfo {
            name: "默认库".to_string(),
            path: path.clone(),
            format: InstanceFormat::Modrinth,
        }],
        migrated: true,
        schema_version: LIBRARIES_SCHEMA_VERSION,
        active_library_path: Some(path),
    }
}

pub async fn get_libraries_config(state: &State) -> crate::Result<LibrariesConfig> {
    let path = state.directories.settings_dir.join(LIBRARIES_FILE_NAME);
    if !path.exists() {
        return Ok(LibrariesConfig {
            libraries: vec![],
            migrated: false,
            schema_version: 0,
            active_library_path: None,
        });
    }
    let content = fs::read_to_string(&path)?;
    let config: LibrariesConfig = serde_json::from_str(&content)?;
    Ok(config)
}

pub async fn save_libraries_config(
    state: &State,
    config: &LibrariesConfig,
) -> crate::Result<()> {
    let path = state.directories.settings_dir.join(LIBRARIES_FILE_NAME);
    let content = serde_json::to_string_pretty(config)?;
    fs::write(&path, content)?;
    Ok(())
}

pub async fn list_instances_from_json(
    state: &State,
) -> crate::Result<Vec<Instance>> {
    let config = get_libraries_config(state).await?;
    let mut instances = Vec::new();

    for library in &config.libraries {
        let lib_path = Path::new(&library.path);
        if !lib_path.exists() {
            continue;
        }

        // Determine which subdirectory to scan based on format
        let scan_root = match library.format {
            InstanceFormat::Modrinth => lib_path.join("profiles"),
            InstanceFormat::Minecraft => lib_path.join("versions"),
        };

        // For Modrinth, scan profiles/ directly; for Minecraft, scan versions/<instance>/
        let entries = if scan_root.exists() {
            match fs::read_dir(&scan_root) {
                Ok(e) => e,
                Err(_) => continue,
            }
        } else {
            // Fallback: scan library root directly (backwards compatibility)
            match fs::read_dir(lib_path) {
                Ok(e) => e,
                Err(_) => continue,
            }
        };

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }

            // For Modrinth format, require instance.json; for .minecraft format,
            // derive a minimal instance from the directory even without one.
            match library.format {
                InstanceFormat::Modrinth => {
                    let Some(instance_json) = (match InstanceJson::read_from_dir(&dir) {
                        Ok(v) => v,
                        Err(e) => {
                            warn!(
                                dir = ?dir,
                                error = %e,
                                "Failed to parse instance.json, skipping"
                            );
                            continue;
                        }
                    }) else {
                        continue;
                    };
                    let instance = instance_json.to_instance_with_format(
                        dir.to_string_lossy().as_ref(),
                        library.format.clone(),
                    );
                    instances.push(instance);
                }
                InstanceFormat::Minecraft => {
                    let dir_name = dir
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(|s| s.to_string());
                    let instance_json = match InstanceJson::read_from_dir(&dir) {
                        Ok(v) => v,
                        Err(e) => {
                            tracing::warn!(
                                dir = ?dir,
                                error = %e,
                                "Failed to parse instance.json, skipping"
                            );
                            continue;
                        }
                    };
                    if let Some(instance_json) = instance_json {
                        let mut instance = instance_json.to_instance_with_format(
                            dir.to_string_lossy().as_ref(),
                            library.format.clone(),
                        );
                        // .minecraft: name is always the directory name
                        instance.name = dir_name.clone().unwrap_or_default();
                        instances.push(instance);
                    } else {
                        // No sidecar — read other settings from celestial.json,
                        // but name always comes from the directory.
                        let id = instance_id_from_path(dir.to_string_lossy().as_ref());
                        let celestial = match CelestialJson::read_from_dir(&dir) {
                            Ok(v) => v,
                            Err(e) => {
                                tracing::warn!(
                                    dir = ?dir,
                                    error = %e,
                                    "Failed to parse celestial.json, falling back to dir name"
                                );
                                None
                            }
                        };
                        instances.push(Instance {
                            id,
                            path: dir.to_string_lossy().to_string(),
                            applied_content_set_id: None,
                            install_stage: crate::state::InstanceInstallStage::Installed,
                            launcher_feature_version:
                                crate::state::LauncherFeatureVersion::MOST_RECENT,
                            update_channel: celestial
                                .as_ref()
                                .map(|c| c.update_channel.clone())
                                .unwrap_or_default(),
                            name: dir_name.clone().unwrap_or_default(),
                            icon_path: celestial.as_ref().and_then(|c| c.icon_path.clone()),
                            created: celestial
                                .as_ref()
                                .and_then(|c| c.created)
                                .or_else(|| dir_created_time(&dir))
                                .unwrap_or_else(Utc::now),
                            modified: dir_modified_time(&dir)
                                .unwrap_or_else(Utc::now),
                            last_played: celestial
                                .as_ref()
                                .and_then(|c| c.last_played),
                            submitted_time_played: celestial
                                .as_ref()
                                .map(|c| c.submitted_time_played)
                                .unwrap_or(0),
                            recent_time_played: celestial
                                .as_ref()
                                .map(|c| c.recent_time_played)
                                .unwrap_or(0),
                            library_format: library.format.clone(),
                        });
                    }
                }
            }
        }
    }

    Ok(instances)
}

pub(crate) async fn migrate_instances_from_db(
    state: &State,
) -> crate::Result<()> {
    let db_instances =
        instance_rows::list_instance_metadata(&state.pool).await?;

    let mut lib_map: HashMap<String, LibraryInfo> = HashMap::new();

    for record in &db_instances {
        let instance = &record.instance;
        let fallback_path =
            resolve_instance_dir(state, &instance.path);

        // Determine the actual absolute path of the instance directory.
        // If it exists at the DB-resolved location, use that; otherwise
        // use the DB-relative path as a placeholder (pre-existing profiles
        // may live elsewhere and will be discovered on the next scan).
        let absolute_path_str = if fallback_path.exists() {
            fallback_path.to_string_lossy().to_string()
        } else {
            instance.path.clone()
        };

        // Determine which library this instance belongs to by walking up
        // the (real or placeholder) path.  Use the real path when available.
        let path_to_walk = if fallback_path.exists() {
            absolute_path_str.as_str()
        } else {
            &instance.path
        };
        let path_parts: Vec<&str> = path_to_walk.split('/').collect();

        let mut lib_path_opt: Option<String> = None;
        for i in 1..path_parts.len() {
            let candidate: String = path_parts[..i].join("/");
            if lib_map.contains_key(&candidate) {
                lib_path_opt = Some(candidate);
                break;
            }
        }

        if lib_path_opt.is_none() {
            if let Some(parent) =
                Path::new(path_to_walk).parent().map(|p| p.to_string_lossy().to_string())
            {
                lib_path_opt = Some(parent);
            }
        }

        let lib_path =
            lib_path_opt.unwrap_or_else(|| "unknown-library".to_string());

        let format = InstanceFormat::from_path(&lib_path);

        lib_map.entry(lib_path.clone()).or_insert_with(|| LibraryInfo {
            name: String::new(),
            path: lib_path,
            format: format.clone(),
        });

        // Only write instance.json if the directory actually exists.
        // We no longer persist derived fields (version, loader) — they are
        // detected from the filesystem on each scan.
        if fallback_path.exists() {
            let mut instance_json = InstanceJson::from_instance_and_content_set(
                instance,
                &record.applied_content_set,
                &absolute_path_str,
                Some(record.link.clone()),
            );
            instance_json.library_format = format.clone();
            instance_json.update_channel = instance.update_channel;
            instance_json.groups = record.group_ids.clone();
            instance_json.launch_overrides =
                Some(InstanceLaunchOverridesData::from(&record.launch_overrides));
            if let Err(e) = instance_json.write_to_dir(&fallback_path) {
                tracing::warn!(
                    "Failed to write instance.json for {}: {e}",
                    instance.id
                );
            }
        }
    }

    let libraries_vec: Vec<LibraryInfo> =
        lib_map.into_values().collect();

    let config = LibrariesConfig {
        libraries: libraries_vec,
        migrated: true,
        // Migration writes `created` into every sidecar it touches, so the data
        // it produces is already at the current schema version.
        schema_version: LIBRARIES_SCHEMA_VERSION,
        active_library_path: None,
    };

    save_libraries_config(state, &config).await?;

    // Second pass: fix missing `link` and other fields in existing instance.json files.
    // All other metadata (version, loader) is derivable from the filesystem.
    for record in &db_instances {
        let dir =
            resolve_instance_dir(state, &record.instance.path);
        if !dir.exists() {
            continue;
        }
        match InstanceJson::read_from_dir(&dir) {
            Ok(Some(mut json)) => {
                if json.link.is_none() {
                    json.link = Some(record.link.clone());
                }
                if json.created.is_none() {
                    json.created = Some(record.instance.created);
                }
                json.update_channel = record.instance.update_channel;
                json.groups = record.group_ids.clone();
                json.launch_overrides =
                    Some(InstanceLaunchOverridesData::from(&record.launch_overrides));
                if let Err(e) = json.write_to_dir(&dir) {
                    tracing::warn!(
                        "Failed to update instance.json for {}: {e}",
                        record.instance.id
                    );
                }
            }
            Ok(None) => {}
            Err(e) => {
                tracing::warn!(
                    "Failed to read instance.json for {}: {e}",
                    record.instance.path
                );
            }
        }
    }

    Ok(())
}

pub(crate) async fn ensure_migration_done(state: &State) -> crate::Result<()> {
    let mut config = get_libraries_config(state).await?;
    let saved_active = config.active_library_path.clone();
    if !config.migrated {
        migrate_instances_from_db(state).await?;
        config = get_libraries_config(state).await?;
    }
    // Fresh install (or an emptied config): seed a single default Modrinth
    // library so the app always has a usable library to work with.
    if config.libraries.is_empty() {
        config = default_libraries_config();
        save_libraries_config(state, &config).await?;
    }
    // Restore saved active tab after migration
    if let Some(active) = saved_active {
        if config.libraries.iter().any(|l| l.path == active) {
            config.active_library_path = Some(active);
            save_libraries_config(state, &config).await?;
        }
    }
    if config.schema_version < LIBRARIES_SCHEMA_VERSION {
        run_schema_backfills(&config).await?;
        config.schema_version = LIBRARIES_SCHEMA_VERSION;
        save_libraries_config(state, &config).await?;
    }
    Ok(())
}

/// Apply one-time data fixes to sidecars written by an older schema version.
///
/// This is best-effort: individual failures are logged and skipped so a single
/// unwritable instance directory cannot block startup.
async fn run_schema_backfills(config: &LibrariesConfig) -> crate::Result<()> {
    let mut patched = 0usize;

    for library in &config.libraries {
        let lib_path = Path::new(&library.path);
        if !lib_path.exists() {
            continue;
        }
        let scan_root = match library.format {
            InstanceFormat::Modrinth => lib_path.join("profiles"),
            InstanceFormat::Minecraft => lib_path.join("versions"),
        };
        // Mirror `list_instances_from_json`: a registered library path may already
        // point at the container directory (the default Modrinth library is
        // `<...>/Modrinth/profiles`), in which case joining again misses.
        // Scanning the library root is safe here because this pass only patches
        // sidecars that already exist and never creates one, so shared content
        // folders such as `mods/` are skipped for lack of a sidecar.
        let scan_root = if scan_root.exists() {
            scan_root
        } else {
            lib_path.to_path_buf()
        };
        let Ok(entries) = fs::read_dir(&scan_root) else {
            continue;
        };

        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let Some(created) = dir_created_time(&dir) else {
                continue;
            };

            match InstanceJson::read_from_dir(&dir) {
                Ok(Some(mut json)) => {
                    if json.created.is_some() {
                        continue;
                    }
                    json.created = Some(created);
                    match json.write_to_dir(&dir) {
                        Ok(()) => patched += 1,
                        Err(e) => warn!(
                            dir = ?dir,
                            error = %e,
                            "Failed to backfill created into instance.json"
                        ),
                    }
                }
                Ok(None) => {
                    // `.minecraft` instances without an instance.json keep their
                    // launcher-managed settings in celestial.json. If neither
                    // sidecar exists, leave the directory untouched — the read
                    // path rederives `created` from the directory itself.
                    if library.format != InstanceFormat::Minecraft {
                        continue;
                    }
                    match CelestialJson::read_from_dir(&dir) {
                        Ok(Some(mut celestial)) => {
                            if celestial.created.is_some() {
                                continue;
                            }
                            celestial.created = Some(created);
                            match celestial.write_to_dir(&dir) {
                                Ok(()) => patched += 1,
                                Err(e) => warn!(
                                    dir = ?dir,
                                    error = %e,
                                    "Failed to backfill created into celestial.json"
                                ),
                            }
                        }
                        Ok(None) => {}
                        Err(e) => warn!(
                            dir = ?dir,
                            error = %e,
                            "Failed to read celestial.json during backfill"
                        ),
                    }
                }
                Err(e) => warn!(
                    dir = ?dir,
                    error = %e,
                    "Failed to read instance.json during backfill"
                ),
            }
        }
    }

    if patched > 0 {
        info!("Backfilled created timestamp into {patched} instance sidecar(s)");
    }
    Ok(())
}

/// Resolve an instance directory from its path string, respecting the
/// library's format.
///
/// For Modrinth format: `<library>/profiles/<path>` (if relative).
/// For `.minecraft` format: `<library>/versions/<path>` (if relative).
/// Absolute paths are returned as-is.
pub(crate) fn resolve_instance_dir_for_library(
    library_path: &Path,
    instance_path: &str,
    library_format: &InstanceFormat,
) -> PathBuf {
    let path = Path::new(instance_path);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    let root = match library_format {
        InstanceFormat::Modrinth => library_path.join("profiles"),
        InstanceFormat::Minecraft => library_path.join("versions"),
    };
    root.join(path)
}

/// Resolve an instance directory from its path string.
/// If the path is absolute, return it directly (supports multi-library).
/// If the path is relative, join it with instances_dir (legacy support).
pub fn resolve_instance_dir(
    state: &State,
    instance_path: &str,
) -> PathBuf {
    let path = Path::new(instance_path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        state.directories.instances_dir().join(instance_path)
    }
}

/// Overload that takes a `DirectoryInfo` directly instead of a full `State`.
#[inline]
pub fn resolve_instance_dir_with_dirs(
    dirs: &crate::state::DirectoryInfo,
    instance_path: &str,
) -> PathBuf {
    let path = Path::new(instance_path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        dirs.instances_dir().join(instance_path)
    }
}

/// Find the library info for a given instance path by walking up the path.
/// Returns None if the instance is not under any configured library.
pub(crate) fn find_library_for_instance<'a>(
    config: &'a LibrariesConfig,
    instance_path: &Path,
) -> Option<&'a LibraryInfo> {
    let mut current = Some(instance_path);
    while let Some(path) = current {
        for lib in &config.libraries {
            let lib_path = Path::new(&lib.path);
            if path.starts_with(lib_path) {
                return Some(lib);
            }
        }
        current = path.parent();
    }
    None
}

/// Returns the library root directory (the configured library path) for an instance.
/// For Modrinth format: `<library>/profiles/<instance>/` → `<library>`
/// For .minecraft format: `<library>/versions/<instance>/` → `<library>`
pub(crate) fn instance_library_root(
    instance_dir: &Path,
    library_format: &InstanceFormat,
) -> Option<PathBuf> {
    let (prefix, _) = match library_format {
        InstanceFormat::Modrinth => ("profiles", true),
        InstanceFormat::Minecraft => ("versions", true),
    };
    if !prefix.is_empty() {
        // Walk up to find the prefix directory
        let mut current = Some(instance_dir);
        while let Some(path) = current {
            if let Some(parent) = path.parent() {
                if let Some(name) = parent.file_name() {
                    if name.to_string_lossy() == prefix {
                        // parent is profiles/ or versions/
                        if let Some(grandparent) = parent.parent() {
                            return Some(grandparent.to_path_buf());
                        }
                    }
                }
                current = Some(parent);
            } else {
                break;
            }
        }
    }
    // Fallback: the instance dir itself is the root
    Some(instance_dir.to_path_buf())
}

/// Get the version jar path for an instance.
/// For Minecraft format: looks in `versions/<instance>/<version>.jar` or `versions/<instance>/<version>-natives/<version>.jar`
/// For Modrinth format: uses the launcher-managed version cache
pub(crate) fn instance_version_jar_path(
    instance_dir: &Path,
    version_id: &str,
    library_format: &InstanceFormat,
) -> Option<PathBuf> {
    match library_format {
        InstanceFormat::Minecraft => {
            // Check for version jar directly in the instance dir
            let jar_path = instance_dir.join(format!("{version_id}.jar"));
            if jar_path.exists() {
                return Some(jar_path);
            }
            None
        }
        InstanceFormat::Modrinth => None,
    }
}

/// Get the natives directory for an instance.
/// For Minecraft format: `versions/<instance>/natives/` or `versions/<instance>/<version>-natives/`
/// For Modrinth format: uses the launcher-managed natives cache
pub(crate) fn instance_natives_dir(
    instance_dir: &Path,
    version_id: &str,
    library_format: &InstanceFormat,
) -> PathBuf {
    match library_format {
        InstanceFormat::Minecraft => {
            // Try instance-specific natives first, then generic natives dir
            let specific = instance_dir.join(format!("{version_id}-natives"));
            if specific.exists() {
                specific
            } else if instance_dir.join("natives").exists() {
                instance_dir.join("natives")
            } else {
                instance_dir.join("natives")
            }
        }
        InstanceFormat::Modrinth => PathBuf::new(),
    }
}

/// Get the libraries directory for a `.minecraft` format instance.
/// For Minecraft format: returns `<library>/libraries/` (shared across instances).
/// For Modrinth format: returns empty path (uses launcher-managed cache).
pub(crate) fn instance_libraries_dir(
    instance_dir: &Path,
    library_format: &InstanceFormat,
) -> PathBuf {
    match library_format {
        InstanceFormat::Minecraft => {
            if let Some(root) = instance_library_root(instance_dir, library_format) {
                root.join("libraries")
            } else {
                instance_dir.join("libraries")
            }
        }
        InstanceFormat::Modrinth => PathBuf::new(),
    }
}

/// Get the assets directory for a `.minecraft` format instance.
/// For Minecraft format: returns `<library>/assets/` (shared across instances).
/// For Modrinth format: returns empty path (uses launcher-managed cache).
pub(crate) fn instance_assets_dir(
    instance_dir: &Path,
    library_format: &InstanceFormat,
) -> PathBuf {
    match library_format {
        InstanceFormat::Minecraft => {
            if let Some(root) = instance_library_root(instance_dir, library_format) {
                root.join("assets")
            } else {
                instance_dir.join("assets")
            }
        }
        InstanceFormat::Modrinth => PathBuf::new(),
    }
}

/// Get the shared content directory for a library format.
/// For Minecraft format: returns the library root shared dirs (mods/, config/, etc.)
/// For Modrinth format: returns empty path (everything is instance-local)
pub(crate) fn shared_content_dirs(
    instance_dir: &Path,
    library_format: &InstanceFormat,
) -> Vec<PathBuf> {
    match library_format {
        InstanceFormat::Minecraft => {
            let mut dirs = Vec::new();
            if let Some(root) = instance_library_root(instance_dir, library_format) {
                for dir_name in ["mods", "config", "saves", "resourcepacks", "shaderpacks", "datapacks"] {
                    let shared = root.join(dir_name);
                    if shared.exists() {
                        dirs.push(shared);
                    }
                }
            }
            dirs
        }
        InstanceFormat::Modrinth => Vec::new(),
    }
}

/// Get the version ID associated with a `.minecraft` format instance directory.
/// Returns the instance directory name (e.g. `1.21.1self-x`).
pub(crate) fn instance_version_id(instance_dir: &Path) -> String {
    instance_dir
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string()
}

/// Read and parse the version info JSON from the instance directory.
/// For .minecraft format instances, this is `<version>.json` in the instance dir.
/// Falls back to scanning for any `.json` file if no `<version_id>.json` exists.
pub fn read_version_json_from_instance_dir(
    instance_dir: &Path,
) -> crate::Result<Option<serde_json::Value>> {
    let version_id = instance_version_id(instance_dir);
    let target_path = instance_dir.join(format!("{version_id}.json"));
    if target_path.exists() {
        let content = fs::read_to_string(&target_path)?;
        let val: serde_json::Value = serde_json::from_str(&content)?;
        return Ok(Some(val));
    }
    // Fall back to scanning for any .json
    if let Ok(entries) = fs::read_dir(instance_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                let content = match fs::read_to_string(&path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                let val: serde_json::Value = match serde_json::from_str(&content) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                return Ok(Some(val));
            }
        }
    }
    Ok(None)
}

/// Check if a Minecraft-format instance has its own local version data
/// (version jar and/or version info JSON).
pub(crate) fn instance_has_local_version(
    instance_dir: &Path,
    version_id: &str,
) -> bool {
    let jar_path = instance_dir.join(format!("{version_id}.jar"));
    let json_path = instance_dir.join(format!("{version_id}.json"));
    jar_path.exists() || json_path.exists()
}

/// Write a version info JSON to the instance directory as `<version>.json`.
/// Called after download during `.minecraft` format install.
pub(crate) async fn write_version_info_to_instance_dir(
    instance_dir: &Path,
    version_id: &str,
    info: &serde_json::Value,
) -> crate::Result<()> {
    tokio_fs::create_dir_all(instance_dir).await?;
    let json_path = instance_dir.join(format!("{version_id}.json"));
    let content = serde_json::to_string_pretty(info)?;
    tokio_fs::write(&json_path, content).await?;
    Ok(())
}

/// Write a client JAR to the instance directory as `<version>.jar`.
/// Called after download during `.minecraft` format install.
pub(crate) async fn write_version_jar_to_instance_dir(
    instance_dir: &Path,
    version_id: &str,
    bytes: &[u8],
) -> crate::Result<()> {
    tokio_fs::create_dir_all(instance_dir).await?;
    let jar_path = instance_dir.join(format!("{version_id}.jar"));
    tokio_fs::write(&jar_path, bytes).await?;
    Ok(())
}

/// Find a JSON-backed instance by ID. Returns the path used to locate it.
///
/// Performs a full scan of all configured libraries on each call. Use
/// `find_json_instance_by_id_fast` when you already have the resolved path.
pub async fn find_json_instance(
    state: &State,
    instance_id: &str,
) -> crate::Result<Option<PathBuf>> {
    let config = get_libraries_config(state).await?;
    for library in &config.libraries {
        let lib_path = Path::new(&library.path);
        if !lib_path.exists() {
            continue;
        }

        let scan_root = match library.format {
            InstanceFormat::Modrinth => lib_path.join("profiles"),
            InstanceFormat::Minecraft => lib_path.join("versions"),
        };

        let entries = if scan_root.exists() {
            match fs::read_dir(&scan_root) {
                Ok(e) => e,
                Err(_) => continue,
            }
        } else {
            match fs::read_dir(lib_path) {
                Ok(e) => e,
                Err(_) => continue,
            }
        };

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let Some(instance_json) =
                InstanceJson::read_from_dir(&dir)?
            else {
                // For .minecraft format, derive instance ID from path and check for a version JSON
                if library.format == InstanceFormat::Minecraft {
                    let id = instance_id_from_path(dir.to_string_lossy().as_ref());
                    if id == instance_id {
                        return Ok(Some(resolve_instance_dir(state, &dir.to_string_lossy().to_string())));
                    }
                }
                continue;
            };
            let instance = instance_json.to_instance_with_format(
                dir.to_string_lossy().as_ref(),
                library.format.clone(),
            );
            if instance.id == instance_id {
                return Ok(Some(resolve_instance_dir(state, &instance.path)));
            }
        }
    }
    Ok(None)
}

/// Detect the Minecraft game version from the instance directory.
///
/// For `.minecraft` format: reads `clientVersion` from the first version JSON
/// found in the instance directory.
/// For Modrinth-style profiles that share a global `versions/` cache:
/// scans `versions/<id>/<id>.json` entries and returns the first match.
/// Falls back to the instance directory name if nothing is found.
pub fn detect_game_version_from_dir(dir: &Path) -> Option<String> {
    // 1. Read clientVersion from a version JSON directly in the instance
    //    directory (`.minecraft` format: `versions/<instance>/<version>.json`)
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let content = match fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let Ok(val) = serde_json::from_str::<Value>(&content) else {
                continue;
            };
            if let Some(cv) = val.get("clientVersion").and_then(|v| v.as_str()) {
                if !cv.is_empty() {
                    debug!(
                        "Read clientVersion '{}' from {}",
                        cv,
                        path.display()
                    );
                    return Some(cv.to_string());
                }
            }
            // Fall back to the JSON filename stem if it looks like a version ID
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if stem.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false)
                    && stem.contains('.')
                {
                    debug!(
                        "Using JSON stem '{}' as version from {}",
                        stem,
                        path.display()
                    );
                    return Some(stem.to_string());
                }
            }
        }
    }

    // 2. Check for a shared versions/ cache (Modrinth-style profiles)
    let versions_dir = dir.join("versions");
    if versions_dir.exists() {
        if let Ok(entries) = fs::read_dir(&versions_dir) {
            let mut best: Option<String> = None;
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let id = path.file_name()?.to_str()?;
                    let json_path = path.join(format!("{id}.json"));
                    if json_path.exists() {
                        debug!(
                            "Found version manifest at {}: {id}",
                            json_path.display()
                        );
                        best = Some(id.to_string());
                    }
                }
            }
            if let Some(v) = best {
                return Some(v);
            }
        }
    }

    // 3. Fallback: use the instance directory name (e.g. "1.21.4" or "My Instance")
    dir.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(|s| s.to_string())
}

/// Detect the Minecraft game version from a jar filename. Handles both
/// explicit `mc1.X.Y` prefixes (e.g. `mod-mc1.20.1.jar`) and bare version
/// suffixes (e.g. `mod-1.20.1.jar`).
pub fn detect_game_version_from_jar_name(name: &str) -> Option<String> {
    // Strip .jar suffix
    let stem = name.strip_suffix(".jar")?;

    // Strip known prefixes before bare version numbers
    for prefix in [
        "minecraft_server.",
        "minecraft.",
        "server.",
        "client.",
    ] {
        if let Some(rest) = stem.strip_prefix(prefix) {
            return Some(rest.to_string());
        }
    }

    // Look for explicit mc1.X.Y pattern anywhere in the name
    static MC_VERSION_RE: std::sync::OnceLock<regex::Regex> =
        std::sync::OnceLock::new();
    let re = MC_VERSION_RE.get_or_init(|| {
        regex::Regex::new(r"\bmc(\d+\.\d+(?:\.\d+)?)\b").unwrap()
    });
    if let Some(cap) = re.captures(stem) {
        return Some(cap[1].to_string());
    }

    // Look for a bare version-like suffix (e.g. "mod-1.21.8.jar" → "1.21.8")
    static MINECRAFT_VERSION_RE: std::sync::OnceLock<regex::Regex> =
        std::sync::OnceLock::new();
    let ver_re = MINECRAFT_VERSION_RE.get_or_init(|| {
        regex::Regex::new(r"(?<![\d.])(\d+\.\d+(?:\.\d+)?)(?![\d.])").unwrap()
    });
    let versions: Vec<&str> =
        ver_re.find_iter(stem).map(|m| m.as_str()).collect();
    versions.into_iter().last().map(String::from)
}

/// Detect the loader from the instance directory by reading the version JSON's
/// `mainClass` field. Falls back to Vanilla if no JSON is found.
pub fn detect_loader_from_dir(dir: &Path) -> crate::state::ModLoader {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return crate::state::ModLoader::Vanilla,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        if ext != "json" {
            continue;
        }
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let Ok(val) = serde_json::from_str::<Value>(&content) else {
            continue;
        };
        let Some(main_class) = val.get("mainClass").and_then(|v| v.as_str()) else {
            continue;
        };
        return detect_loader_from_main_class(main_class);
    }
    crate::state::ModLoader::Vanilla
}

/// Parse a Minecraft launcher `mainClass` string and return the corresponding loader.
fn detect_loader_from_main_class(main_class: &str) -> crate::state::ModLoader {
    let mc = main_class.to_lowercase();
    if mc.contains("knot")
        || mc.contains("fabric.loader")
        || mc.contains("fabricmc")
    {
        return crate::state::ModLoader::Fabric;
    }
    if mc.contains("quilt") {
        return crate::state::ModLoader::Quilt;
    }
    if mc.contains("neoforge") {
        return crate::state::ModLoader::NeoForge;
    }
    if mc.contains("forge") {
        return crate::state::ModLoader::Forge;
    }
    crate::state::ModLoader::Vanilla
}

/// Check whether an instance is quarantined — covers both DB-backed and
/// JSON-backed instances. For JSON-backed instances the quarantine flag is
/// read from the sidecar file; for DB-backed ones the DB table is queried.
pub async fn is_instance_quarantined(
    instance_id: &str,
    pool: &sqlx::SqlitePool,
) -> crate::Result<bool> {
    // DB-backed check
    if instance_rows::is_instance_quarantined(instance_id, pool).await? {
        return Ok(true);
    }
    // JSON-backed check
    let state = State::get().await?;
    let json_instances = list_instances_from_json(&state).await?;
    Ok(json_instances
        .iter()
        .find(|i| i.id == instance_id)
        .and_then(|inst| {
            let dir = resolve_instance_dir(&state, &inst.path);
            InstanceJson::read_from_dir(&dir).ok().flatten()
        })
        .map(|j| j.quarantined)
        .unwrap_or(false))
}

/// Rename a `.minecraft`-format instance directory and its version files.
/// Returns the new absolute instance directory path on success.
pub(crate) fn rename_minecraft_instance(
    old_dir: &Path,
    new_name: &str,
) -> crate::Result<PathBuf> {
    // Compute new parent (versions/<new_name>)
    let parent = old_dir
        .parent()
        .ok_or_else(|| {
            crate::ErrorKind::InputError(
                "Cannot determine parent directory for instance".to_string(),
            )
        })?;
    let old_filestem = old_dir
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| {
            crate::ErrorKind::InputError("Invalid instance directory name".to_string())
        })?;
    let new_dir = parent.join(new_name);

    tracing::info!(
        "rename_minecraft_instance: old_dir={:?} old_filestem={} new_dir={:?}",
        old_dir,
        old_filestem,
        new_dir
    );

    // Rename the instance directory.
    // On Windows, os error 5 (Access Denied) often means a transient lock
    // (antivirus, indexer, etc.). We retry with a short delay, matching the
    // approach used by PCL2 launcher's Retrier.
    if new_dir.exists() {
        tracing::warn!(
            "rename_minecraft_instance: target dir already exists! old_dir={:?} new_dir={:?}",
            old_dir,
            new_dir
        );
        return Err(
            crate::ErrorKind::InputError(format!(
                "A directory named '{}' already exists",
                new_name
            ))
            .into(),
        );
    }
    info!("rename_minecraft_instance: renaming directory {:?} → {:?}", old_dir, new_dir);
    let new_dir = retry_io(
        || fs::rename(old_dir, &new_dir).map(|_| new_dir.clone()),
        5,
    )?;
    info!("rename_minecraft_instance: directory rename succeeded");

    // List entries to debug what files exist
    let entries: Vec<_> = match fs::read_dir(&new_dir) {
        Ok(rd) => rd.filter_map(|e| e.ok()).collect(),
        Err(e) => {
            tracing::error!(
                "rename_minecraft_instance: failed to read_dir after rename: {:?}: {}",
                new_dir,
                e
            );
            return Err(e.into());
        }
    };
    tracing::info!(
        "rename_minecraft_instance: entries in new dir ({}): {:?}",
        entries.len(),
        entries.iter().map(|e| e.file_name().to_string_lossy().to_string()).collect::<Vec<_>>()
    );

    // Rename only the version-specific files and natives directory.
    // Do NOT rename other files like content_cache.json, usercache.json, etc.
    let mut _had_jar = false;
    let mut had_json = false;
    for entry in entries {
        let file_name = entry.file_name().to_string_lossy().to_string();
        let new_file_name = if file_name == format!("{old_filestem}.jar") {
            format!("{new_name}.jar")
        } else if file_name == format!("{old_filestem}.json") {
            format!("{new_name}.json")
        } else if file_name == format!("{old_filestem}-natives") {
            format!("{new_name}-natives")
        } else {
            continue;
        };
        let src = entry.path();
        let dst = new_dir.join(&new_file_name);
        tracing::info!(
            "rename_minecraft_instance: renaming file {:?} → {:?}",
            src,
            dst
        );
        match retry_io(
            || fs::rename(&src, &dst).map(|_| ()),
            3,
        ) {
            Ok(()) => {
                tracing::info!("rename_minecraft_instance: file rename succeeded: {}", file_name);
                if file_name.ends_with(".jar") {
                    _had_jar = true;
                } else if file_name.ends_with(".json") {
                    had_json = true;
                }
            }
            Err(e) => {
                tracing::warn!(
                    "rename_minecraft_instance: file rename FAILED (after retries): {:?} → {:?}: {}. Skipping.",
                    src,
                    dst,
                    e
                );
            }
        }
    }
    // Ensure we renamed at least the version manifest; the jar may or may not exist.
    if !had_json {
        return Err(
            crate::ErrorKind::InputError(format!(
                "No version JSON found in instance directory"
            ))
            .into(),
        );
    }

    tracing::info!("rename_minecraft_instance: completed successfully, returning {:?}", new_dir);
    Ok(new_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_sidecar(dir: &Path, file: &str, body: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(file), body).unwrap();
    }

    /// The backfill has to cope with both shapes of registered library path:
    /// the default Modrinth library is registered as `<...>/Modrinth/profiles`
    /// (already the container), while `.minecraft` libraries are registered as
    /// the root and hold instances under `versions/`.
    #[tokio::test]
    async fn backfill_fills_created_for_both_library_layouts() {
        let tmp = tempfile::tempdir().unwrap();

        // Modrinth library registered *at* the profiles directory.
        let mr_root = tmp.path().join("Modrinth").join("profiles");
        let mr_instance = mr_root.join("BBS FS");
        write_sidecar(&mr_instance, "instance.json", r#"{"name":"BBS FS"}"#);

        // .minecraft library registered at the root, instances under versions/.
        let mc_root = tmp.path().join(".minecraft");
        let mc_instance = mc_root.join("versions").join("1.21.1self");
        write_sidecar(&mc_instance, "celestial.json", r#"{"name":"1.21.1self"}"#);

        // Library-root shared content: must not gain a sidecar.
        let shared_mods = mc_root.join("mods");
        fs::create_dir_all(&shared_mods).unwrap();

        let config = LibrariesConfig {
            libraries: vec![
                LibraryInfo {
                    name: "default".into(),
                    path: mr_root.to_string_lossy().to_string(),
                    format: InstanceFormat::Modrinth,
                },
                LibraryInfo {
                    name: "pcl".into(),
                    path: mc_root.to_string_lossy().to_string(),
                    format: InstanceFormat::Minecraft,
                },
            ],
            migrated: true,
            schema_version: 0,
            active_library_path: None,
        };

        run_schema_backfills(&config).await.unwrap();

        let mr = InstanceJson::read_from_dir(&mr_instance).unwrap().unwrap();
        assert!(
            mr.created.is_some(),
            "Modrinth library registered at the profiles dir must still be backfilled"
        );

        let mc = CelestialJson::read_from_dir(&mc_instance).unwrap().unwrap();
        assert!(
            mc.created.is_some(),
            "celestial.json sidecar must be backfilled"
        );

        assert!(
            !shared_mods.join("instance.json").exists()
                && !shared_mods.join("celestial.json").exists(),
            "backfill must never create a sidecar in a shared content directory"
        );
    }

    /// Regression guard for the ordering bug: a sidecar without `created` must
    /// resolve to a stable directory-derived timestamp, never `Utc::now()`,
    /// otherwise every scan makes the instance look freshly created and it
    /// outranks genuinely recent instances.
    #[test]
    fn missing_created_resolves_to_stable_directory_time() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("versions").join("1.21.1");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.to_string_lossy().to_string();

        let json = InstanceJson::default();
        assert!(json.created.is_none());

        let first = json.to_instance_with_format(&path, InstanceFormat::Minecraft);
        std::thread::sleep(std::time::Duration::from_millis(30));
        let second = json.to_instance_with_format(&path, InstanceFormat::Minecraft);

        assert_eq!(
            first.created, second.created,
            "created must be stable across scans"
        );
        assert_eq!(
            first.created,
            dir_created_time(&dir).unwrap(),
            "created must come from the instance directory"
        );
    }
}
