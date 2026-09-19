//! Plugin management: discovery, installation, and the permission record.
//!
//! The model is deliberately split in two:
//!
//! - [`manifest`] is untrusted third-party input, validated before use.
//! - [`permissions`] is the vocabulary a manifest may ask for, and how risky
//!   each entry is.
//! - [`store`] owns the on-disk layout, the registry, and the operations the
//!   plugin page drives.
//!
//! Nothing here loads or runs plugin code — that is the loader's job, and it
//! only ever acts on a [`PluginSummary`] this module produced.

pub mod manifest;
pub mod permissions;
pub mod store;

pub use manifest::{
    CURRENT_API_VERSION, MANIFEST_FILE, PluginManifest, PluginType,
    is_valid_plugin_id,
};
pub use permissions::{PermissionKind, PermissionRisk, PluginPermission};
pub use store::{
    PluginSummary, get, grant_permission, install, list, plugin_data_dir,
    plugins_dir, resolve_entry, revoke_permission, set_enabled, set_granted,
    uninstall,
};
