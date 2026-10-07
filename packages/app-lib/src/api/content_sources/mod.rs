//! Content sources: the plugins that add browsable download sources to
//! Discover.
//!
//! A content source is its own kind of plugin, not a launcher UI plugin with a
//! different badge. It shows no launcher UI, owns no slots or routes, and its
//! entire contract is answering content questions and producing a fetchable
//! file. It therefore has its own manifest, its own folder tree, its own
//! registry and its own network proxy — so neither system's rules constrain the
//! other.
//!
//! The model is split up the same way the plugin one is:
//!
//! - [`manifest`] is untrusted third-party input, validated before use.
//! - [`store`] owns the on-disk layout, the registry, and the operations the
//!   source manager drives.
//! - [`data`] is what a source may do for itself: its own key/value storage,
//!   its settings values, and the crash log that switches it off when it throws.
//! - [`network`] is the CSP escape hatch, gated on the hosts a manifest declared.
//!
//! Nothing here loads or runs source code — that is the loader's job, and it
//! only ever acts on a [`SourceSummary`] this module produced.

pub mod data;
pub mod install;
pub mod installed;
pub mod manifest;
pub mod network;
pub mod store;

pub use data::{
    crash_log, record_crash, settings_get_all, settings_set, storage_get,
    storage_keys, storage_remove, storage_set,
};
pub use install::{
    DownloadedSkin, FileHash, InstallDescriptor, InstallMetadata, InstalledContent,
    SKINS_DIR, StagedModpack, install_content, list_downloaded_skins, skins_dir,
    stage_modpack,
};
pub use installed::{
    SourceContentRecord, forget_install, list_installed, record_install,
};
pub use manifest::{
    CURRENT_API_VERSION, ContentSort, ContentSourceManifest, ContentType,
    MANIFEST_FILE, ManualEntryField, SourceAuth, SourceCapabilities,
    SourceDetailFormat, SourceSettingField, SourceSettingOption,
    SourceSettingType, is_valid_source_id,
};
pub use network::{SourceFetchRequest, SourceFetchResponse, fetch};
pub use store::{
    SourceRecord, SourceSummary, content_sources_dir, get, install,
    install_from_url, latest_release_tag, list, read_entry, resolve_entry,
    set_enabled, source_data_dir, uninstall, update, update_from_url,
};
