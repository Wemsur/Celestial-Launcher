use crate::state::ModLoader;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InstanceInstallTarget {
    pub game_version: String,
    pub loader: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct InstanceInstallCandidate {
    pub id: String,
    pub name: String,
    pub icon_path: Option<String>,
    pub game_version: String,
    pub loader: ModLoader,
    pub installed: bool,
    pub compatible: bool,
    /// Path of the registered library this instance lives in, spelled as
    /// `libraries.json` spells it, so the install picker can filter by library.
    /// `None` for an instance outside every registered library (a legacy DB row
    /// with a stale path), which simply never matches a library tab.
    pub library_path: Option<String>,
}
