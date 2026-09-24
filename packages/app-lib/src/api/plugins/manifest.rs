//! The `manifest.json` every plugin ships, and the rules for accepting one.
//!
//! A manifest is untrusted input from a third party, so it is validated before
//! anything acts on it: the ID has to be usable as a directory name, the entry
//! point has to stay inside the plugin folder, and every declared permission
//! has to parse. A manifest that fails validation is reported to the plugin
//! page rather than partially honoured.

use serde::{Deserialize, Serialize};
use std::path::{Component, Path};

use super::permissions::PluginPermission;

pub const MANIFEST_FILE: &str = "manifest.json";

/// A plugin's own folder is third-party input. A manifest big enough to stall
/// the scan is not a manifest, it is a denial of service.
const MAX_MANIFEST_BYTES: u64 = 256 * 1024;

/// Highest `api_version` this launcher can run. A plugin written against a
/// newer API is refused outright: loading it and failing later would surface as
/// mysterious breakage instead of "this plugin needs a newer launcher".
pub const CURRENT_API_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginType {
    #[default]
    Ui,
    Sidecar,
}

/// One declarative setting the launcher renders and stores for a plugin.
///
/// Kept intentionally small: the point is to spare the common plugin from
/// writing its own settings page. A plugin that needs more builds its own UI
/// with the `route` capability and reads/writes `storage` directly.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginSettingField {
    /// Storage key the value is saved under. Same namespace as `api.storage`,
    /// so a plugin can read a declared setting with `api.storage.get(key)`.
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
    pub r#type: PluginSettingType,
    /// Default serialised to a string, matching how storage holds everything.
    #[serde(default)]
    pub default: Option<String>,
    /// Options for `select`; ignored otherwise.
    #[serde(default)]
    pub options: Vec<PluginSettingOption>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginSettingType {
    Toggle,
    Text,
    Number,
    Select,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginSettingOption {
    pub value: String,
    #[serde(default)]
    pub label: Option<String>,
}

impl PluginType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ui => "ui",
            Self::Sidecar => "sidecar",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Reverse-DNS identifier. Doubles as the plugin's folder name, so it is
    /// restricted to characters that are safe in a path on every platform.
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub version: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub r#type: PluginType,
    #[serde(default = "current_api_version")]
    pub api_version: u32,
    /// UI plugins only: the JavaScript bundle to load, relative to the plugin
    /// folder.
    #[serde(default)]
    pub entry: Option<String>,
    /// Requested permissions, as `kind` or `kind:scope` strings.
    #[serde(default)]
    pub permissions: Vec<String>,
    /// Declarative settings the launcher renders into a form for this plugin.
    /// The plugin reads the values back through `api.settings`.
    #[serde(default)]
    pub settings: Vec<PluginSettingField>,
    /// Sidecar plugins only. Parsed but not yet acted on — the sidecar runtime
    /// lands later, and keeping the field here means a manifest written for it
    /// still round-trips through the model.
    #[serde(default)]
    pub sidecar: Option<serde_json::Value>,
}

fn current_api_version() -> u32 {
    CURRENT_API_VERSION
}

impl PluginManifest {
    pub fn read_from_dir(dir: &Path) -> crate::Result<Self> {
        let path = dir.join(MANIFEST_FILE);
        let unreadable = |error: std::io::Error| {
            crate::ErrorKind::InputError(format!(
                "Could not read {MANIFEST_FILE}: {error}"
            ))
        };

        let size = std::fs::metadata(&path)
            .map_err(unreadable)
            .map(|metadata| metadata.len())?;
        if size > MAX_MANIFEST_BYTES {
            return Err(crate::ErrorKind::InputError(format!(
                "{MANIFEST_FILE} is {size} bytes, which is over the \
                 {MAX_MANIFEST_BYTES} byte limit"
            ))
            .into());
        }

        let bytes = std::fs::read(&path).map_err(unreadable)?;
        Self::parse(&bytes)
    }

    pub fn parse(bytes: &[u8]) -> crate::Result<Self> {
        let manifest: Self = serde_json::from_slice(bytes)?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> crate::Result<()> {
        if !is_valid_plugin_id(&self.id) {
            return Err(crate::ErrorKind::InputError(format!(
                "Invalid plugin id '{}': use letters, digits, '.', '-' or '_' \
                 (max 128 characters, no leading or trailing '.')",
                self.id
            ))
            .into());
        }
        if self.name.trim().is_empty() {
            return Err(crate::ErrorKind::InputError(format!(
                "Plugin '{}' has an empty name",
                self.id
            ))
            .into());
        }
        if self.version.trim().is_empty() {
            return Err(crate::ErrorKind::InputError(format!(
                "Plugin '{}' has an empty version",
                self.id
            ))
            .into());
        }
        if self.api_version == 0 || self.api_version > CURRENT_API_VERSION {
            return Err(crate::ErrorKind::InputError(format!(
                "Plugin '{}' needs plugin API version {}, but this launcher \
                 supports up to {CURRENT_API_VERSION}",
                self.id, self.api_version
            ))
            .into());
        }

        for raw in &self.permissions {
            PluginPermission::parse(raw).map_err(|reason| {
                crate::ErrorKind::InputError(format!(
                    "Plugin '{}' declares an invalid permission '{raw}': \
                     {reason}",
                    self.id
                ))
            })?;
        }

        for field in &self.settings {
            if field.key.trim().is_empty() {
                return Err(crate::ErrorKind::InputError(format!(
                    "Plugin '{}' has a setting with an empty key",
                    self.id
                ))
                .into());
            }
            if field.r#type == PluginSettingType::Select
                && field.options.is_empty()
            {
                return Err(crate::ErrorKind::InputError(format!(
                    "Plugin '{}' setting '{}' is a select but has no options",
                    self.id, field.key
                ))
                .into());
            }
        }

        match self.r#type {
            PluginType::Ui => {
                let entry = self.entry.as_deref().ok_or_else(|| {
                    crate::ErrorKind::InputError(format!(
                        "Plugin '{}' is a UI plugin but declares no entry",
                        self.id
                    ))
                })?;
                validate_entry(entry).map_err(|reason| {
                    crate::ErrorKind::InputError(format!(
                        "Plugin '{}' has an invalid entry '{entry}': {reason}",
                        self.id
                    ))
                })?;
            }
            PluginType::Sidecar => {
                if self.sidecar.is_none() {
                    return Err(crate::ErrorKind::InputError(format!(
                        "Plugin '{}' is a sidecar plugin but declares no \
                         sidecar section",
                        self.id
                    ))
                    .into());
                }
            }
        }

        Ok(())
    }

    pub fn parsed_permissions(&self) -> crate::Result<Vec<PluginPermission>> {
        self.permissions
            .iter()
            .map(|raw| {
                PluginPermission::parse(raw).map_err(|reason| {
                    crate::ErrorKind::InputError(format!(
                        "Plugin '{}' declares an invalid permission '{raw}': \
                         {reason}",
                        self.id
                    ))
                    .into()
                })
            })
            .collect()
    }
}

/// Whether `id` can be used both as a manifest identifier and as the plugin's
/// folder name.
///
/// `..` is the important one: an ID like `../../foo` would otherwise walk out
/// of the plugins directory when joined onto it.
pub fn is_valid_plugin_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && !id.starts_with('.')
        && !id.ends_with('.')
        && !id.contains("..")
        && id.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || matches!(character, '.' | '-' | '_')
        })
}

/// An entry point must be a plain relative path inside the plugin folder —
/// no absolute paths, no `..`, no drive prefixes.
fn validate_entry(entry: &str) -> Result<(), String> {
    if entry.trim().is_empty() {
        return Err("entry must not be empty".to_string());
    }
    let path = Path::new(entry);
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("entry must be a relative path inside the plugin folder"
            .to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ui_manifest(json: &str) -> crate::Result<PluginManifest> {
        PluginManifest::parse(json.as_bytes())
    }

    fn minimal() -> String {
        r#"{
            "id": "com.example.demo",
            "name": "Demo",
            "version": "1.0.0",
            "entry": "index.js"
        }"#
        .to_string()
    }

    #[test]
    fn minimal_ui_manifest_parses() {
        let manifest = ui_manifest(&minimal()).unwrap();
        assert_eq!(manifest.id, "com.example.demo");
        assert_eq!(manifest.r#type, PluginType::Ui);
        assert_eq!(manifest.api_version, CURRENT_API_VERSION);
    }

    #[test]
    fn ui_manifest_without_an_entry_is_rejected() {
        let json = r#"{"id":"a","name":"A","version":"1.0.0"}"#;
        assert!(ui_manifest(json).is_err());
    }

    #[test]
    fn entry_may_not_escape_the_plugin_folder() {
        for entry in ["../evil.js", "/etc/passwd", "a/../../b.js"] {
            let json = format!(
                r#"{{"id":"a","name":"A","version":"1.0.0","entry":"{entry}"}}"#
            );
            assert!(ui_manifest(&json).is_err(), "entry {entry} was accepted");
        }
    }

    #[test]
    fn nested_entry_inside_the_folder_is_allowed() {
        let json = r#"{"id":"a","name":"A","version":"1.0.0",
            "entry":"dist/index.js"}"#;
        assert!(ui_manifest(json).is_ok());
    }

    #[test]
    fn ids_that_could_escape_the_plugins_directory_are_rejected() {
        for id in ["..", "../evil", "a/../b", ".hidden", "trailing."] {
            assert!(!is_valid_plugin_id(id), "id {id} was accepted");
        }
        for id in ["com.example.demo", "seed_viewer", "a-b.c"] {
            assert!(is_valid_plugin_id(id), "id {id} was rejected");
        }
    }

    #[test]
    fn a_newer_api_version_is_refused() {
        let json = format!(
            r#"{{"id":"a","name":"A","version":"1.0.0","entry":"i.js",
                "api_version":{}}}"#,
            CURRENT_API_VERSION + 1
        );
        let error = ui_manifest(&json).unwrap_err().to_string();
        assert!(error.contains("plugin API version"), "{error}");
    }

    #[test]
    fn invalid_permissions_fail_the_whole_manifest() {
        let json = r#"{"id":"a","name":"A","version":"1.0.0","entry":"i.js",
            "permissions":["storage","bogus"]}"#;
        assert!(ui_manifest(json).is_err());
    }

    #[test]
    fn sidecar_manifest_needs_a_sidecar_section() {
        let json = r#"{"id":"a","name":"A","version":"1.0.0",
            "type":"sidecar"}"#;
        assert!(ui_manifest(json).is_err());

        let json = r#"{"id":"a","name":"A","version":"1.0.0",
            "type":"sidecar","sidecar":{"binaries":{}}}"#;
        assert!(ui_manifest(json).is_ok());
    }

    #[test]
    fn declared_permissions_parse_back_out() {
        let json = r#"{"id":"a","name":"A","version":"1.0.0","entry":"i.js",
            "permissions":["storage","style","slot:sidebar.bottom"]}"#;
        let manifest = ui_manifest(json).unwrap();
        let parsed = manifest.parsed_permissions().unwrap();
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[2].to_string(), "slot:sidebar.bottom");
    }
}
