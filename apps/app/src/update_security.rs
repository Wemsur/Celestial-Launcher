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
	"download.celestial.byside.top",
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
	"download.celestial.byside.top",
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
	host_in(url, METADATA_HOSTS)
}

pub fn download_host_allowed(url: &Url) -> bool {
	host_in(url, DOWNLOAD_HOSTS)
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
	use minisign_verify::{PublicKey, Signature};

	let key = PublicKey::from_base64(UPDATER_PUBLIC_KEY)
		.map_err(|error| format!("The embedded update key is unusable: {error}"))?;
	let signature = Signature::decode(signature)
		.map_err(|error| format!("The update signature is malformed: {error}"))?;

	// `allow_legacy` stays off: releases are signed by the current signer, and
	// accepting the older format would only widen what counts as valid.
	key.verify(data, &signature, false)
		.map_err(|error| format!("The update signature does not match it: {error}"))
}
