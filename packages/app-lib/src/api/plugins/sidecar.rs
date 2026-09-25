//! The sidecar capability: download, verify, run, and talk to a native binary
//! on a plugin's behalf.
//!
//! This is the single most dangerous thing a plugin can do — it runs native
//! code outside the webview sandbox — so every entry point is gated on the
//! `sidecar` permission, and the download URL is additionally checked against
//! the plugin's `network:` grants so the host it fetches from shows up in the
//! permission list the user approved.
//!
//! The launcher owns the process: children are tracked per plugin, killed when
//! the plugin unloads and when the app exits, and controlled only over
//! `127.0.0.1` with a proxy-bypassing client (a sidecar's local port must never
//! be routed through a system proxy).

use super::store;
use crate::util::fetch::REQWEST_CLIENT;
use crate::util::io;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha512};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

/// A sidecar bundle is a small native tool; the cap only stops a hostile URL
/// from streaming an unbounded download into memory.
const MAX_ARCHIVE_BYTES: usize = 128 * 1024 * 1024;
/// Cap on a control response, mirroring the network proxy.
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
/// Placeholder in a sidecar's argument list that the launcher rewrites to the
/// path of a launcher-managed port file (see [`start`]).
const PORT_FILE_TOKEN: &str = "{{PORT_FILE}}";

struct SidecarProcess {
    handle: u64,
    child: Child,
    port: Option<u16>,
}

static REGISTRY: LazyLock<Mutex<HashMap<String, Vec<SidecarProcess>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);

/// A client that ignores any configured proxy: sidecar control talks to
/// 127.0.0.1, which must never be routed through one.
static LOOPBACK_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("failed to build sidecar loopback client")
});

#[derive(Debug, Serialize)]
pub struct SidecarStarted {
    pub handle: u64,
    pub port: Option<u16>,
}

#[derive(Debug, Deserialize)]
pub struct SidecarRequest {
    /// Path plus query string, built by the plugin. Used verbatim, so the
    /// plugin can send repeated query keys (Terracotta needs `public_nodes`
    /// more than once) that a map could not express.
    pub path: String,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub headers: Option<HashMap<String, String>>,
    #[serde(default)]
    pub body: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SidecarResponse {
    pub status: u16,
    pub ok: bool,
    pub body: String,
}

#[derive(Deserialize)]
struct PortFile {
    port: u16,
}

/// Reject a key that could escape the plugin's sidecar directory.
fn checked_key(key: &str) -> crate::Result<&str> {
    let ok = !key.is_empty()
        && key != ".."
        && !key.contains(['/', '\\'])
        && key.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || matches!(character, '.' | '-' | '_')
        });
    if !ok {
        return Err(crate::ErrorKind::InputError(format!(
            "Invalid sidecar key '{key}'"
        ))
        .into());
    }
    Ok(key)
}

async fn sidecar_dir(plugin_id: &str, key: &str) -> crate::Result<PathBuf> {
    let state = crate::State::get().await?;
    let base = store::plugin_data_dir(&state, plugin_id)?;
    Ok(base.join("sidecars").join(checked_key(key)?))
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
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

/// Download, verify, and unpack a sidecar binary into the plugin's data dir.
///
/// `archive` is `"tar.gz"`, `"zip"`, or `"none"` (a raw executable). If a
/// `sha512` (or a `sha512_url` pointing at a `.sha512` sidecar file) is given
/// the download is verified; a missing sidecar hash file only warns, matching
/// Terracotta's own behaviour. Idempotent: a matching URL already installed is
/// a no-op.
pub async fn ensure(
    plugin_id: &str,
    key: &str,
    url: &str,
    sha512: Option<&str>,
    sha512_url: Option<&str>,
    archive: &str,
    version: Option<&str>,
) -> crate::Result<()> {
    store::ensure_granted(plugin_id, "sidecar").await?;

    let parsed = url::Url::parse(url).map_err(|error| {
        crate::ErrorKind::InputError(format!("Invalid sidecar URL: {error}"))
    })?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(crate::ErrorKind::InputError(
            "Sidecar downloads must be http or https".to_string(),
        )
        .into());
    }
    let host = parsed.host_str().ok_or_else(|| {
        crate::ErrorKind::InputError("Sidecar URL has no host".to_string())
    })?;
    super::network::ensure_host_granted(plugin_id, host).await?;

    let dir = sidecar_dir(plugin_id, key).await?;
    let source_marker = dir.join(".source");
    let exe_marker = dir.join(".executable");
    let version_marker = dir.join(".version");
    if io::metadata(&exe_marker).await.is_ok()
        && let Ok(prev) = io::read(&source_marker).await
        && prev[..] == *url.as_bytes()
    {
        return Ok(());
    }

    let response = REQWEST_CLIENT.get(url).send().await?;
    if !response.status().is_success() {
        return Err(crate::ErrorKind::InputError(format!(
            "Sidecar download failed: HTTP {}",
            response.status()
        ))
        .into());
    }
    let bytes = response.bytes().await?;
    if bytes.len() > MAX_ARCHIVE_BYTES {
        return Err(crate::ErrorKind::InputError(format!(
            "Sidecar download exceeds {} MiB",
            MAX_ARCHIVE_BYTES / (1024 * 1024)
        ))
        .into());
    }

    verify_sha512(plugin_id, &bytes, sha512, sha512_url).await?;

    let filename = parsed
        .path_segments()
        .and_then(|segments| segments.last())
        .filter(|segment| !segment.is_empty())
        .unwrap_or("sidecar")
        .to_string();
    let dir_for_extract = dir.clone();
    let archive = archive.to_string();
    let bytes_vec = bytes.to_vec();
    let exe_rel = tokio::task::spawn_blocking(move || {
        extract(&bytes_vec, &dir_for_extract, &archive, &filename)
    })
    .await??;

    io::write(&source_marker, url.as_bytes().to_vec()).await?;
    io::write(
        &exe_marker,
        exe_rel.to_string_lossy().as_bytes().to_vec(),
    )
    .await?;
    if let Some(version) = version {
        io::write(&version_marker, version.as_bytes().to_vec()).await?;
    }
    tracing::info!(plugin = %plugin_id, key = %key, "Sidecar ready");
    Ok(())
}

/// Whether a sidecar is installed, and the version recorded when it was.
#[derive(Debug, Serialize)]
pub struct SidecarStatus {
    pub installed: bool,
    pub version: Option<String>,
}

/// Report whether `key` is installed for the plugin and its recorded version.
pub async fn status(plugin_id: &str, key: &str) -> crate::Result<SidecarStatus> {
    store::ensure_granted(plugin_id, "sidecar").await?;
    let dir = sidecar_dir(plugin_id, key).await?;
    let installed = io::metadata(&dir.join(".executable")).await.is_ok();
    let version = if installed {
        io::read(&dir.join(".version"))
            .await
            .ok()
            .map(|bytes| String::from_utf8_lossy(&bytes).trim().to_string())
            .filter(|value| !value.is_empty())
    } else {
        None
    };
    Ok(SidecarStatus { installed, version })
}

async fn verify_sha512(
    plugin_id: &str,
    bytes: &[u8],
    sha512: Option<&str>,
    sha512_url: Option<&str>,
) -> crate::Result<()> {
    let expected = if let Some(hex) = sha512 {
        Some(hex.trim().to_ascii_lowercase())
    } else if let Some(hash_url) = sha512_url {
        match REQWEST_CLIENT.get(hash_url).send().await {
            Ok(response) if response.status().is_success() => response
                .text()
                .await
                .ok()
                .and_then(|text| {
                    text.split_whitespace()
                        .next()
                        .map(|token| token.to_ascii_lowercase())
                }),
            _ => {
                tracing::warn!(
                    plugin = %plugin_id,
                    "Sidecar .sha512 file unavailable; skipping verification"
                );
                None
            }
        }
    } else {
        None
    };

    if let Some(expected) = expected {
        let mut hasher = Sha512::new();
        hasher.update(bytes);
        if hex_encode(&hasher.finalize()) != expected {
            return Err(crate::ErrorKind::InputError(
                "Sidecar SHA-512 does not match the expected hash".to_string(),
            )
            .into());
        }
    }
    Ok(())
}

/// Join an archive-relative path onto `base`, rejecting any component that
/// would climb out (path traversal in a hostile archive).
fn safe_join(base: &Path, relative: &Path) -> Option<PathBuf> {
    let mut out = base.to_path_buf();
    for component in relative.components() {
        match component {
            std::path::Component::Normal(part) => out.push(part),
            std::path::Component::CurDir => {}
            _ => return None,
        }
    }
    Some(out)
}

/// Unpack a downloaded bundle into `dir` and return the executable's path
/// relative to `dir`. Runs on a blocking thread — all synchronous IO.
fn extract(
    bytes: &[u8],
    dir: &Path,
    archive: &str,
    filename: &str,
) -> crate::Result<PathBuf> {
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    std::fs::create_dir_all(dir)?;

    match archive {
        "tar.gz" | "tgz" => {
            let decoder =
                flate2::read::GzDecoder::new(std::io::Cursor::new(bytes));
            let mut tar = tar::Archive::new(decoder);
            for entry in tar.entries()? {
                let mut entry = entry?;
                let raw = entry.path()?.into_owned();
                let Some(out_path) = safe_join(dir, &raw) else {
                    continue;
                };
                if entry.header().entry_type().is_dir() {
                    std::fs::create_dir_all(&out_path)?;
                    continue;
                }
                if let Some(parent) = out_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let mut out = std::fs::File::create(&out_path)?;
                std::io::copy(&mut entry, &mut out)?;
            }
        }
        "zip" => {
            let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))
                .map_err(|error| {
                    crate::ErrorKind::InputError(format!(
                        "Invalid sidecar zip: {error}"
                    ))
                })?;
            for index in 0..zip.len() {
                let mut file = zip.by_index(index).map_err(|error| {
                    crate::ErrorKind::InputError(format!(
                        "Bad zip entry: {error}"
                    ))
                })?;
                let Some(name) = file.enclosed_name() else {
                    continue;
                };
                let out_path = dir.join(&name);
                if file.is_dir() {
                    std::fs::create_dir_all(&out_path)?;
                    continue;
                }
                if let Some(parent) = out_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let mut out = std::fs::File::create(&out_path)?;
                std::io::copy(&mut file, &mut out)?;
            }
        }
        _ => {
            std::fs::write(dir.join(filename), bytes)?;
        }
    }

    let executable = find_executable(dir)?.ok_or_else(|| {
        crate::ErrorKind::InputError(
            "No executable found in the sidecar bundle".to_string(),
        )
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&executable)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&executable, perms)?;
    }
    Ok(executable.strip_prefix(dir).unwrap_or(&executable).to_path_buf())
}

/// Pick the executable out of an unpacked bundle: on Windows an `.exe` wins,
/// otherwise the first file whose leading bytes are a known executable format.
fn find_executable(dir: &Path) -> crate::Result<Option<PathBuf>> {
    let mut files = Vec::new();
    collect_files(dir, &mut files)?;

    #[cfg(windows)]
    if let Some(exe) = files.iter().find(|path| {
        path.extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
    }) {
        return Ok(Some(exe.clone()));
    }

    for path in &files {
        if is_executable_magic(path)? {
            return Ok(Some(path.clone()));
        }
    }
    Ok(None)
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) -> crate::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

/// Whether a file starts with the magic bytes of a native executable
/// (PE `MZ`, ELF, or a Mach-O / fat-binary header).
fn is_executable_magic(path: &Path) -> crate::Result<bool> {
    let mut file = std::fs::File::open(path)?;
    let mut magic = [0u8; 4];
    use std::io::Read;
    let read = file.read(&mut magic)?;
    if read < 4 {
        return Ok(false);
    }
    Ok(matches!(
        magic,
        [0x4D, 0x5A, ..]                 // MZ (PE)
            | [0x7F, 0x45, 0x4C, 0x46]   // ELF
            | [0xFE, 0xED, 0xFA, 0xCE]   // Mach-O 32 BE
            | [0xCE, 0xFA, 0xED, 0xFE]   // Mach-O 32 LE
            | [0xFE, 0xED, 0xFA, 0xCF]   // Mach-O 64 BE
            | [0xCF, 0xFA, 0xED, 0xFE]   // Mach-O 64 LE
            | [0xCA, 0xFE, 0xBA, 0xBE]   // Mach-O fat
    ))
}

/// Spawn an installed sidecar. If `port_file` is set, the launcher creates a
/// port-file path, substitutes it for the `{{PORT_FILE}}` token in `args`, and
/// polls that JSON file (`{"port":N}`) for the port the sidecar reports —
/// Terracotta's `--hmcl` convention.
pub async fn start(
    plugin_id: &str,
    key: &str,
    mut args: Vec<String>,
    port_file: bool,
) -> crate::Result<SidecarStarted> {
    store::ensure_granted(plugin_id, "sidecar").await?;

    let dir = sidecar_dir(plugin_id, key).await?;
    let exe_rel = io::read(&dir.join(".executable")).await.map_err(|_| {
        crate::ErrorKind::InputError(format!(
            "Sidecar '{key}' is not installed"
        ))
    })?;
    let executable =
        dir.join(String::from_utf8_lossy(&exe_rel).trim().to_string());

    let mut port_path = None;
    if port_file {
        let path = dir.join(format!("port-{}.json", uuid::Uuid::new_v4()));
        for arg in args.iter_mut() {
            if arg.contains(PORT_FILE_TOKEN) {
                *arg = arg.replace(PORT_FILE_TOKEN, &path.to_string_lossy());
            }
        }
        port_path = Some(path);
    }

    let mut command = Command::new(&executable);
    command
        .args(&args)
        .current_dir(&dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

    let mut child = command.spawn().map_err(|error| {
        crate::ErrorKind::InputError(format!(
            "Failed to start sidecar '{key}': {error}"
        ))
    })?;

    if let Some(stdout) = child.stdout.take() {
        drain(plugin_id.to_string(), "stdout", stdout);
    }
    if let Some(stderr) = child.stderr.take() {
        drain(plugin_id.to_string(), "stderr", stderr);
    }

    let mut port = None;
    if let Some(path) = &port_path {
        port = poll_port_file(path).await;
    }

    let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
    REGISTRY
        .lock()
        .await
        .entry(plugin_id.to_string())
        .or_default()
        .push(SidecarProcess {
            handle,
            child,
            port,
        });
    Ok(SidecarStarted { handle, port })
}

/// Drain a child stream to the log so a full pipe never blocks the sidecar.
fn drain<R>(plugin_id: String, stream: &'static str, reader: R)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        use tokio::io::AsyncBufReadExt;
        let mut lines = tokio::io::BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            tracing::debug!(plugin = %plugin_id, stream, "{line}");
        }
    });
}

async fn poll_port_file(path: &Path) -> Option<u16> {
    for _ in 0..60 {
        if let Ok(bytes) = io::read(path).await
            && let Ok(parsed) = serde_json::from_slice::<PortFile>(&bytes)
        {
            return Some(parsed.port);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    None
}

/// Proxy a control request to a running sidecar over `127.0.0.1:{port}`.
pub async fn request(
    plugin_id: &str,
    handle: u64,
    request: SidecarRequest,
) -> crate::Result<SidecarResponse> {
    store::ensure_granted(plugin_id, "sidecar").await?;

    let port = {
        let registry = REGISTRY.lock().await;
        registry
            .get(plugin_id)
            .and_then(|list| list.iter().find(|proc| proc.handle == handle))
            .and_then(|proc| proc.port)
    }
    .ok_or_else(|| {
        crate::ErrorKind::InputError(format!(
            "Sidecar handle {handle} is not running or reported no port"
        ))
    })?;

    let path = if request.path.starts_with('/') {
        request.path.clone()
    } else {
        format!("/{}", request.path)
    };
    let target = format!("http://127.0.0.1:{port}{path}");

    let method = parse_method(request.method.as_deref())?;
    let mut builder = LOOPBACK_CLIENT.request(method, target);
    if let Some(headers) = request.headers {
        for (name, value) in headers {
            builder = builder.header(name, value);
        }
    }
    if let Some(body) = request.body {
        builder = builder.body(body);
    }

    let response = builder.send().await?;
    let status = response.status();
    let bytes = response.bytes().await?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(crate::ErrorKind::InputError(format!(
            "Sidecar response exceeds {} MiB",
            MAX_RESPONSE_BYTES / (1024 * 1024)
        ))
        .into());
    }

    Ok(SidecarResponse {
        status: status.as_u16(),
        ok: status.is_success(),
        body: String::from_utf8_lossy(&bytes).into_owned(),
    })
}

/// Stop one sidecar of a plugin.
pub async fn stop(plugin_id: &str, handle: u64) -> crate::Result<()> {
    let mut registry = REGISTRY.lock().await;
    if let Some(list) = registry.get_mut(plugin_id)
        && let Some(index) = list.iter().position(|proc| proc.handle == handle)
    {
        let mut proc = list.remove(index);
        let _ = proc.child.start_kill();
    }
    Ok(())
}

/// Kill every sidecar a plugin started — called when it unloads or is disabled.
pub async fn cleanup(plugin_id: &str) {
    let mut registry = REGISTRY.lock().await;
    if let Some(mut list) = registry.remove(plugin_id) {
        for proc in list.iter_mut() {
            let _ = proc.child.start_kill();
        }
    }
}

/// Kill every sidecar of every plugin — called on app shutdown.
pub async fn shutdown_all() {
    let mut registry = REGISTRY.lock().await;
    for (_, mut list) in registry.drain() {
        for proc in list.iter_mut() {
            let _ = proc.child.start_kill();
        }
    }
}







