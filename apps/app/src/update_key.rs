//! The public key update packages are checked against.
//!
//! Compiled into the binary rather than fetched. A key read over the network is
//! a key the same attacker who replaced the download could replace, which would
//! make the signature prove nothing.
//!
//! The value is the base64 of a minisign public key file: what
//! `tauri signer generate` prints, and what `minisign-verify` parses. The
//! matching private key lives only in the release workflow's secrets.
//!
//! Rotating it means shipping a release signed with the *old* private key that
//! carries the new public key. The reverse order leaves every installed copy
//! unable to verify anything, and the only way back is a manual reinstall.
#[allow(dead_code)]
pub const UPDATER_PUBLIC_KEY: &str =
	"dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDRCODk0M0U2NEM5QkNDOUEKUldTYXpKdE01a09KUzBNRUJNMjJ5RlROU1lEMXl4cUtIbW81bTBnMmswYzhhcFkzMC9OU3JFeUgK";
