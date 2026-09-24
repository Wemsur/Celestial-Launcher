//! Plugin management: discovery, installation, the permission record, and the
//! capabilities a plugin can actually use.
//!
//! The model is deliberately split up:
//!
//! - [`manifest`] is untrusted third-party input, validated before use.
//! - [`permissions`] is the vocabulary a manifest may ask for, and how risky
//!   each entry is.
//! - [`store`] owns the on-disk layout, the registry, and the operations the
//!   plugin page drives. [`store::ensure_granted`] is the gate every capability
//!   goes through.
//! - [`data`] is what a plugin may do for itself: its own key/value storage,
//!   and the crash log that switches it off when it throws.
//!
//! Nothing here loads or runs plugin code — that is the loader's job, and it
//! only ever acts on a [`PluginSummary`] this module produced.

pub mod data;
pub mod manifest;
pub mod network;
pub mod permissions;
pub mod store;

pub use data::{
    crash_log, record_crash, settings_get_all, settings_set, storage_get,
    storage_keys, storage_remove, storage_set,
};
pub use manifest::{
    CURRENT_API_VERSION, MANIFEST_FILE, PluginManifest, PluginSettingField,
    PluginSettingOption, PluginSettingType, PluginType, is_valid_plugin_id,
};
pub use network::{PluginFetchRequest, PluginFetchResponse, fetch};
pub use permissions::{PermissionKind, PermissionRisk, PluginPermission};
pub use store::{
    PluginSummary, ensure_granted, get, get_hot_reload, grant_permission,
    install, install_from_url, list, plugin_data_dir, plugins_dir, read_entry,
    resolve_entry, revoke_permission, set_enabled, set_granted, set_hot_reload,
    uninstall,
};
