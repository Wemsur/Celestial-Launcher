//! The one capability a content source cannot reach on its own: the network.
//!
//! A source runs in the webview, where `fetch` is boxed in by the launcher's
//! CSP — it cannot add hosts to `connect-src`. So a source declares the hosts it
//! needs in its manifest and calls a command that lands here; this module checks
//! the host against that list and makes the request from Rust, where CSP does
//! not apply.
//!
//! This differs from the UI-plugin network proxy in one deliberate way: it
//! *allows* `cookie`, `referer`, `user-agent` and `authorization`. Content sites
//! routinely require a browser-looking request and a sign-in cookie, and an API
//! source needs a token — none of which the plugin proxy permits. The headers
//! that govern the connection itself are still refused, since forging those (a
//! `Host` for another site, say) is a request-smuggling foothold rather than a
//! legitimate content fetch.

use crate::util::fetch::REQWEST_CLIENT;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Cap on a proxied response. A source fetching JSON or a listing does not need
/// more, and an unbounded read is a way for a hostile endpoint to exhaust memory.
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

/// Cap on a request body, for the same reason in the other direction.
const MAX_REQUEST_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Deserialize)]
pub struct SourceFetchRequest {
    pub url: String,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub headers: Option<HashMap<String, String>>,
    #[serde(default)]
    pub body: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SourceFetchResponse {
    pub status: u16,
    pub ok: bool,
    pub headers: HashMap<String, String>,
    pub body: String,
    /// The final URL after redirects, so a source can follow a `Location` it
    /// could not read from the headers.
    #[serde(default)]
    pub final_url: Option<String>,
}

/// Perform a network request on behalf of `source_id`.
pub async fn fetch(
    source_id: &str,
    request: SourceFetchRequest,
) -> crate::Result<SourceFetchResponse> {
    let url = url::Url::parse(&request.url).map_err(|error| {
        crate::ErrorKind::InputError(format!("Invalid URL: {error}"))
    })?;

    if !matches!(url.scheme(), "http" | "https") {
        return Err(crate::ErrorKind::InputError(
            "Content source requests must be http or https".to_string(),
        )
        .into());
    }
    let host = url.host_str().ok_or_else(|| {
        crate::ErrorKind::InputError("URL has no host".to_string())
    })?;

    ensure_host_allowed(source_id, host).await?;

    let method = parse_method(request.method.as_deref())?;

    if let Some(body) = &request.body
        && body.len() > MAX_REQUEST_BYTES
    {
        return Err(crate::ErrorKind::InputError(format!(
            "Content source request body exceeds {} MiB",
            MAX_REQUEST_BYTES / (1024 * 1024)
        ))
        .into());
    }

    let mut builder = REQWEST_CLIENT.request(method, url.clone());
    if let Some(headers) = request.headers {
        for (name, value) in headers {
            if is_forbidden_header(&name) {
                continue;
            }
            builder = builder.header(name, value);
        }
    }
    if let Some(body) = request.body {
        builder = builder.body(body);
    }

    let response = builder.send().await?;
    let status = response.status();
    let final_url = Some(response.url().to_string());

    let headers = response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect();

    let bytes = response.bytes().await?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(crate::ErrorKind::InputError(format!(
            "Content source response exceeds {} MiB",
            MAX_RESPONSE_BYTES / (1024 * 1024)
        ))
        .into());
    }

    Ok(SourceFetchResponse {
        status: status.as_u16(),
        ok: status.is_success(),
        headers,
        body: String::from_utf8_lossy(&bytes).into_owned(),
        final_url,
    })
}

/// Whether `host` is covered by the hosts the source declared in its manifest.
pub(super) async fn ensure_host_allowed(
    source_id: &str,
    host: &str,
) -> crate::Result<()> {
    let summary = super::store::get(source_id).await?.ok_or_else(|| {
        crate::ErrorKind::InputError(format!(
            "Unknown content source '{source_id}'"
        ))
    })?;
    if !summary.enabled {
        return Err(crate::ErrorKind::InputError(format!(
            "Content source '{source_id}' is disabled"
        ))
        .into());
    }
    let manifest = summary.manifest.ok_or_else(|| {
        crate::ErrorKind::InputError(format!(
            "Content source '{source_id}' has an unreadable manifest"
        ))
    })?;

    let matches = manifest
        .hosts
        .iter()
        .any(|pattern| host_matches(pattern, host));

    if !matches {
        return Err(crate::ErrorKind::InputError(format!(
            "Content source '{source_id}' did not declare access to '{host}'"
        ))
        .into());
    }
    Ok(())
}

/// Match a declared host pattern against a request host.
///
/// `example.com` matches only that host. `*.example.com` matches any single
/// additional label (`api.example.com`) but not the bare domain and not a
/// deeper subdomain, which keeps a wildcard from quietly widening.
fn host_matches(pattern: &str, host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    let pattern = pattern.to_ascii_lowercase();

    if let Some(suffix) = pattern.strip_prefix("*.") {
        if let Some(rest) = host.strip_suffix(suffix) {
            let label = rest.strip_suffix('.').unwrap_or(rest);
            return !label.is_empty() && !label.contains('.');
        }
        return false;
    }
    pattern == host
}

fn parse_method(method: Option<&str>) -> crate::Result<reqwest::Method> {
    let method = method.unwrap_or("GET").to_ascii_uppercase();
    Ok(match method.as_str() {
        "GET" => reqwest::Method::GET,
        "POST" => reqwest::Method::POST,
        "PUT" => reqwest::Method::PUT,
        "PATCH" => reqwest::Method::PATCH,
        "DELETE" => reqwest::Method::DELETE,
        "HEAD" => reqwest::Method::HEAD,
        other => {
            return Err(crate::ErrorKind::InputError(format!(
                "Unsupported HTTP method '{other}'"
            ))
            .into());
        }
    })
}

/// Headers a source must not set: they govern the connection itself, and letting
/// a source forge them (a `Host` for another site, say) is a request-smuggling
/// foothold. Everything a content fetch legitimately needs — `cookie`,
/// `referer`, `user-agent`, `authorization` — is permitted.
pub(super) fn is_forbidden_header(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    matches!(
        name.as_str(),
        "host" | "content-length" | "connection" | "transfer-encoding"
    ) || name.starts_with("proxy-")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content_sources::manifest::is_valid_host_pattern;

    #[test]
    fn exact_host_matches_only_itself() {
        assert!(host_matches("example.com", "example.com"));
        assert!(host_matches("example.com", "EXAMPLE.com"));
        assert!(!host_matches("example.com", "api.example.com"));
    }

    #[test]
    fn wildcard_matches_one_label_only() {
        assert!(host_matches("*.example.com", "api.example.com"));
        assert!(!host_matches("*.example.com", "example.com"));
        assert!(!host_matches("*.example.com", "a.b.example.com"));
        assert!(!host_matches("*.example.com", "example.com.evil.com"));
    }

    #[test]
    fn content_headers_are_allowed_but_connection_headers_are_not() {
        assert!(!is_forbidden_header("Cookie"));
        assert!(!is_forbidden_header("Referer"));
        assert!(!is_forbidden_header("User-Agent"));
        assert!(!is_forbidden_header("Authorization"));
        assert!(is_forbidden_header("Host"));
        assert!(is_forbidden_header("Content-Length"));
        assert!(is_forbidden_header("proxy-connection"));
    }

    #[test]
    fn declared_hosts_must_be_valid_patterns() {
        assert!(is_valid_host_pattern("goto.pixelmap.cc"));
        assert!(is_valid_host_pattern("*.githubusercontent.com"));
    }
}
