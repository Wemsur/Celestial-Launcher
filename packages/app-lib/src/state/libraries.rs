use crate::state::instances::adapters::sqlite::instance_rows;
use crate::state::{ContentSet, Instance, InstanceLink, State};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::debug;

pub const LIBRARIES_FILE_NAME: &str = "libraries.json";

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

impl From<&str> for InstanceFormat {
    fn from(s: &str) -> Self {
        match s {
            "minecraft" => Self::Minecraft,
            "modrinth" => Self::Modrinth,
            _ => Self::default(),
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
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct InstanceJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_played: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub game_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loader: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loader_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<InstanceLink>,
    #[serde(default)]
    pub submitted_time_played: u64,
    #[serde(default)]
    pub recent_time_played: u64,
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
            last_played: instance.last_played,
            game_version: Some(content_set.game_version.clone()),
            loader: Some(content_set.loader.as_str().to_string()),
            loader_version: content_set.loader_version.clone(),
            link,
            submitted_time_played: instance.submitted_time_played,
            recent_time_played: instance.recent_time_played,
        }
    }

    pub(crate) fn from_instance(instance: &Instance) -> Self {
        Self {
            name: Some(instance.name.clone()),
            icon_path: instance.icon_path.clone(),
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
        let id = instance_id_from_path(absolute_path);
        // Prefer stored name, then fall back to directory name
        let name = self
            .name
            .clone()
            .or_else(|| Path::new(absolute_path).file_name().and_then(|n| n.to_str()).map(|s| s.to_string()));
        Instance {
            id: id.clone(),
            path: absolute_path.to_string(),
            applied_content_set_id: None,
            install_stage: crate::state::InstanceInstallStage::Installed,
            launcher_feature_version:
                crate::state::LauncherFeatureVersion::MOST_RECENT,
            update_channel: crate::state::ReleaseChannel::Release,
            name: name.unwrap_or_default(),
            icon_path: self.icon_path.clone(),
            created: Utc::now(),
            modified: Utc::now(),
            last_played: self.last_played,
            submitted_time_played: self.submitted_time_played,
            recent_time_played: self.recent_time_played,
        }
    }
}

pub fn instance_id_from_path(path: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(path.as_bytes());
    let result = hasher.finalize();
    let hex: String = result
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("local:{}", &hex[..32])
}

pub async fn get_libraries_config(state: &State) -> crate::Result<LibrariesConfig> {
    let path = state.directories.settings_dir.join(LIBRARIES_FILE_NAME);
    if !path.exists() {
        return Ok(LibrariesConfig {
            libraries: vec![],
            migrated: false,
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

        let entries = match fs::read_dir(lib_path) {
            Ok(e) => e,
            Err(_) => continue,
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
                continue;
            };

            let instance = instance_json.to_instance(dir.to_string_lossy().as_ref());
            instances.push(instance);
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

        let format = if lib_path.contains(".minecraft") {
            InstanceFormat::Minecraft
        } else {
            InstanceFormat::Modrinth
        };

        lib_map.entry(lib_path.clone()).or_insert_with(|| LibraryInfo {
            name: String::new(),
            path: lib_path,
            format: format.clone(),
        });

        // Only write instance.json if the directory actually exists.
        // We no longer persist derived fields (version, loader) — they are
        // detected from the filesystem on each scan.
        if fallback_path.exists() {
            let instance_json = InstanceJson::from_instance_and_content_set(
                instance,
                &record.applied_content_set,
                &absolute_path_str,
                Some(record.link.clone()),
            );
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
    };

    save_libraries_config(state, &config).await?;

    // Second pass: fix missing `link` in existing instance.json files only.
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
                    if let Err(e) = json.write_to_dir(&dir) {
                        tracing::warn!(
                            "Failed to update instance.json for {}: {e}",
                            record.instance.id
                        );
                    }
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
    let config = get_libraries_config(state).await?;
    if !config.migrated {
        migrate_instances_from_db(state).await?;
    }
    Ok(())
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

/// Find a JSON-backed instance by ID. Returns the path used to locate it.
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
        let entries = match fs::read_dir(lib_path) {
            Ok(e) => e,
            Err(_) => continue,
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
                continue;
            };
            let instance = instance_json.to_instance(dir.to_string_lossy().as_ref());
            if instance.id == instance_id {
                return Ok(Some(resolve_instance_dir(state, &instance.path)));
            }
        }
    }
    Ok(None)
}

/// Detect the Minecraft game version from the instance directory by scanning
/// for a version manifest JSON (e.g. `versions/1.20.1/1.20.1.json`).
/// If no `versions/` subdirectory exists (common for Modrinth-style profiles
/// that share a global `.minecraft/versions/`), fall back to using the
/// instance directory name as a version guess.
pub fn detect_game_version_from_dir(dir: &Path) -> Option<String> {
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
    // No versions/ subdirectory — this is common for Modrinth profiles
    // which share a global versions cache. Fall back to the directory name,
    // which is typically the Minecraft version (e.g. "1.21.8").
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

/// Detect the loader from the instance directory by scanning jars in `mods/`.
/// Returns the first loader detected, or Vanilla as fallback.
pub fn detect_loader_from_dir(dir: &Path) -> crate::state::ModLoader {
    let mods_dir = dir.join("mods");
    if !mods_dir.exists() {
        return crate::state::ModLoader::Vanilla;
    }
    let Ok(entries) = fs::read_dir(&mods_dir) else {
        return crate::state::ModLoader::Vanilla;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(file_name) =
            path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if let Some(loader) = detect_modloader_from_file_name(file_name) {
            debug!(
                "Detected loader {} from mod file: {}",
                loader.as_str(),
                file_name
            );
            return loader;
        }
    }
    crate::state::ModLoader::Vanilla
}

/// Detect a ModLoader from a single file name (usually a mod jar).
fn detect_modloader_from_file_name(name: &str) -> Option<crate::state::ModLoader> {
    let lower = name.to_lowercase();
    if lower.contains("fabric") {
        return Some(crate::state::ModLoader::Fabric);
    }
    if lower.contains("quilt") {
        return Some(crate::state::ModLoader::Quilt);
    }
    if lower.contains("neoforge") {
        return Some(crate::state::ModLoader::NeoForge);
    }
    if lower.contains("forge") {
        return Some(crate::state::ModLoader::Forge);
    }
    None
}
