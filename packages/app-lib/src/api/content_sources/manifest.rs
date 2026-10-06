//! The `manifest.json` a content-source plugin ships, and the rules for
//! accepting one.
//!
//! A content source is its own kind of plugin, deliberately unlike a launcher
//! UI plugin: it contributes no UI, holds no slots or routes, and its whole job
//! is to answer questions about content — what exists, what a project looks
//! like, and how to fetch a file. It therefore has its own manifest rather than
//! borrowing the UI plugin's, so neither system's vocabulary leaks into the
//! other.
//!
//! A manifest is untrusted third-party input, so it is validated before
//! anything acts on it: the id has to be safe as a directory name, the entry
//! point has to stay inside the source folder, and every declared content type
//! and capability has to be known.

use serde::{Deserialize, Serialize};
use std::path::{Component, Path};

pub const MANIFEST_FILE: &str = "manifest.json";

/// A manifest big enough to stall the scan is a denial of service, not a
/// manifest.
const MAX_MANIFEST_BYTES: u64 = 256 * 1024;

/// Highest `api_version` this launcher can run. A source written against a
/// newer API is refused outright: loading it and failing later would surface as
/// mysterious breakage instead of "this source needs a newer launcher".
pub const CURRENT_API_VERSION: u32 = 1;

/// The launcher's closed set of content types. A source may only serve these;
/// how each one is installed is the launcher's business, not the source's.
///
/// The wire names are spelled out rather than derived: `rename_all` would turn
/// `ResourcePack` into `resource_pack` and `DataPack` into `data_pack`, while
/// the launcher's own vocabulary — and therefore every plugin manifest — uses
/// `resourcepack` and `datapack`. A source declaring the former names would fail
/// to load with a serde error that says nothing about the real problem.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ContentType {
    #[serde(rename = "mod")]
    Mod,
    #[serde(rename = "modpack")]
    Modpack,
    #[serde(rename = "resourcepack")]
    ResourcePack,
    #[serde(rename = "shader")]
    Shader,
    #[serde(rename = "datapack")]
    DataPack,
    #[serde(rename = "world")]
    World,
    #[serde(rename = "skin")]
    Skin,
}

impl ContentType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mod => "mod",
            Self::Modpack => "modpack",
            Self::ResourcePack => "resourcepack",
            Self::Shader => "shader",
            Self::DataPack => "datapack",
            Self::World => "world",
            Self::Skin => "skin",
        }
    }
}

/// A sort key the launcher may ask for. A source that cannot supply the number
/// behind one ranks its items last for that sort rather than refusing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentSort {
    Relevance,
    Downloads,
    Follows,
    Newest,
    Updated,
}

/// What a source can actually do. Anything `false` degrades the UI rather than
/// breaking it: a source that cannot `resolve_download` still browses, and its
/// install button opens the project page instead of fetching a file.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct SourceCapabilities {
    #[serde(default)]
    pub search: bool,
    #[serde(default)]
    pub browse: bool,
    #[serde(default)]
    pub detail: bool,
    #[serde(default)]
    pub versions: bool,
    #[serde(default)]
    pub resolve_download: bool,
    #[serde(default)]
    pub update_check: bool,
}

/// One field of the form a user fills in to add a project by hand, for a source
/// with no browsing of its own (a GitHub repo, say).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ManualEntryField {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub placeholder: Option<String>,
    /// A regex the value must match; the form shows it as a hint when the value
    /// does not match.
    #[serde(default)]
    pub pattern: Option<String>,
    /// Present when the field is a choice rather than free text; the form then
    /// renders a dropdown. Used for the content type an entry belongs to, so one
    /// entry does not show up under every tab the source serves.
    #[serde(default)]
    pub options: Vec<SourceSettingOption>,
    #[serde(default)]
    pub required: bool,
}

/// Set when a source needs the user to sign in before it works. The launcher
/// opens a dedicated window at `login_url`; the source decides how to tell that
/// sign-in finished and then holds the resulting cookie itself.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceAuth {
    pub required: bool,
    #[serde(default)]
    pub login_url: Option<String>,
    #[serde(default)]
    pub help_url: Option<String>,
}

/// A setting the launcher renders as a form and stores for the source. Kept
/// intentionally small; a source that needs more keeps its own state in storage.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceSettingField {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
    pub r#type: SourceSettingType,
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub options: Vec<SourceSettingOption>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceSettingType {
    Toggle,
    Text,
    Number,
    Select,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceSettingOption {
    pub value: String,
    #[serde(default)]
    pub label: Option<String>,
}

/// How a source's project body is presented.
///
/// The launcher renders the body itself rather than letting a source ship HTML,
/// so a source says which of the two it means and no markup from a manifest ever
/// reaches the page unparsed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceDetailFormat {
    /// Render as Markdown, sanitised — what a README or a site's `content_md`
    /// wants. The default, because that is what content sites publish.
    #[default]
    Markdown,
    /// Show verbatim, formatting characters and all.
    Text,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContentSourceManifest {
    /// Identifier. Doubles as the source's folder name, so it is restricted to
    /// characters that are safe in a path on every platform.
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    /// An image in the source's own folder to show as its icon, e.g.
    /// `icon.svg`. Anything the webview can display works (svg, png, webp). A
    /// source without one is drawn as its name's first character instead.
    #[serde(default)]
    pub icon: Option<String>,
    /// The source's theme colour as `#rrggbb`, used for that fallback icon and
    /// to tint the source's cards. Absent means no tint.
    #[serde(default)]
    pub color: Option<String>,
    /// How the `detail` body is presented. Defaults to Markdown.
    #[serde(default)]
    pub detail_format: SourceDetailFormat,
    #[serde(default = "current_api_version")]
    pub api_version: u32,
    /// The JavaScript bundle to load, relative to the source folder.
    pub entry: String,
    /// Content types this source serves. A request for another type is refused.
    pub content_types: Vec<ContentType>,
    #[serde(default)]
    pub capabilities: SourceCapabilities,
    /// Hosts this source may reach, as `example.com` or `*.example.com`. The
    /// network proxy refuses any host not listed here.
    #[serde(default)]
    pub hosts: Vec<String>,
    #[serde(default)]
    pub auth: Option<SourceAuth>,
    #[serde(default)]
    pub manual_entry: Vec<ManualEntryField>,
    #[serde(default)]
    pub settings: Vec<SourceSettingField>,
}

fn current_api_version() -> u32 {
    CURRENT_API_VERSION
}

impl ContentSourceManifest {
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
        // A bare serde message ("unknown variant `resourcepack`") says nothing
        // about which file it came from, and a source author reads this error in
        // the source manager, not in a debugger.
        let manifest: Self = serde_json::from_slice(bytes).map_err(|error| {
            crate::ErrorKind::InputError(format!(
                "{MANIFEST_FILE} is not valid: {error}"
            ))
        })?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> crate::Result<()> {
        if !is_valid_source_id(&self.id) {
            return Err(crate::ErrorKind::InputError(format!(
                "Invalid content source id '{}': use letters, digits, '.', \
                 '-' or '_' (max 128 characters, no leading or trailing '.')",
                self.id
            ))
            .into());
        }
        if self.name.trim().is_empty() {
            return Err(crate::ErrorKind::InputError(format!(
                "Content source '{}' has an empty name",
                self.id
            ))
            .into());
        }
        if self.version.trim().is_empty() {
            return Err(crate::ErrorKind::InputError(format!(
                "Content source '{}' has an empty version",
                self.id
            ))
            .into());
        }
        if self.api_version == 0 || self.api_version > CURRENT_API_VERSION {
            return Err(crate::ErrorKind::InputError(format!(
                "Content source '{}' needs content-source API version {}, but \
                 this launcher supports up to {CURRENT_API_VERSION}",
                self.id, self.api_version
            ))
            .into());
        }
        if let Err(reason) = validate_entry(&self.entry) {
            return Err(crate::ErrorKind::InputError(format!(
                "Content source '{}' has an invalid entry point: {reason}",
                self.id
            ))
            .into());
        }
        if let Some(icon) = &self.icon
            && let Err(reason) = validate_icon(icon)
        {
            return Err(crate::ErrorKind::InputError(format!(
                "Content source '{}' has an invalid icon: {reason}",
                self.id
            ))
            .into());
        }
        if let Some(color) = &self.color
            && !is_valid_hex_color(color)
        {
            return Err(crate::ErrorKind::InputError(format!(
                "Content source '{}' has an invalid color '{color}': expected \
                 '#rrggbb'",
                self.id
            ))
            .into());
        }
        if self.content_types.is_empty() {
            return Err(crate::ErrorKind::InputError(format!(
                "Content source '{}' declares no content types",
                self.id
            ))
            .into());
        }

        for host in &self.hosts {
            if !is_valid_host_pattern(host) {
                return Err(crate::ErrorKind::InputError(format!(
                    "Content source '{}' declares an invalid host '{host}'",
                    self.id
                ))
                .into());
            }
        }

        for field in &self.manual_entry {
            if field.key.trim().is_empty() {
                return Err(crate::ErrorKind::InputError(format!(
                    "Content source '{}' has a manual-entry field with an \
                     empty key",
                    self.id
                ))
                .into());
            }
        }

        for field in &self.settings {
            if field.key.trim().is_empty() {
                return Err(crate::ErrorKind::InputError(format!(
                    "Content source '{}' has a setting with an empty key",
                    self.id
                ))
                .into());
            }
            if field.r#type == SourceSettingType::Select
                && field.options.is_empty()
            {
                return Err(crate::ErrorKind::InputError(format!(
                    "Content source '{}' has a select setting '{}' with no \
                     options",
                    self.id, field.key
                ))
                .into());
            }
        }

        Ok(())
    }
}

/// Whether `id` is safe to use as a direct child directory name.
pub fn is_valid_source_id(id: &str) -> bool {
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

/// A host pattern is a bare host or `*.` plus one label; ports are rejected
/// because the proxy matches on host alone.
pub fn is_valid_host_pattern(pattern: &str) -> bool {
    let host = pattern.strip_prefix("*.").unwrap_or(pattern);
    !host.is_empty()
        && host.len() <= 253
        && !host.contains('*')
        && !host.contains('/')
        && !host.contains(':')
        && host.split('.').all(|label| {
            !label.is_empty()
                && label
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
}

/// An entry point must be a plain relative path inside the source folder — no
/// absolute paths, no `..`, no drive prefixes.
fn validate_entry(entry: &str) -> Result<(), String> {
    if entry.trim().is_empty() {
        return Err("entry must not be empty".to_string());
    }
    let path = Path::new(entry);
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(
            "entry must be a relative path inside the source folder"
                .to_string(),
        );
    }
    Ok(())
}

/// The icon is read out of the source's folder, so it carries the same
/// restriction the entry point does: a plain relative path, nothing that could
/// name a file elsewhere on the disk.
fn validate_icon(icon: &str) -> Result<(), String> {
    if icon.trim().is_empty() {
        return Err("icon must not be empty".to_string());
    }
    if Path::new(icon)
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(
            "icon must be a relative path inside the source folder".to_string(),
        );
    }
    Ok(())
}

/// `#rrggbb`, the only colour form the launcher reads. Accepting names or
/// `rgb()` would mean shipping a colour parser for no gain.
fn is_valid_hex_color(value: &str) -> bool {
    let Some(hex) = value.strip_prefix('#') else {
        return false;
    };
    hex.len() == 6 && hex.chars().all(|character| character.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_minimal_manifest_parses() {
        let json = r#"{
            "id": "example",
            "name": "Example",
            "version": "1.0.0",
            "entry": "index.js",
            "content_types": ["mod"]
        }"#;
        let manifest =
            ContentSourceManifest::parse(json.as_bytes()).unwrap();
        assert_eq!(manifest.id, "example");
        assert_eq!(manifest.content_types, vec![ContentType::Mod]);
        assert_eq!(manifest.api_version, CURRENT_API_VERSION);
        assert!(!manifest.capabilities.search);
    }

    #[test]
    fn ids_that_could_escape_the_folder_are_rejected() {
        assert!(is_valid_source_id("pixelmap"));
        assert!(is_valid_source_id("github.repos"));
        assert!(!is_valid_source_id(".."));
        assert!(!is_valid_source_id(".hidden"));
        assert!(!is_valid_source_id("a/b"));
        assert!(!is_valid_source_id(""));
    }

    #[test]
    fn host_patterns_are_validated() {
        assert!(is_valid_host_pattern("example.com"));
        assert!(is_valid_host_pattern("*.example.com"));
        assert!(!is_valid_host_pattern("*.example.*"));
        assert!(!is_valid_host_pattern("example.com:8080"));
        assert!(!is_valid_host_pattern("http://example.com"));
        assert!(!is_valid_host_pattern(""));
    }

    #[test]
    fn an_entry_that_escapes_the_folder_is_rejected() {
        assert!(validate_entry("index.js").is_ok());
        assert!(validate_entry("../evil.js").is_err());
        assert!(validate_entry("/etc/passwd").is_err());
        assert!(validate_entry("").is_err());
    }

    #[test]
    fn no_content_types_is_rejected() {
        let json = r#"{
            "id": "example",
            "name": "Example",
            "version": "1.0.0",
            "entry": "index.js",
            "content_types": []
        }"#;
        assert!(ContentSourceManifest::parse(json.as_bytes()).is_err());
    }

    #[test]
    fn content_type_wire_names_match_the_launcher_vocabulary() {
        for (json, expected) in [
            ("\"mod\"", ContentType::Mod),
            ("\"modpack\"", ContentType::Modpack),
            ("\"resourcepack\"", ContentType::ResourcePack),
            ("\"shader\"", ContentType::Shader),
            ("\"datapack\"", ContentType::DataPack),
            ("\"world\"", ContentType::World),
            ("\"skin\"", ContentType::Skin),
        ] {
            let parsed: ContentType = serde_json::from_str(json).unwrap();
            assert_eq!(parsed, expected, "{json} should parse");
            assert_eq!(
                serde_json::to_string(&expected).unwrap(),
                json,
                "round trip"
            );
        }
    }

    /// A derived `rename_all` spells these `resource_pack` and `data_pack`,
    /// which no manifest writes; the derivation would reject every source that
    /// serves them, with an error that names neither the field nor the cause.
    #[test]
    fn the_derived_snake_case_names_are_not_accepted() {
        assert!(serde_json::from_str::<ContentType>("\"resource_pack\"").is_err());
        assert!(serde_json::from_str::<ContentType>("\"data_pack\"").is_err());
    }

    #[test]
    fn a_manifest_serving_every_loose_type_parses() {
        let json = r#"{
            "id": "example",
            "name": "Example",
            "version": "1.0.0",
            "entry": "index.js",
            "content_types": ["mod", "resourcepack", "shader", "datapack", "world"]
        }"#;
        let manifest =
            ContentSourceManifest::parse(json.as_bytes()).unwrap();
        assert_eq!(manifest.content_types.len(), 5);
    }

    #[test]
    fn appearance_is_optional_and_defaults_sensibly() {
        let json = r#"{
            "id": "example",
            "name": "Example",
            "version": "1.0.0",
            "entry": "index.js",
            "content_types": ["world"]
        }"#;
        let manifest =
            ContentSourceManifest::parse(json.as_bytes()).unwrap();
        assert!(manifest.icon.is_none());
        assert!(manifest.color.is_none());
        assert_eq!(manifest.detail_format, SourceDetailFormat::Markdown);
    }

    #[test]
    fn appearance_is_read_when_present() {
        // `r##"…"##`: the JSON contains `"#`, which would close an `r#"…"#`.
        let json = r##"{
            "id": "example",
            "name": "Example",
            "version": "1.0.0",
            "entry": "index.js",
            "content_types": ["world"],
            "icon": "icon.svg",
            "color": "#8B5A2B",
            "detail_format": "text"
        }"##;
        let manifest =
            ContentSourceManifest::parse(json.as_bytes()).unwrap();
        assert_eq!(manifest.icon.as_deref(), Some("icon.svg"));
        assert_eq!(manifest.color.as_deref(), Some("#8B5A2B"));
        assert_eq!(manifest.detail_format, SourceDetailFormat::Text);
    }

    #[test]
    fn a_loose_detail_format_is_rejected() {
        let json = r#"{
            "id": "example",
            "name": "Example",
            "version": "1.0.0",
            "entry": "index.js",
            "content_types": ["world"],
            "detail_format": "html"
        }"#;
        assert!(ContentSourceManifest::parse(json.as_bytes()).is_err());
    }

    #[test]
    fn an_icon_that_escapes_the_folder_is_rejected() {
        assert!(validate_icon("icon.svg").is_ok());
        assert!(validate_icon("assets/icon.png").is_ok());
        assert!(validate_icon("../icon.svg").is_err());
        assert!(validate_icon("/etc/passwd").is_err());
        assert!(validate_icon("").is_err());
    }

    #[test]
    fn colors_must_be_hex() {
        assert!(is_valid_hex_color("#1bd96a"));
        assert!(is_valid_hex_color("#8B5A2B"));
        assert!(!is_valid_hex_color("1bd96a"));
        assert!(!is_valid_hex_color("#1bd96"));
        assert!(!is_valid_hex_color("#1bd96g"));
        assert!(!is_valid_hex_color("green"));
        assert!(!is_valid_hex_color(""));
    }
}
