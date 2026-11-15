use super::{CelestialJson, InstanceFormat, InstanceJson};
use crate::state::{
	Instance, InstanceLaunchOverrides, InstanceLink, InstanceSyncedOptions,
	ReleaseChannel,
};
use std::path::Path;

/// A single sidecar snapshot shared by instance metadata and launch preparation.
pub(crate) struct JsonInstanceSettings {
	pub game_version: Option<String>,
	pub loader: Option<String>,
	pub loader_version: Option<String>,
	pub link: InstanceLink,
	pub launch_overrides: InstanceLaunchOverrides,
	pub update_channel: ReleaseChannel,
	pub synced_options: InstanceSyncedOptions,
}

impl JsonInstanceSettings {
	pub(crate) fn empty(instance: &Instance) -> Self {
		Self {
			game_version: None,
			loader: None,
			loader_version: None,
			link: InstanceLink::Unmanaged,
			launch_overrides: InstanceLaunchOverrides::empty(instance.id.clone()),
			update_channel: instance.update_channel,
			synced_options: InstanceSyncedOptions::default(),
		}
	}
}

/// instance.json owns launch settings when present, including explicit inheritance.
/// Only its absence permits a Minecraft instance to use celestial.json instead.
pub(crate) fn read_json_instance_settings(
	dir: &Path,
	instance: &Instance,
) -> crate::Result<JsonInstanceSettings> {
	let json = InstanceJson::read_from_dir(dir)?;
	let celestial = CelestialJson::read_from_dir(dir)?;
	let mut settings = JsonInstanceSettings::empty(instance);
	if let Some(celestial) = &celestial {
		settings.synced_options = celestial.synced_options;
	}

	if let Some(json) = json {
		settings.launch_overrides = json.launch_overrides(&instance.id);
		settings.game_version = json.game_version;
		settings.loader = json.loader;
		settings.loader_version = json.loader_version;
		settings.link = json.link.unwrap_or(InstanceLink::Unmanaged);
		settings.update_channel = json.update_channel;
	} else if instance.library_format == InstanceFormat::Minecraft {
		if let Some(celestial) = celestial {
			settings.link = celestial.link.unwrap_or(InstanceLink::Unmanaged);
			settings.update_channel = celestial.update_channel;
			if let Some(data) = celestial.launch_overrides {
				settings.launch_overrides = data.into_launch_overrides(instance.id.clone());
			}
		}
	}

	Ok(settings)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::state::{
		Hooks, InstanceLaunchOverridesData, MemorySettings, WindowSize,
	};

	fn instance(dir: &Path, format: InstanceFormat) -> Instance {
		InstanceJson::default().to_instance_with_format(
			dir.to_string_lossy().as_ref(),
			format,
		)
	}

	#[test]
	fn primary_sidecar_preserves_every_override_and_uses_current_id() {
		let dir = tempfile::tempdir().unwrap();
		for format in [InstanceFormat::Modrinth, InstanceFormat::Minecraft] {
			let instance = instance(dir.path(), format);
			let mut expected = InstanceLaunchOverrides::empty(instance.id.clone());
			expected.memory = Some(MemorySettings { maximum: 8192 });
			expected.java_path = Some("example-java".into());
			expected.extra_launch_args = Some(vec!["-Dexample=true".into()]);
			expected.custom_env_vars = Some(vec![("EXAMPLE".into(), "value".into())]);
			expected.force_fullscreen = Some(false);
			expected.game_resolution = Some(WindowSize(1280, 720));
			expected.hooks = Hooks {
				pre_launch: Some("example-before".into()),
				wrapper: Some("example-wrapper".into()),
				post_exit: Some("example-after".into()),
			};
			expected.visible_tabs.screenshots = true;
			let mut stored = InstanceLaunchOverridesData::from(&expected);
			stored.instance_id = Some("old-instance-id".into());
			let json = InstanceJson {
				game_version: Some("1.21.1".into()),
				loader: Some("vanilla".into()),
				launch_overrides: Some(stored),
				..InstanceJson::default()
			};
			json.write_to_dir(dir.path()).unwrap();

			let actual = read_json_instance_settings(dir.path(), &instance).unwrap();
			assert_eq!(actual.game_version.as_deref(), Some("1.21.1"));
			assert_eq!(
				serde_json::to_value(actual.launch_overrides).unwrap(),
				serde_json::to_value(expected).unwrap(),
			);
		}
	}

	#[test]
	fn lightweight_sidecar_memory_survives_other_edits_and_can_be_cleared() {
		let dir = tempfile::tempdir().unwrap();
		let instance = instance(dir.path(), InstanceFormat::Minecraft);
		let mut overrides = InstanceLaunchOverridesData::new(instance.id.clone());
		overrides.memory = Some(MemorySettings { maximum: 8192 });
		CelestialJson {
			launch_overrides: Some(overrides),
			..CelestialJson::default()
		}.write_to_dir(dir.path()).unwrap();

		let loaded = read_json_instance_settings(dir.path(), &instance).unwrap();
		assert_eq!(loaded.launch_overrides.memory.unwrap().maximum, 8192);
		let mut json = CelestialJson::read_from_dir(dir.path()).unwrap().unwrap();
		json.launch_overrides.as_mut().unwrap().java_path = Some("other-java".into());
		json.write_to_dir(dir.path()).unwrap();
		let loaded = read_json_instance_settings(dir.path(), &instance).unwrap();
		assert_eq!(loaded.launch_overrides.memory.unwrap().maximum, 8192);

		json.launch_overrides.as_mut().unwrap().memory = None;
		json.write_to_dir(dir.path()).unwrap();
		let loaded = read_json_instance_settings(dir.path(), &instance).unwrap();
		assert_eq!(
			loaded.launch_overrides.memory.unwrap_or(MemorySettings { maximum: 4096 }).maximum,
			4096,
		);
	}

	#[test]
	fn primary_inheritance_does_not_resurrect_lightweight_overrides() {
		let dir = tempfile::tempdir().unwrap();
		let instance = instance(dir.path(), InstanceFormat::Minecraft);
		let mut overrides = InstanceLaunchOverridesData::new(instance.id.clone());
		overrides.memory = Some(MemorySettings { maximum: 16384 });
		let mut celestial = CelestialJson {
			launch_overrides: Some(overrides),
			..CelestialJson::default()
		};
		celestial.synced_options.game_options = true;
		celestial.write_to_dir(dir.path()).unwrap();
		for launch_overrides in [None, Some(InstanceLaunchOverridesData::new(instance.id.clone()))] {
			InstanceJson {
				launch_overrides,
				..InstanceJson::default()
			}.write_to_dir(dir.path()).unwrap();
			let loaded = read_json_instance_settings(dir.path(), &instance).unwrap();
			assert!(loaded.launch_overrides.memory.is_none());
			assert!(loaded.synced_options.game_options);
		}
	}

	#[test]
	fn absent_sidecars_inherit_global_memory_without_writing_files() {
		let dir = tempfile::tempdir().unwrap();
		for format in [InstanceFormat::Modrinth, InstanceFormat::Minecraft] {
			let instance = instance(dir.path(), format);
			let loaded = read_json_instance_settings(dir.path(), &instance).unwrap();
			assert!(loaded.launch_overrides.memory.is_none());
			assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
		}
	}

	#[test]
	fn invalid_primary_sidecar_never_falls_back_to_lightweight_settings() {
		let dir = tempfile::tempdir().unwrap();
		let instance = instance(dir.path(), InstanceFormat::Minecraft);
		CelestialJson::default().write_to_dir(dir.path()).unwrap();
		let path = dir.path().join("instance.json");
		std::fs::write(&path, "{").unwrap();
		assert!(read_json_instance_settings(dir.path(), &instance).is_err());
		std::fs::remove_file(&path).unwrap();
		std::fs::create_dir(&path).unwrap();
		assert!(read_json_instance_settings(dir.path(), &instance).is_err());
	}

	#[test]
	fn invalid_lightweight_sidecar_is_not_reported_as_inherited_memory() {
		let dir = tempfile::tempdir().unwrap();
		let instance = instance(dir.path(), InstanceFormat::Minecraft);
		let path = dir.path().join("celestial.json");
		std::fs::write(&path, "{").unwrap();
		assert!(read_json_instance_settings(dir.path(), &instance).is_err());
		std::fs::remove_file(&path).unwrap();
		std::fs::create_dir(&path).unwrap();
		assert!(read_json_instance_settings(dir.path(), &instance).is_err());
	}
}
