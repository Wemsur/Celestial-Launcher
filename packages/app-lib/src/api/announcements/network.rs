//! Restricted public announcement downloads; this client never sends credentials.

use super::input;
use crate::util::fetch::NO_REDIRECT_REQWEST_CLIENT;
use reqwest::header::{CACHE_CONTROL, LOCATION};
use url::Url;

const HOST: &str = "download.celestial.byside.top";
const ROOT: &str = "/celestial/celestial-announcement/raw/branch/main/";
const BASE: &str = "https://download.celestial.byside.top/celestial/celestial-announcement/raw/branch/main/";

fn validate_url(url: &Url) -> crate::Result<()> {
	if url.scheme() != "https" || url.host_str() != Some(HOST)
		|| url.port().is_some_and(|port| port != 443)
		|| !url.username().is_empty() || url.password().is_some()
		|| url.query().is_some() || url.fragment().is_some()
		|| !url.path().starts_with(ROOT) || url.path().contains('%') || url.path().contains('\\')
	{
		return Err(input("Announcement URL is outside the public repository"));
	}
	Ok(())
}

pub(super) async fn fetch(path: &str, limit: usize) -> crate::Result<Vec<u8>> {
	if path.starts_with('/') || path.contains("..") || path.contains('\\') || path.contains('%') || path.contains(':') || path.contains('?') || path.contains('#') {
		return Err(input("Invalid announcement file path"));
	}
	let mut url = Url::parse(BASE).map_err(|_| input("Invalid announcement source"))?
		.join(path).map_err(|_| input("Invalid announcement file path"))?;
	for _ in 0..=5 {
		validate_url(&url)?;
		let mut response = NO_REDIRECT_REQWEST_CLIENT.get(url.clone())
			.header(CACHE_CONTROL, "no-cache")
			.send().await?;
		match response.status().as_u16() {
			301 | 302 | 303 | 307 | 308 => {
				let location = response.headers().get(LOCATION)
					.and_then(|value| value.to_str().ok()).ok_or_else(|| input("Announcement redirect has no valid location"))?;
				url = url.join(location).map_err(|_| input("Invalid announcement redirect"))?;
			}
			200 => {
				let mut bytes = Vec::new();
				while let Some(chunk) = response.chunk().await? {
					if chunk.len() > limit.saturating_sub(bytes.len()) {
						return Err(input("Announcement file exceeds the size limit"));
					}
					bytes.extend_from_slice(&chunk);
				}
				return Ok(bytes);
			}
			status => return Err(input(format!("Announcement server returned HTTP {status}"))),
		}
	}
	Err(input("Too many announcement redirects"))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn rejects_other_hosts_ports_and_paths() {
		assert!(validate_url(&Url::parse(&format!("{BASE}index.json")).unwrap()).is_ok());
		for url in [
			"http://download.celestial.byside.top/celestial/celestial-announcement/raw/branch/main/index.json",
			"https://download.celestial.byside.top:444/celestial/celestial-announcement/raw/branch/main/index.json",
			"https://user:secret@download.celestial.byside.top/celestial/celestial-announcement/raw/branch/main/index.json",
			"https://download.celestial.byside.top/other/index.json",
			"https://localhost/index.json",
		] {
			assert!(validate_url(&Url::parse(url).unwrap()).is_err());
		}
	}
}
