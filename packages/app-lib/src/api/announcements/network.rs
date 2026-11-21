//! Restricted public announcement downloads; neither source receives credentials.

use super::input;
use crate::util::fetch::NO_REDIRECT_REQWEST_CLIENT;
use reqwest::header::{CACHE_CONTROL, LOCATION};
use url::Url;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Source {
	Wemgit,
	Github,
}

impl Source {
	fn location(self) -> (&'static str, &'static str, &'static str) {
		match self {
			Self::Wemgit => (
				"download.celestial.byside.top",
				"/celestial/celestial-announcement/raw/branch/main/",
				"https://download.celestial.byside.top/celestial/celestial-announcement/raw/branch/main/",
			),
			Self::Github => (
				"raw.githubusercontent.com",
				"/Wemsur/celestial-announcement/published/",
				"https://raw.githubusercontent.com/Wemsur/celestial-announcement/published/",
			),
		}
	}

	fn timeout(self) -> std::time::Duration {
		std::time::Duration::from_secs(match self { Self::Wemgit => 4, Self::Github => 15 })
	}

	pub(super) fn label(self) -> &'static str {
		match self { Self::Wemgit => "wemgit", Self::Github => "GitHub published" }
	}

	pub(super) fn body_order(preferred: Option<Self>) -> [Self; 2] {
		match preferred {
			Some(Self::Github) => [Self::Github, Self::Wemgit],
			_ => [Self::Wemgit, Self::Github],
		}
	}
}

pub(super) const SOURCES: [Source; 2] = [Source::Wemgit, Source::Github];

fn validate_url(url: &Url, source: Source) -> crate::Result<()> {
	let (host, root, _) = source.location();
	if url.scheme() != "https" || url.host_str() != Some(host)
		|| url.port().is_some_and(|port| port != 443)
		|| !url.username().is_empty() || url.password().is_some()
		|| url.query().is_some() || url.fragment().is_some()
		|| !url.path().starts_with(root) || url.path().contains('%') || url.path().contains('\\')
	{
		return Err(input("Announcement URL is outside its public source"));
	}
	Ok(())
}

pub(super) async fn fetch(source: Source, path: &str, limit: usize) -> crate::Result<Vec<u8>> {
	let started = std::time::Instant::now();
	let result = tokio::time::timeout(source.timeout(), fetch_inner(source, path, limit))
		.await.unwrap_or_else(|_| Err(input(format!("{} announcement request timed out", source.label()))));
	match &result {
		Ok(bytes) => tracing::info!(source = source.label(), bytes = bytes.len(), elapsed_ms = started.elapsed().as_millis() as u64, "[announcements] 公告文件获取完成"),
		Err(error) => tracing::warn!(source = source.label(), elapsed_ms = started.elapsed().as_millis() as u64, "[announcements] 公告请求失败: {error}"),
	}
	result
}

async fn fetch_inner(source: Source, path: &str, limit: usize) -> crate::Result<Vec<u8>> {
	if path.starts_with('/') || path.contains("..") || path.contains('\\') || path.contains('%') || path.contains(':') || path.contains('?') || path.contains('#') {
		return Err(input("Invalid announcement file path"));
	}
	let mut url = Url::parse(source.location().2).map_err(|_| input("Invalid announcement source"))?
		.join(path).map_err(|_| input("Invalid announcement file path"))?;
	for _ in 0..=5 {
		validate_url(&url, source)?;
		tracing::info!(source = source.label(), url = %url, "[announcements] 发起公告 HTTP 请求");
		let mut response = NO_REDIRECT_REQWEST_CLIENT.get(url.clone())
			.header(CACHE_CONTROL, "no-cache")
			.send().await?;
		tracing::info!(source = source.label(), status = response.status().as_u16(), "[announcements] 公告 HTTP 响应");
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
			status => return Err(input(format!("{} announcement server returned HTTP {status}", source.label()))),
		}
	}
	Err(input("Too many announcement redirects"))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn sources_are_isolated_and_only_read_published_data() {
		for source in SOURCES {
			let valid = Url::parse(&format!("{}index.json", source.location().2)).unwrap();
			assert!(validate_url(&valid, source).is_ok());
			let other = match source { Source::Wemgit => Source::Github, Source::Github => Source::Wemgit };
			assert!(validate_url(&valid, other).is_err());
		}
		assert!(validate_url(&Url::parse("https://raw.githubusercontent.com/Wemsur/celestial-announcement/main/index.json").unwrap(), Source::Github).is_err());
	}

	#[test]
	fn fallback_has_a_larger_cold_connection_budget() {
		assert_eq!(Source::Wemgit.timeout(), std::time::Duration::from_secs(4));
		assert_eq!(Source::Github.timeout(), std::time::Duration::from_secs(15));
	}

	#[test]
	fn body_prefers_the_accepted_index_source() {
		assert_eq!(Source::body_order(None), [Source::Wemgit, Source::Github]);
		assert_eq!(Source::body_order(Some(Source::Github)), [Source::Github, Source::Wemgit]);
	}

	#[test]
	fn rejects_other_hosts_ports_and_paths() {
		for url in [
			"http://download.celestial.byside.top/celestial/celestial-announcement/raw/branch/main/index.json",
			"https://download.celestial.byside.top:444/celestial/celestial-announcement/raw/branch/main/index.json",
			"https://user:secret@download.celestial.byside.top/celestial/celestial-announcement/raw/branch/main/index.json",
			"https://download.celestial.byside.top/other/index.json",
			"https://localhost/index.json",
		] {
			assert!(validate_url(&Url::parse(url).unwrap(), Source::Wemgit).is_err());
		}
	}
}
