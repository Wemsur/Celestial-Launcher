use crate::state::{
    InstanceInstallStage, LauncherFeatureVersion, ReleaseChannel,
    libraries::InstanceFormat,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Instance {
    pub id: String,
    pub path: String,
    pub applied_content_set_id: Option<String>,
    pub install_stage: InstanceInstallStage,
    pub launcher_feature_version: LauncherFeatureVersion,
    pub update_channel: ReleaseChannel,
    pub name: String,
    pub icon_path: Option<String>,
    pub created: DateTime<Utc>,
    pub modified: DateTime<Utc>,
    pub last_played: Option<DateTime<Utc>>,
    pub submitted_time_played: u64,
    pub recent_time_played: u64,
    #[serde(default)]
    pub library_format: InstanceFormat,
}

impl Instance {
    pub fn is_json_backed(&self) -> bool {
        self.applied_content_set_id.is_none()
    }
}

pub(crate) fn playtime_to_storage(
    value: u64,
    column: &str,
) -> crate::Result<i64> {
    i64::try_from(value).map_err(|_| {
        crate::ErrorKind::InputError(format!(
            "Expected {column} to fit in SQLite INTEGER"
        ))
        .into()
    })
}
