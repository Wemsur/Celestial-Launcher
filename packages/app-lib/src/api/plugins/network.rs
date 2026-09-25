//! The one capability a plugin cannot reach on its own: the network.
//!
//! A plugin runs in the webview, where `fetch` is boxed in by the launcher's
//! CSP — it cannot add hosts to `connect-src`. So a plugin that needs the
//! network declares `network:<host>` and calls a command that lands here; this
//! module checks the host against the plugin's grants and makes the request
//! from Rust, where CSP does not apply.
//!
//! This is the launcher acting on a plugin's behalf, so the checks are strict:
//! the plugin must hold the exact host (wildcards match one label), the URL has
//! to be http(s), the method has to be one of a small set, and both the request
//! and the response body are capped.

use super::permissions::PluginPermission;
use crate::util::fetch::REQWEST_CLIENT;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Cap on a proxied response. A plugin fetching JSON does not need more, and an
/// unbounded read is a way for a hostile endpoint to exhaust memory.
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

/// Cap on a request body, for the same reason in the other direction.
const MAX_REQUEST_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Deserialize)]
pub struct PluginFetchRequest {
    pub url: String,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub headers: Option<HashMap<String, String>>,
    #[serde(default)]
    pub body: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PluginFetchResponse {
    pub status: u16,
    pub ok: bool,
    pub headers: HashMap<String, String>,
    pub body: String,
}

/// Perform a network request on behalf of `plugin_id`.
pub async fn fetch(
    plugin_id: &str,
    request: PluginFetchRequest,
) -> crate::Result<PluginFetchResponse> {
    let url = url::Url::parse(&request.url).map_err(|error| {
        crate::ErrorKind::InputError(format!("Invalid URL: {error}"))
    })?;

    if !matches!(url.scheme(), "http" | "https") {
        return Err(crate::ErrorKind::InputError(
            "Plugin requests must be http or https".to_string(),
        )
        .into());
    }
    let host = url.host_str().ok_or_else(|| {
        crate::ErrorKind::InputError("URL has no host".to_string())
    })?;

    // The plugin has to hold a `network:` grant whose scope covers this host.
    // `ensure_granted` matches a permission wholesale, so the host match is done
    // here against every granted network permission.
    ensure_host_granted(plugin_id, host).await?;

    let method = parse_method(request.method.as_deref())?;

    if let Some(body) = &request.body
        && body.len() > MAX_REQUEST_BYTES
    {
        return Err(crate::ErrorKind::InputError(format!(
            "Plugin request body exceeds {} MiB",
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
            "Plugin response exceeds {} MiB",
            MAX_RESPONSE_BYTES / (1024 * 1024)
        ))
        .into());
    }

    Ok(PluginFetchResponse {
        status: status.as_u16(),
        ok: status.is_success(),
        headers,
        body: String::from_utf8_lossy(&bytes).into_owned(),
    })
}

/// Whether any of the plugin's granted `network:` permissions covers `host`.
pub(super) async fn ensure_host_granted(
    plugin_id: &str,
    host: &str,
) -> crate::Result<()> {
    let summary = super::store::get(plugin_id).await?.ok_or_else(|| {
        crate::ErrorKind::InputError(format!("Unknown plugin '{plugin_id}'"))
    })?;
    if !summary.enabled {
        return Err(crate::ErrorKind::InputError(format!(
            "Plugin '{plugin_id}' is disabled"
        ))
        .into());
    }

    let matches = summary.granted.iter().any(|entry| {
        PluginPermission::parse(entry).is_ok_and(|permission| {
            permission.kind == super::permissions::PermissionKind::Network
                && permission
                    .scope
                    .as_deref()
                    .is_some_and(|scope| host_matches(scope, host))
        })
    });

    if !matches {
        return Err(crate::ErrorKind::InputError(format!(
            "Plugin '{plugin_id}' has not been granted network access to \
             '{host}'"
        ))
        .into());
    }
    Ok(())
}

/// Match a granted host pattern against a request host.
///
/// `example.com` matches only that host. `*.example.com` matches any single
/// additional label (`api.example.com`) but not the bare domain and not a
/// deeper subdomain, which keeps a wildcard grant from quietly widening.
fn host_matches(pattern: &str, host: &str) -> bool {
    let pattern = pattern.split(':').next().unwrap_or(pattern);
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

/// Headers a plugin must not set: they govern the connection itself, and letting
/// a plugin forge them (a `Host` for another site, say) is a request-smuggling
/// foothold.
fn is_forbidden_header(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    matches!(
        name.as_str(),
        "host"
            | "content-length"
            | "connection"
            | "transfer-encoding"
            | "cookie"
            | "authorization"
    ) || name.starts_with("proxy-")
        || name.starts_with("sec-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_host_matches_only_itself() {
        assert!(host_matches("example.com", "example.com"));
        assert!(host_matches("example.com", "EXAMPLE.com"));
        assert!(!host_matches("example.com", "api.example.com"));
        assert!(!host_matches("example.com", "evil.com"));
    }

    #[test]
    fn wildcard_matches_one_label_only() {
        assert!(host_matches("*.example.com", "api.example.com"));
        assert!(!host_matches("*.example.com", "example.com"));
        assert!(!host_matches("*.example.com", "a.b.example.com"));
        assert!(!host_matches("*.example.com", "example.com.evil.com"));
    }

    #[test]
    fn a_port_in_the_pattern_is_ignored_for_matching() {
        assert!(host_matches("example.com:8080", "example.com"));
    }

    #[test]
    fn forbidden_headers_are_recognised() {
        assert!(is_forbidden_header("Host"));
        assert!(is_forbidden_header("authorization"));
        assert!(is_forbidden_header("Sec-Fetch-Mode"));
        assert!(is_forbidden_header("proxy-connection"));
        assert!(!is_forbidden_header("Accept"));
        assert!(!is_forbidden_header("X-Api-Key"));
    }
}
