//! What an update has to pass before its installer runs.
//!
//! Three things live here, all compiled in rather than fetched: the key a
//! signature is checked against, the hosts an update may come from, and the
//! name the installer is written under. Any of them read from the network would
//! be as replaceable as the download itself.
//!
//! The signature is the part that actually decides. The host lists exist so a
//! redirected or hand-edited download fails early and with a clear reason,
//! rather than as a verification error from a host nobody expected.

use std::path::PathBuf;
use std::sync::Mutex;
use url::Url;

/// The minisign public key `tauri signer generate` printed: the base64 of the
/// whole key file. Its private half exists only in the release workflow's
/// secrets, and nothing in the launcher can sign with it.
///
/// Rotating it means shipping a release signed with the *old* private key that
/// carries the new public key. The reverse order leaves every installed copy
/// unable to verify anything, and the only way back is a manual reinstall.
pub const UPDATER_PUBLIC_KEY: &str =
	"dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDRCODk0M0U2NEM5QkNDOUEKUldTYXpKdE01a09KUzBNRUJNMjJ5RlROU1lEMXl4cUtIbW81bTBnMmswYzhhcFkzMC9OU3JFeUgK";

/// The release APIs the update check reads.
const METADATA_HOSTS: &[&str] = &[
	"api.gitcode.com",
	"git.gay",
	"api.github.com",
];

/// Where an installer and its signature may be fetched from, including every
/// redirect hop.
///
/// The three release mirrors, plus GitHub's asset hosts: `github.com` answers a
/// release-asset request with a redirect to one of the `*.githubusercontent.com`
/// hosts, so both legs have to be listed.
///
/// This list is also a maintenance cost — a host that changes breaks updates
/// until this constant does. The signature is what makes that safe to notice:
/// the worst a stale list does is refuse a legitimate update, never accept a
/// forged one.
const DOWNLOAD_HOSTS: &[&str] = &[
	"api.gitcode.com",
	"git.gay",
	"github.com",
	"objects.githubusercontent.com",
	"release-assets.githubusercontent.com",
];

/// The installer that has been downloaded, verified and is waiting for the user
/// to restart.
///
/// Held here rather than handed to the frontend. A path the page can name is a
/// path anything running in the page can name, and this path is executed.
#[derive(Default)]
pub struct PendingInstaller(pub Mutex<Option<PathBuf>>);

pub fn metadata_host_allowed(url: &Url) -> bool {
	if !safe_https(url) || !host_in(url, METADATA_HOSTS) {
		return false;
	}
	url.host_str() != Some("api.gitcode.com") || url.path().starts_with(GITCODE_RELEASE_ROOT)
}

const GITCODE_RELEASE_ROOT: &str = "/api/v5/repos/Wemsur/Celestial-Launcher/releases/";

fn safe_https(url: &Url) -> bool {
	url.scheme() == "https" && url.port().is_none_or(|port| port == 443)
		&& url.username().is_empty() && url.password().is_none()
}

pub fn download_host_allowed(url: &Url) -> bool {
	if !safe_https(url) { return false; }
	match url.host_str() {
		Some("api.gitcode.com") => {
			url.path().starts_with(GITCODE_RELEASE_ROOT) && url.path().ends_with("/download")
				&& url.path().contains("/attach_files/") && !url.path().contains("%2f") && !url.path().contains("%2F")
		}
		Some("gitcode.com") => url.path().starts_with("/Wemsur/Celestial-Launcher/"),
		Some("file-cdn.gitcode.com") => url.path().starts_with("/11450940/releases/"),
		Some(host) if gitcode_object_host(host) => true,
		_ => host_in(url, DOWNLOAD_HOSTS),
	}
}

fn gitcode_object_host(host: &str) -> bool {
	let Some((bucket, storage)) = host.split_once(".obs.") else { return false; };
	let Some(region) = storage.strip_suffix(".myhuaweicloud.com") else { return false; };
	(bucket == "gitcode" || bucket.starts_with("gitcode-") || bucket == "atomgit" || bucket.starts_with("atomgit-"))
		&& bucket.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
		&& !region.is_empty()
		&& region.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn host_in(url: &Url, allowed: &[&str]) -> bool {
	match url.host_str() {
		Some(host) => allowed
			.iter()
			.any(|candidate| host.eq_ignore_ascii_case(candidate)),
		None => false,
	}
}

/// Follow redirects only while they stay inside `allowed`.
///
/// A redirect is the easy way around an allowlist: the first URL looks right
/// and the next hop goes anywhere. `reqwest` follows redirects inside the
/// request, so the check has to be part of its policy rather than something
/// done to the URL before sending it.
pub fn redirect_policy(
	allowed: fn(&Url) -> bool,
) -> tauri_plugin_http::reqwest::redirect::Policy {
	use tauri_plugin_http::reqwest::redirect::Policy;

	Policy::custom(move |attempt| {
		if allowed(attempt.url()) {
			attempt.follow()
		} else {
			let message = format!(
				"Update download redirected to an untrusted host: {}",
				attempt.url()
			);
			attempt.error(message)
		}
	})
}

/// A client for the update path.
///
/// The user agent matters: git.gay's WAF answers 403 to requests without a
/// browser-like one, so the metadata fetch succeeded while the asset download
/// did not until this was set. Redirects are limited to `allowed`.
pub fn client(
	allowed: fn(&Url) -> bool,
) -> Result<tauri_plugin_http::reqwest::Client, String> {
	tauri_plugin_http::reqwest::Client::builder()
		.user_agent(
			"Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
			 (KHTML, like Gecko) CelestialLauncher/updater",
		)
		.redirect(redirect_policy(allowed))
		.build()
		.map_err(|error| format!("Could not build the update client: {error}"))
}

/// Resolves the detached signature without appending `.sig` to a download action.
pub fn signature_url(asset: &Url) -> Result<Url, String> {
	if !download_host_allowed(asset) {
		return Err("Untrusted installer URL".into());
	}
	let mut signature = asset.clone();
	if asset.host_str() == Some("api.gitcode.com") {
		if asset.query().is_some() || asset.fragment().is_some() {
			return Err("GitCode attachment URLs must use the public canonical endpoint".into());
		}
		let prefix = asset.path().strip_suffix("/download")
			.ok_or_else(|| "Invalid GitCode attachment endpoint".to_string())?;
		let (release, filename) = prefix.rsplit_once("/attach_files/")
			.ok_or_else(|| "Invalid GitCode attachment endpoint".to_string())?;
		if !release.starts_with(GITCODE_RELEASE_ROOT) || filename.is_empty() || filename.contains('/') {
			return Err("Invalid GitCode attachment name".into());
		}
		signature.set_path(&format!("{release}/attach_files/{filename}.sig/download"));
	} else {
		signature.set_path(&format!("{}.sig", asset.path()));
	}
	if !download_host_allowed(&signature) { return Err("Untrusted signature URL".into()); }
	Ok(signature)
}

#[cfg(test)]
mod gitcode_tests {
	use super::*;

	#[test]
	fn signature_uses_filename_not_download_action() {
		let asset = Url::parse("https://api.gitcode.com/api/v5/repos/Wemsur/Celestial-Launcher/releases/v3.5.0/attach_files/CelestialLauncher-3.5.0-windows-x64-setup.exe/download").unwrap();
		assert_eq!(signature_url(&asset).unwrap().as_str(),
			"https://api.gitcode.com/api/v5/repos/Wemsur/Celestial-Launcher/releases/v3.5.0/attach_files/CelestialLauncher-3.5.0-windows-x64-setup.exe.sig/download");
		let legacy = Url::parse("https://github.com/Wemsur/Celestial-Launcher/releases/download/v3.5.0/setup.exe").unwrap();
		assert!(signature_url(&legacy).unwrap().path().ends_with("/setup.exe.sig"));
	}

	#[test]
	fn restricts_gitcode_sources_and_storage() {
		assert!(metadata_host_allowed(&Url::parse("https://api.gitcode.com/api/v5/repos/Wemsur/Celestial-Launcher/releases/latest?type=latest").unwrap()));
		assert!(!metadata_host_allowed(&Url::parse("https://api.gitcode.com/api/v5/repos/other/other/releases/latest").unwrap()));
		assert!(!download_host_allowed(&Url::parse("https://api.gitcode.com/api/v5/repos/other/other/releases/v1/attach_files/setup.exe/download").unwrap()));
		assert!(!download_host_allowed(&Url::parse("http://gitcode.com/Wemsur/Celestial-Launcher/setup.exe").unwrap()));
		assert!(!download_host_allowed(&Url::parse("https://user:password@gitcode.com/Wemsur/Celestial-Launcher/setup.exe").unwrap()));
		assert!(download_host_allowed(&Url::parse("https://file-cdn.gitcode.com/11450940/releases/test/setup.exe?signature=example").unwrap()));
		assert!(!download_host_allowed(&Url::parse("https://file-cdn.gitcode.com/other/releases/test/setup.exe").unwrap()));
		assert!(!download_host_allowed(&Url::parse("https://file-cdn.gitcode.com.evil.example/11450940/releases/test/setup.exe").unwrap()));
		assert!(gitcode_object_host("gitcode-release.obs.cn-north-4.myhuaweicloud.com"));
		assert!(!gitcode_object_host("unrelated.obs.cn-north-4.myhuaweicloud.com"));
		assert!(!gitcode_object_host("gitcode-release.obs.cn-north-4.myhuaweicloud.com.evil.example"));
	}
}

/// The name an installer is stored under.
///
/// Fixed rather than taken from the asset URL. Nobody is ever shown this name,
/// and deriving a path from an untrusted string is exactly how `..` gets in —
/// the reason the path stopped travelling through the frontend at all.
pub fn installer_file_name(asset_url: &str) -> String {
	if asset_url.to_ascii_lowercase().contains(".msi") {
		"celestial-update.msi".to_string()
	} else {
		"celestial-update.exe".to_string()
	}
}

/// Check that `signature` — the contents of a `.sig` file — was made for `data`
/// by the private key matching [`UPDATER_PUBLIC_KEY`].
pub fn verify_signature(data: &[u8], signature: &str) -> Result<(), String> {
	use base64::{engine::general_purpose::STANDARD, Engine as _};
	use minisign_verify::{PublicKey, Signature};

	let key_bytes = STANDARD
		.decode(UPDATER_PUBLIC_KEY.trim())
		.map_err(|error| format!("The embedded update key is not valid base64: {error}"))?;
	let key_text = std::str::from_utf8(&key_bytes)
		.map_err(|error| format!("The embedded update key is not valid text: {error}"))?;
	let key = PublicKey::decode(key_text)
		.map_err(|error| format!("The embedded update key is unusable: {error}"))?;

	let signature_bytes = STANDARD
		.decode(signature.trim())
		.map_err(|error| format!("The update signature is not valid base64: {error}"))?;
	let signature_text = std::str::from_utf8(&signature_bytes)
		.map_err(|error| format!("The update signature is not valid text: {error}"))?;
	let signature = Signature::decode(signature_text)
		.map_err(|error| format!("The update signature is malformed: {error}"))?;

	key.verify(data, &signature, false)
		.map_err(|error| format!("The update signature does not match it: {error}"))
}
