//! Minecraft file-source selection. Metadata and other download clients stay unchanged.

use crate::state::{State, celestial_db::ui_settings};
use crate::util::fetch::{self, CheckedFileFailure, DownloadedFile, FetchProgressFn};
use parking_lot::Mutex;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

const MIRROR_HOST: &str = "bmclapi2.bangbang93.com";
const STALL_TIMEOUT: Duration = Duration::from_secs(5);
const SOURCE_COOLDOWN: Duration = Duration::from_secs(60);
const MAX_ATTEMPTS: usize = 4;
const ALLOWED_HOSTS: &[&str] = &[
	"piston-data.mojang.com",
	"piston-meta.mojang.com",
	"launcher.mojang.com",
	"launchermeta.mojang.com",
	"resources.download.minecraft.net",
	"libraries.minecraft.net",
	MIRROR_HOST,
];

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DownloadSourcePreference {
	#[default]
	Auto,
	OfficialFirst,
	MirrorFirst,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum FileRole {
	Launcher,
	Asset,
	Library,
}

#[derive(Clone, Debug)]
struct Candidate {
	url: Url,
	mirror: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct MinecraftDownloadSources {
	preference: DownloadSourcePreference,
	mirror_until: Arc<Mutex<HashMap<(FileRole, String), Instant>>>,
}

fn read_preference(raw: Option<&str>) -> DownloadSourcePreference {
	let Some(raw) = raw else { return DownloadSourcePreference::Auto };
	let Ok(document) = serde_json::from_str::<serde_json::Value>(raw) else {
		tracing::warn!("UI preferences are not valid JSON; using automatic Minecraft sources");
		return DownloadSourcePreference::Auto;
	};
	document.get("minecraftDownloadSourcePreference")
		.and_then(|value| serde_json::from_value(value.clone()).ok())
		.unwrap_or_default()
}

impl MinecraftDownloadSources {
	pub(crate) async fn load(state: &State) -> crate::Result<Self> {
		let raw = ui_settings::get_json(state.celestial.pool(), ui_settings::KEY_UI_PREFERENCES).await?;
		Ok(Self::new(read_preference(raw.as_deref())))
	}

	fn new(preference: DownloadSourcePreference) -> Self {
		Self { preference, mirror_until: Arc::new(Mutex::new(HashMap::new())) }
	}

	fn prefers_mirror(&self, role: FileRole, host: &str, now: Instant) -> bool {
		match self.preference {
			DownloadSourcePreference::MirrorFirst => true,
			DownloadSourcePreference::OfficialFirst => false,
			DownloadSourcePreference::Auto => {
				let key = (role, host.to_string());
				let mut states = self.mirror_until.lock();
				if states.get(&key).is_some_and(|until| *until > now) {
					true
				} else {
					states.remove(&key);
					false
				}
			}
		}
	}

	fn record_success(&self, role: FileRole, host: &str, mirror: bool, original_failed: bool) {
		if self.preference != DownloadSourcePreference::Auto { return; }
		let key = (role, host.to_string());
		let mut states = self.mirror_until.lock();
		if mirror && original_failed {
			states.insert(key, Instant::now() + SOURCE_COOLDOWN);
		} else if !mirror {
			states.remove(&key);
		}
	}

	fn candidates(&self, original: &str, role: FileRole, trusted_file: bool) -> crate::Result<Vec<Candidate>> {
		let mut original = Url::parse(original).map_err(|error| {
			crate::ErrorKind::InputError(format!("Invalid Minecraft download URL: {error}"))
		})?;
		if !original.username().is_empty() || original.password().is_some() {
			return Err(crate::ErrorKind::InputError("Minecraft download URLs must not contain credentials".into()).into());
		}
		if original.scheme() == "http" && original.host_str() == Some("resources.download.minecraft.net") {
			original.set_scheme("https").map_err(|_| {
				crate::ErrorKind::InputError("Cannot upgrade the Minecraft resource URL to HTTPS".into())
			})?;
		}
		let host = original.host_str().unwrap_or_default();
		let prefixes: &[&str] = if trusted_file && original.scheme() == "https" && original.port_or_known_default() == Some(443) {
			match role {
				FileRole::Launcher if matches!(host, "piston-data.mojang.com" | "piston-meta.mojang.com" | "launcher.mojang.com" | "launchermeta.mojang.com") => &[""],
				FileRole::Asset if matches!(host, "resources.download.minecraft.net" | "piston-data.mojang.com" | "piston-meta.mojang.com") => &["/assets"],
				FileRole::Library if matches!(host, "libraries.minecraft.net" | "piston-data.mojang.com" | "piston-meta.mojang.com") => &["/maven", "/libraries"],
				_ => &[],
			}
		} else {
			&[]
		};
		let mut mirrors = Vec::new();
		for prefix in prefixes {
			let mut url = original.clone();
			url.set_host(Some(MIRROR_HOST)).map_err(|error| {
				crate::ErrorKind::InputError(format!("Invalid Minecraft mirror host: {error}"))
			})?;
			url.set_path(&format!("{prefix}{}", original.path()));
			mirrors.push(Candidate { url, mirror: true });
		}
		let official = Candidate { url: original.clone(), mirror: false };
		let mut ordered = if self.prefers_mirror(role, host, Instant::now()) {
			mirrors.push(official);
			mirrors
		} else {
			let mut ordered = vec![official];
			ordered.extend(mirrors);
			ordered
		};
		let mut seen = HashSet::new();
		ordered.retain(|candidate| seen.insert(candidate.url.to_string()));
		Ok(ordered)
	}

	pub(crate) fn candidate_urls(&self, original: &str, role: FileRole, sha1: Option<&str>, size: Option<u64>) -> crate::Result<Vec<String>> {
		let trusted = has_trusted_file(sha1, size);
		Ok(self.candidates(original, role, trusted)?.into_iter().map(|candidate| candidate.url.to_string()).collect())
	}

	pub(crate) async fn fetch(
		&self,
		state: &State,
		original: &str,
		role: FileRole,
		sha1: Option<&str>,
		size: Option<u64>,
		mut progress: Option<&mut FetchProgressFn<'_>>,
	) -> crate::Result<DownloadedFile> {
		let candidates = self.candidates(original, role, has_trusted_file(sha1, size))?;
		if !candidates.iter().any(|candidate| candidate.mirror) {
			return fetch::fetch_file(candidates[0].url.as_str(), sha1, None, None, &state.fetch_semaphore, &state.pool, progress).await;
		}
		let sha1 = sha1.expect("mirror candidates require a trusted hash");
		let size = size.expect("mirror candidates require a known size");
		let allowed_hosts: Vec<_> = ALLOWED_HOSTS.iter().map(|host| host.to_string()).collect();
		let original_host = Url::parse(original).ok().and_then(|url| url.host_str().map(str::to_string)).unwrap_or_default();
		let mut can_retry = vec![false; candidates.len()];
		let mut cooldowns = HashMap::<String, Instant>::new();
		let mut attempts = 0;
		let mut original_failed = false;
		let mut last_error = None;
		'rounds: for round in 0..2 {
			for (index, candidate) in candidates.iter().enumerate() {
				if round > 0 && !can_retry[index] { continue; }
				if !claim_attempt(&mut attempts) { break 'rounds; }
				let host = candidate.url.host_str().unwrap_or_default();
				if let Some(until) = cooldowns.get(host) {
					let remaining = until.saturating_duration_since(Instant::now());
					if !remaining.is_zero() { tokio::time::sleep(remaining).await; }
				} else if round > 0 {
					tokio::time::sleep(Duration::from_millis(250)).await;
				}
				tracing::debug!(host, attempt = attempts, mirror = candidate.mirror, "Trying Minecraft file source");
				match fetch::fetch_checked_file(
					candidate.url.clone(), sha1, size, &allowed_hosts,
					&state.fetch_semaphore, progress.as_deref_mut(), STALL_TIMEOUT,
				).await {
					Ok(file) => {
						tracing::debug!(host, ?role, mirror = candidate.mirror, "Minecraft file source verified");
						self.record_success(role, &original_host, candidate.mirror, original_failed);
						return Ok(file);
					}
					Err(CheckedFileFailure::Fatal(error)) => return Err(error),
					Err(CheckedFileFailure::Rejected(error)) => {
						can_retry[index] = false;
						tracing::warn!(host, %error, "Minecraft file source rejected; trying another source");
						last_error = Some(error);
					}
					Err(CheckedFileFailure::Retryable { error, retry_after, stalled }) => {
						can_retry[index] = true;
						if !candidate.mirror { original_failed = true; }
						if let Some(delay) = retry_after {
							cooldowns.insert(host.to_string(), Instant::now() + delay.min(Duration::from_secs(5)));
						}
						tracing::warn!(host, stalled, %error, "Minecraft file source failed; trying another source");
						last_error = Some(error);
					}
				}
			}
		}
		Err(last_error.unwrap_or_else(|| crate::ErrorKind::FSError("No Minecraft download source succeeded".into()).into()))
	}
}

fn claim_attempt(attempts: &mut usize) -> bool {
	if *attempts >= MAX_ATTEMPTS { return false; }
	*attempts += 1;
	true
}

fn has_trusted_file(sha1: Option<&str>, size: Option<u64>) -> bool {
	sha1.is_some_and(|hash| hash.len() == 40 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
		&& size.is_some_and(|size| size > 0)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn urls(preference: DownloadSourcePreference, url: &str, role: FileRole) -> Vec<String> {
		MinecraftDownloadSources::new(preference).candidate_urls(url, role, Some(&"a".repeat(40)), Some(1)).unwrap()
	}

	#[test]
	fn preferences_are_backward_compatible_and_validated() {
		assert_eq!(read_preference(None), DownloadSourcePreference::Auto);
		for raw in ["{}", "{", r#"{"minecraftDownloadSourcePreference":"other"}"#, r#"{"minecraftDownloadSourcePreference":true}"#] {
			assert_eq!(read_preference(Some(raw)), DownloadSourcePreference::Auto);
		}
		assert_eq!(read_preference(Some(r#"{"githubMirrorEnabled":true,"minecraftDownloadSourcePreference":"mirror_first"}"#)), DownloadSourcePreference::MirrorFirst);
	}

	#[test]
	fn candidates_keep_encoded_paths_queries_and_source_preference() {
		let url = "https://libraries.minecraft.net/a%20b/test.jar?value=1%2F2";
		let mirror_first = urls(DownloadSourcePreference::MirrorFirst, url, FileRole::Library);
		assert_eq!(mirror_first, vec![
			"https://bmclapi2.bangbang93.com/maven/a%20b/test.jar?value=1%2F2",
			"https://bmclapi2.bangbang93.com/libraries/a%20b/test.jar?value=1%2F2",
			url,
		]);
		let official_first = urls(DownloadSourcePreference::OfficialFirst, url, FileRole::Library);
		assert_eq!(official_first[0], url);
		assert_eq!(urls(DownloadSourcePreference::Auto, url, FileRole::Library), official_first);
	}

	#[test]
	fn launcher_assets_unknown_hosts_and_untrusted_files() {
		for host in ["piston-data.mojang.com", "piston-meta.mojang.com", "launcher.mojang.com", "launchermeta.mojang.com"] {
			let result = urls(DownloadSourcePreference::MirrorFirst, &format!("https://{host}/v1/objects/hash/client.jar"), FileRole::Launcher);
			assert_eq!(result[0], "https://bmclapi2.bangbang93.com/v1/objects/hash/client.jar");
		}
		assert_eq!(urls(DownloadSourcePreference::MirrorFirst, "http://resources.download.minecraft.net/ab/hash", FileRole::Asset), vec![
			"https://bmclapi2.bangbang93.com/assets/ab/hash", "https://resources.download.minecraft.net/ab/hash",
		]);
		for url in ["https://libraries.minecraft.net.evil.test/file", "https://maven.fabricmc.net/file", "https://cdn.modrinth.com/file", "https://libraries.minecraft.net:8443/file"] {
			assert_eq!(urls(DownloadSourcePreference::MirrorFirst, url, FileRole::Library), vec![url]);
		}
		let sources = MinecraftDownloadSources::new(DownloadSourcePreference::MirrorFirst);
		assert_eq!(sources.candidate_urls("https://libraries.minecraft.net/file", FileRole::Library, None, Some(1)).unwrap().len(), 1);
		assert!(sources.candidates("https://user:password@libraries.minecraft.net/file", FileRole::Library, true).is_err());
	}

	#[test]
	fn attempt_budget_and_unverified_files_do_not_expand_sources() {
		let mut attempts = 0;
		for _ in 0..MAX_ATTEMPTS { assert!(claim_attempt(&mut attempts)); }
		assert!(!claim_attempt(&mut attempts));
		assert!(!claim_attempt(&mut attempts));
		assert_eq!(attempts, MAX_ATTEMPTS);
		assert!(!has_trusted_file(Some("not a SHA1"), Some(10)));
		assert!(!has_trusted_file(Some(&"a".repeat(40)), None));
		assert!(!has_trusted_file(Some(&"a".repeat(40)), Some(0)));
	}

	#[test]
	fn automatic_promotion_is_scoped_and_expires() {
		let sources = MinecraftDownloadSources::new(DownloadSourcePreference::Auto);
		let host = "piston-data.mojang.com";
		sources.record_success(FileRole::Launcher, host, true, false);
		assert!(!sources.prefers_mirror(FileRole::Launcher, host, Instant::now()));
		sources.record_success(FileRole::Launcher, host, true, true);
		assert!(sources.prefers_mirror(FileRole::Launcher, host, Instant::now()));
		assert!(!sources.prefers_mirror(FileRole::Library, host, Instant::now()));
		assert!(!sources.prefers_mirror(FileRole::Launcher, "launcher.mojang.com", Instant::now()));
		assert!(!sources.prefers_mirror(FileRole::Launcher, host, Instant::now() + SOURCE_COOLDOWN));
		sources.record_success(FileRole::Launcher, host, true, true);
		sources.record_success(FileRole::Launcher, host, false, false);
		assert!(!sources.prefers_mirror(FileRole::Launcher, host, Instant::now()));
		let fixed = MinecraftDownloadSources::new(DownloadSourcePreference::OfficialFirst);
		fixed.record_success(FileRole::Launcher, host, true, true);
		assert!(!fixed.prefers_mirror(FileRole::Launcher, host, Instant::now()));
	}
}
