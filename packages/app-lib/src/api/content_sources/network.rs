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

    // Built before the lookup: a background refresh re-sends the same request,
    // the connection-level headers are dropped exactly as they are below, and a
    // request carrying credentials must not be cached at all.
    let mut headers = HashMap::new();
    if let Some(requested) = request.headers {
        for (name, value) in requested {
            if is_forbidden_header(&name) {
                continue;
            }
            headers.insert(name, value);
        }
    }

    // A cacheable request is answered from disk when the launcher has it, so a
    // source gets Modrinth's behaviour — a revisit costs nothing — without
    // implementing a cache of its own. The lookup happens after the host check,
    // so a cached entry can never be read for a host the source did not declare.
    let cache_key = super::http_cache::cache_key(
        method.as_str(),
        request.url.as_str(),
        request.body.as_deref(),
        &headers,
    );
    let state = if cache_key.is_some() {
        crate::State::get().await.ok()
    } else {
        None
    };

    if let (Some(key), Some(state)) = (cache_key.as_deref(), state.as_ref())
        && let Ok(Some(cached)) =
            super::http_cache::get(source_id, key, state).await
    {
        if !cached.fresh {
            // Stale: answer now, refresh behind the caller's back. The refresh
            // has to re-run the whole request, so it is spawned rather than
            // awaited — a slow site must not make the cached answer slow too.
            let source_id = source_id.to_string();
            let key = key.to_string();
            let headers = headers.clone();
            let body = request.body.clone();
            tokio::spawn(async move {
                if let Err(error) =
                    refresh(&source_id, &key, headers, body).await
                {
                    tracing::debug!(
                        source = %source_id,
                        url = %key,
                        "Content source background cache refresh failed: {error}"
                    );
                }
            });
        }
        return Ok(cached.response);
    }

    // Redirects are followed by hand so every hop's host is checked, not just
    // the first: otherwise a declared host could bounce the request to one that
    // was never granted. `NO_REDIRECT_REQWEST_CLIENT` never follows a `Location`
    // on its own.
    let response = crate::util::fetch::send_checked(
        &crate::util::fetch::NO_REDIRECT_REQWEST_CLIENT,
        method,
        url.clone(),
        &headers,
        request.body.as_deref(),
        |host| async move { ensure_host_allowed(source_id, &host).await },
    )
    .await?;

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
        .collect::<HashMap<_, _>>();

    let bytes = response.bytes().await?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(crate::ErrorKind::InputError(format!(
            "Content source response exceeds {} MiB",
            MAX_RESPONSE_BYTES / (1024 * 1024)
        ))
        .into());
    }

    let result = SourceFetchResponse {
        status: status.as_u16(),
        ok: status.is_success(),
        headers,
        body: String::from_utf8_lossy(&bytes).into_owned(),
        final_url,
    };

    if let (Some(key), Some(state)) = (cache_key, state.as_ref())
        && result.ok
        && let Some(ttl) = super::http_cache::cacheable_for(&result.headers)
        && let Err(error) =
            super::http_cache::put(source_id, &key, ttl, &result, state).await
    {
        // A cache that cannot be written is a slow launcher, not a broken one.
        tracing::debug!(
            source = %source_id,
            url = %key,
            "Could not store a content source response: {error}"
        );
    }

    Ok(result)
}

/// Re-fetch a URL and store the answer, for a stale cache entry.
async fn refresh(
    source_id: &str,
    url: &str,
    headers: HashMap<String, String>,
    body: Option<String>,
) -> crate::Result<()> {
    let parsed = url::Url::parse(url).map_err(|error| {
        crate::ErrorKind::InputError(format!("Invalid URL: {error}"))
    })?;
    let host = parsed.host_str().ok_or_else(|| {
        crate::ErrorKind::InputError("URL has no host".to_string())
    })?;
    ensure_host_allowed(source_id, host).await?;

    let response = crate::util::fetch::send_checked(
        &crate::util::fetch::NO_REDIRECT_REQWEST_CLIENT,
        reqwest::Method::GET,
        parsed,
        &headers,
        body.as_deref(),
        |host| async move { ensure_host_allowed(source_id, &host).await },
    )
    .await?;

    let status = response.status();
    let final_url = Some(response.url().to_string());
    let response_headers = response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect::<HashMap<_, _>>();
    let bytes = response.bytes().await?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Ok(());
    }

    let result = SourceFetchResponse {
        status: status.as_u16(),
        ok: status.is_success(),
        headers: response_headers,
        body: String::from_utf8_lossy(&bytes).into_owned(),
        final_url,
    };

    if !result.ok {
        return Ok(());
    }
    let Some(ttl) = super::http_cache::cacheable_for(&result.headers) else {
        return Ok(());
    };
    let state = crate::State::get().await?;
    super::http_cache::put(source_id, url, ttl, &result, &state).await
}

/// Whether `host` is covered by the hosts the source declared in its manifest.
///
/// Public because the launcher's own browser-download window has to apply the
/// same check before it opens a page: a `WebviewWindow` is outside the network
/// proxy, so without this a source could aim a launcher window at any host.
pub async fn ensure_host_allowed(
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
