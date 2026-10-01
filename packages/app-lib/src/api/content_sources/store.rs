//! Where content sources live, what state they carry, and the operations that
//! change that state: scan, install, uninstall, enable.
//!
//! Layout, under the launcher's settings directory:
//!
//! ```text
//! settings_dir/
//!   content-sources.json     ← registry: enabled flag per source
//!   content-sources/<id>/    ← the source itself, folder name == manifest id
//!   content-source-data/<id>/ ← writable scratch space handed to the source
//! ```
//!
//! This mirrors the plugin layout but is its own tree: a content source and a
//! launcher plugin are different kinds of thing, and keeping their folders and
//! registries apart means neither can shadow or corrupt the other.

use super::manifest::{
    ContentSourceManifest, MANIFEST_FILE, is_valid_source_id,
};
use crate::State;
use crate::util::io;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const SOURCES_DIR: &str = "content-sources";
const SOURCE_DATA_DIR: &str = "content-source-data";
const STATE_FILE: &str = "content-sources.json";

/// An entry bundle is a compiled source, so it is small by nature. The cap only
/// exists so a hostile folder cannot make the loader read a huge file into
/// memory before it ever gets a chance to fail validation.
const MAX_ENTRY_BYTES: u64 = 8 * 1024 * 1024;

/// Cap on a downloaded source archive. A source bundle is small; an unbounded
/// download from a store entry is a way to fill the disk.
const MAX_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;

pub fn content_sources_dir(state: &State) -> PathBuf {
    state.directories.settings_dir.join(SOURCES_DIR)
}

/// The scratch space handed to a source. Validated, because the id comes from
/// the frontend and would otherwise be a path-traversal primitive.
pub fn source_data_dir(
    state: &State,
    source_id: &str,
) -> crate::Result<PathBuf> {
    Ok(state
        .directories
        .settings_dir
        .join(SOURCE_DATA_DIR)
        .join(checked_id(source_id)?))
}

fn state_file(state: &State) -> PathBuf {
    state.directories.settings_dir.join(STATE_FILE)
}

/// Reject an id that could name anything other than a direct child of the
/// content-sources directory.
fn checked_id(source_id: &str) -> crate::Result<&str> {
    if !is_valid_source_id(source_id) {
        return Err(crate::ErrorKind::InputError(format!(
            "Invalid content source id '{source_id}'"
        ))
        .into());
    }
    Ok(source_id)
}

fn source_dir(state: &State, source_id: &str) -> crate::Result<PathBuf> {
    Ok(content_sources_dir(state).join(checked_id(source_id)?))
}

/// What the launcher remembers about a source between runs.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SourceRecord {
    pub id: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub installed_at: Option<i64>,
    /// Where the source came from, when it was installed by the launcher.
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct SourceStateFile {
    #[serde(default)]
    sources: Vec<SourceRecord>,
}

/// A source as the source manager sees it: the manifest, plus whether it is on.
///
/// `manifest` is `None` for a folder that exists but whose manifest could not
/// be read; `error` then says why. Those still have to be listed — otherwise a
/// broken source would be invisible and impossible to remove from the UI.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceSummary {
    /// The manifest id, or the folder name when the manifest is unusable.
    pub id: String,
    pub dir_name: String,
    pub path: String,
    pub enabled: bool,
    pub manifest: Option<ContentSourceManifest>,
    pub error: Option<String>,
}

impl SourceSummary {
    /// Whether this source may be loaded at all.
    pub fn loadable(&self) -> bool {
        self.manifest.is_some() && self.enabled
    }
}

/// Every source folder on disk, as a summary.
pub async fn list() -> crate::Result<Vec<SourceSummary>> {
    let state = State::get().await?;
    let root = content_sources_dir(&state);
    if !root.is_dir() {
        return Ok(Vec::new());
    }

    let registry = read_state(&state).await?;
    let mut entries = io::read_dir(&root).await?;
    let mut summaries = Vec::new();

    while let Some(entry) = entries.next_entry().await? {
        if !entry.file_type().await?.is_dir() {
            continue;
        }
        let dir_name = entry.file_name().to_string_lossy().into_owned();
        // A staging folder from an interrupted install is not a source.
        if dir_name.starts_with(".staging-") {
            continue;
        }
        summaries.push(summarize(
            &entry.path(),
            dir_name,
            registry.sources.as_slice(),
        ));
    }

    summaries.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(summaries)
}

pub async fn get(source_id: &str) -> crate::Result<Option<SourceSummary>> {
    let state = State::get().await?;
    let dir = source_dir(&state, source_id)?;
    if !dir.is_dir() {
        return Ok(None);
    }
    let registry = read_state(&state).await?;
    Ok(Some(summarize(
        &dir,
        source_id.to_string(),
        registry.sources.as_slice(),
    )))
}

/// Copy a source folder into the content-sources directory.
///
/// `source` must be a directory holding a valid `manifest.json`; the folder is
/// placed under the manifest's id, which is also the invariant `list` relies on
/// to find it again.
pub async fn install(
    source: &Path,
    origin: Option<String>,
) -> crate::Result<SourceSummary> {
    let state = State::get().await?;
    let manifest = ContentSourceManifest::read_from_dir(source)?;
    let destination = source_dir(&state, &manifest.id)?;

    if destination.exists() {
        return Err(crate::ErrorKind::InputError(format!(
            "Content source '{}' is already installed",
            manifest.id
        ))
        .into());
    }

    io::create_dir_all(&destination).await?;
    if let Err(error) = copy_dir(source, &destination).await {
        // A half-copied folder would be picked up by the next scan as a broken
        // source, so clean it up rather than leaving the debris behind.
        let _ = io::remove_dir_all(&destination).await;
        return Err(error);
    }

    let mut registry = read_state(&state).await?;
    registry.sources.retain(|record| record.id != manifest.id);
    registry.sources.push(SourceRecord {
        id: manifest.id.clone(),
        enabled: true,
        installed_at: Some(chrono::Utc::now().timestamp()),
        source: origin,
    });
    write_state(&state, &registry).await?;

    tracing::info!(source = %manifest.id, "Installed content source");

    Ok(summarize(
        &destination,
        manifest.id.clone(),
        registry.sources.as_slice(),
    ))
}

/// Download a zipped source from a store entry, unpack it, and install it.
///
/// The archive is untrusted: entries are checked for path traversal, the total
/// size is capped, and the unpacked folder still goes through the same manifest
/// validation as any other install. `origin` is recorded as the source's origin.
pub async fn install_from_url(
    url: &str,
    origin: Option<String>,
) -> crate::Result<SourceSummary> {
    let parsed = url::Url::parse(url).map_err(|error| {
        crate::ErrorKind::InputError(format!("Invalid download URL: {error}"))
    })?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(crate::ErrorKind::InputError(
            "Content source downloads must be http or https".to_string(),
        )
        .into());
    }

    let state = State::get().await?;
    let bytes = crate::util::fetch::fetch(
        url,
        None,
        None,
        None,
        &state.fetch_semaphore,
        &state.pool,
    )
    .await?;

    let staging = content_sources_dir(&state)
        .join(format!(".staging-{}", uuid::Uuid::new_v4()));
    io::create_dir_all(&staging).await?;

    let result =
        unpack_and_install(&bytes, &staging, origin.or_else(|| Some(url.to_string())))
            .await;
    let _ = io::remove_dir_all(&staging).await;
    result
}

/// Extract a source zip into `staging`, then install whichever folder holds the
/// manifest.
async fn unpack_and_install(
    bytes: &[u8],
    staging: &Path,
    origin: Option<String>,
) -> crate::Result<SourceSummary> {
    let bytes = bytes.to_vec();
    let target = staging.to_path_buf();

    // Zip reading is synchronous CPU/IO work, so keep it off the async runtime.
    tokio::task::spawn_blocking(move || extract_zip(&bytes, &target)).await??;

    // The manifest may be at the archive root or one level down (a zip made from
    // a folder). Find it rather than assuming a layout.
    let manifest_dir = find_manifest_dir(staging).await?.ok_or_else(|| {
        crate::ErrorKind::InputError(
            "The downloaded archive has no manifest.json".to_string(),
        )
    })?;

    install(&manifest_dir, origin).await
}

/// Replace an already-installed source's files from `source`, keeping its
/// enabled state.
pub async fn update(
    source: &Path,
    origin: Option<String>,
) -> crate::Result<SourceSummary> {
    let state = State::get().await?;
    let manifest = ContentSourceManifest::read_from_dir(source)?;
    let destination = source_dir(&state, &manifest.id)?;

    if !destination.exists() {
        return Err(crate::ErrorKind::InputError(format!(
            "Content source '{}' is not installed, so there is nothing to update",
            manifest.id
        ))
        .into());
    }

    // Swap the folder wholesale: the bundle (index.js + manifest.json) lives
    // here, while source data sits in a separate directory, so replacing this
    // folder keeps user state intact.
    io::remove_dir_all(&destination).await?;
    io::create_dir_all(&destination).await?;
    if let Err(error) = copy_dir(source, &destination).await {
        let _ = io::remove_dir_all(&destination).await;
        return Err(error);
    }

    let mut registry = read_state(&state).await?;
    if let Some(record) =
        registry.sources.iter_mut().find(|record| record.id == manifest.id)
    {
        if origin.is_some() {
            record.source = origin;
        }
    }
    write_state(&state, &registry).await?;

    tracing::info!(source = %manifest.id, "Updated content source");

    Ok(summarize(
        &destination,
        manifest.id.clone(),
        registry.sources.as_slice(),
    ))
}

/// Download and update a source from a store entry.
pub async fn update_from_url(
    url: &str,
    origin: Option<String>,
) -> crate::Result<SourceSummary> {
    let parsed = url::Url::parse(url).map_err(|error| {
        crate::ErrorKind::InputError(format!("Invalid download URL: {error}"))
    })?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(crate::ErrorKind::InputError(
            "Content source downloads must be http or https".to_string(),
        )
        .into());
    }

    let state = State::get().await?;
    let bytes = crate::util::fetch::fetch(
        url,
        None,
        None,
        None,
        &state.fetch_semaphore,
        &state.pool,
    )
    .await?;

    let staging = content_sources_dir(&state)
        .join(format!(".staging-{}", uuid::Uuid::new_v4()));
    io::create_dir_all(&staging).await?;

    let bytes = bytes.to_vec();
    let target = staging.clone();
    let result = async {
        tokio::task::spawn_blocking(move || extract_zip(&bytes, &target))
            .await??;
        let manifest_dir = find_manifest_dir(&staging).await?.ok_or_else(|| {
            crate::ErrorKind::InputError(
                "The downloaded archive has no manifest.json".to_string(),
            )
        })?;
        update(&manifest_dir, origin.or_else(|| Some(url.to_string()))).await
    }
    .await;
    let _ = io::remove_dir_all(&staging).await;
    result
}

/// Synchronously extract a zip into `dest`, rejecting any entry that would
/// escape it.
fn extract_zip(bytes: &[u8], dest: &Path) -> crate::Result<()> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|error| {
            crate::ErrorKind::InputError(format!(
                "Invalid content source archive: {error}"
            ))
        })?;

    let mut total: u64 = 0;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| {
            crate::ErrorKind::InputError(format!("Bad archive entry: {error}"))
        })?;

        // `enclosed_name` returns None for anything with `..` or an absolute
        // path — exactly the entries that would write outside `dest`.
        let Some(relative) = entry.enclosed_name() else {
            return Err(crate::ErrorKind::InputError(format!(
                "Archive entry '{}' has an unsafe path",
                entry.name()
            ))
            .into());
        };
        let out_path = dest.join(&relative);

        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)?;
            continue;
        }

        total = total.saturating_add(entry.size());
        if total > MAX_ARCHIVE_BYTES {
            return Err(crate::ErrorKind::InputError(format!(
                "Content source archive is larger than {} MiB",
                MAX_ARCHIVE_BYTES / (1024 * 1024)
            ))
            .into());
        }

        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&out_path)?;
        std::io::copy(&mut entry, &mut out)?;
    }

    Ok(())
}

/// Locate the folder containing `manifest.json`: the extraction root, or a
/// single top-level subfolder (a zip built from a source directory).
async fn find_manifest_dir(root: &Path) -> crate::Result<Option<PathBuf>> {
    if root.join(MANIFEST_FILE).is_file() {
        return Ok(Some(root.to_path_buf()));
    }

    let mut entries = io::read_dir(root).await?;
    let mut only_dir: Option<PathBuf> = None;
    let mut dir_count = 0;
    while let Some(entry) = entries.next_entry().await? {
        if entry.file_type().await?.is_dir() {
            dir_count += 1;
            only_dir = Some(entry.path());
        }
    }

    if dir_count == 1
        && let Some(dir) = only_dir
        && dir.join(MANIFEST_FILE).is_file()
    {
        return Ok(Some(dir));
    }
    Ok(None)
}

pub async fn uninstall(
    source_id: &str,
    remove_data: bool,
) -> crate::Result<()> {
    let state = State::get().await?;
    let dir = source_dir(&state, source_id)?;
    if dir.is_dir() {
        io::remove_dir_all(&dir).await?;
    }

    if remove_data {
        let data = source_data_dir(&state, source_id)?;
        if data.is_dir() {
            io::remove_dir_all(&data).await?;
        }
    }

    let mut registry = read_state(&state).await?;
    registry.sources.retain(|record| record.id != source_id);
    write_state(&state, &registry).await?;

    tracing::info!(source = %source_id, "Uninstalled content source");
    Ok(())
}

pub async fn set_enabled(
    source_id: &str,
    enabled: bool,
) -> crate::Result<SourceSummary> {
    let state = State::get().await?;
    // The source has to exist before its state can be changed.
    if !source_dir(&state, source_id)?.is_dir() {
        return Err(crate::ErrorKind::InputError(format!(
            "Unknown content source '{source_id}'"
        ))
        .into());
    }

    let mut registry = read_state(&state).await?;
    record_mut(&mut registry, source_id).enabled = enabled;
    write_state(&state, &registry).await?;

    let dir = source_dir(&state, source_id)?;
    Ok(summarize(&dir, source_id.to_string(), registry.sources.as_slice()))
}

/// Resolve a source's entry file path, if it is installed and loadable.
pub async fn resolve_entry(
    source_id: &str,
) -> crate::Result<Option<PathBuf>> {
    let Some(summary) = get(source_id).await? else {
        return Ok(None);
    };
    let Some(manifest) = summary.manifest else {
        return Ok(None);
    };
    let path = Path::new(&summary.path).join(&manifest.entry);
    if path.is_file() {
        Ok(Some(path))
    } else {
        Ok(None)
    }
}

/// Read a source's entry bundle.
///
/// The loader prefers to import the file straight over the asset protocol, but
/// that is a cross-origin module load and can be refused; this is the fallback,
/// which hands back the source so the loader can import it from a blob URL
/// instead.
pub async fn read_entry(source_id: &str) -> crate::Result<Option<String>> {
    let Some(path) = resolve_entry(source_id).await? else {
        return Ok(None);
    };
    let size = io::metadata(&path).await?.len();
    if size > MAX_ENTRY_BYTES {
        return Err(crate::ErrorKind::InputError(format!(
            "Content source '{source_id}' has an entry bundle of {size} bytes, \
             which is over the {} MiB limit",
            MAX_ENTRY_BYTES / (1024 * 1024)
        ))
        .into());
    }
    let bytes = io::read(&path).await?;
    Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
}

/// A client that does not follow redirects, so [`latest_release_tag`] can read
/// the `Location` header GitHub returns for `releases/latest` rather than
/// downloading the release page it points at.
static NO_REDIRECT_CLIENT: std::sync::LazyLock<reqwest::Client> =
    std::sync::LazyLock::new(|| {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(std::time::Duration::from_secs(15))
            .read_timeout(std::time::Duration::from_secs(30))
            .user_agent(crate::launcher_user_agent())
            .build()
            .expect("client configuration should be valid")
    });

/// The tag of a GitHub repository's latest published release, e.g. `v1.2.0`.
///
/// Reads the redirect `github.com/<owner>/<repo>/releases/latest` issues to the
/// concrete tag page, not the REST API, so it does not count against the API's
/// unauthenticated hourly rate limit. `None` means the repo has no published
/// release, or the request failed — update detection then shows nothing, the
/// right silent default for a background check.
pub async fn latest_release_tag(repo: &str) -> crate::Result<Option<String>> {
    if !is_valid_owner_repo(repo) {
        return Err(crate::ErrorKind::InputError(format!(
            "Invalid repository '{repo}': expected 'owner/name'"
        ))
        .into());
    }

    let url = format!("https://github.com/{repo}/releases/latest");
    let response = match NO_REDIRECT_CLIENT.get(&url).send().await {
        Ok(response) => response,
        Err(error) => {
            tracing::debug!(repo, "Latest-release check failed: {error}");
            return Ok(None);
        }
    };

    // A published release redirects to `.../releases/tag/<tag>`; a repo with no
    // release redirects to the releases index, which has no `/tag/` segment.
    let location = response
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|value| value.to_str().ok());
    Ok(location.and_then(tag_from_location))
}

/// Whether `repo` is a plain `owner/name` pair safe to interpolate into a URL.
fn is_valid_owner_repo(repo: &str) -> bool {
    let mut parts = repo.split('/');
    let (Some(owner), Some(name), None) =
        (parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    let segment_ok = |segment: &str| {
        !segment.is_empty()
            && segment.len() <= 100
            && segment.chars().all(|character| {
                character.is_ascii_alphanumeric()
                    || matches!(character, '.' | '-' | '_')
            })
    };
    segment_ok(owner) && segment_ok(name)
}

/// Pull the tag out of a `releases/tag/<tag>` redirect target, ignoring any
/// query or fragment. Works for both absolute and relative `Location` values.
fn tag_from_location(location: &str) -> Option<String> {
    let (_, tail) = location.rsplit_once("/releases/tag/")?;
    let tag = tail
        .split(['?', '#'])
        .next()
        .unwrap_or(tail)
        .trim_end_matches('/');
    (!tag.is_empty()).then(|| tag.to_string())
}

/// Build a summary for one source folder, reading its manifest best-effort.
fn summarize(
    dir: &Path,
    dir_name: String,
    records: &[SourceRecord],
) -> SourceSummary {
    let record = records.iter().find(|record| record.id == dir_name);
    match ContentSourceManifest::read_from_dir(dir) {
        Ok(manifest) => SourceSummary {
            id: manifest.id.clone(),
            dir_name,
            path: dir.to_string_lossy().into_owned(),
            enabled: record.map(|record| record.enabled).unwrap_or(true),
            manifest: Some(manifest),
            error: None,
        },
        Err(error) => SourceSummary {
            id: dir_name.clone(),
            dir_name,
            path: dir.to_string_lossy().into_owned(),
            enabled: record.map(|record| record.enabled).unwrap_or(false),
            manifest: None,
            error: Some(error.to_string()),
        },
    }
}

fn record_mut<'a>(
    registry: &'a mut SourceStateFile,
    source_id: &str,
) -> &'a mut SourceRecord {
    if let Some(index) =
        registry.sources.iter().position(|record| record.id == source_id)
    {
        return &mut registry.sources[index];
    }
    registry.sources.push(SourceRecord {
        id: source_id.to_string(),
        enabled: true,
        installed_at: Some(chrono::Utc::now().timestamp()),
        source: None,
    });
    let index = registry.sources.len() - 1;
    &mut registry.sources[index]
}

async fn read_state(state: &State) -> crate::Result<SourceStateFile> {
    let path = state_file(state);
    let bytes = match io::read(&path).await {
        Ok(bytes) => bytes,
        Err(_) => return Ok(SourceStateFile::default()),
    };
    match serde_json::from_slice(&bytes) {
        Ok(registry) => Ok(registry),
        Err(error) => {
            // Losing the registry only means sources start enabled again, a far
            // better outcome than refusing to start.
            tracing::warn!(
                path = %path.display(),
                "Content source registry is unreadable, starting from an \
                 empty one: {error}"
            );
            Ok(SourceStateFile::default())
        }
    }
}

async fn write_state(
    state: &State,
    registry: &SourceStateFile,
) -> crate::Result<()> {
    let path = state_file(state);
    if let Some(parent) = path.parent() {
        io::create_dir_all(parent).await?;
    }
    io::write(&path, serde_json::to_vec_pretty(registry)?).await?;
    Ok(())
}

async fn copy_dir(from: &Path, to: &Path) -> crate::Result<()> {
    let mut entries = io::read_dir(from).await?;
    while let Some(entry) = entries.next_entry().await? {
        let source = entry.path();
        let target = to.join(entry.file_name());
        let file_type = entry.file_type().await?;
        if file_type.is_dir() {
            io::create_dir_all(&target).await?;
            Box::pin(copy_dir(&source, &target)).await?;
        } else if file_type.is_file() {
            io::copy(&source, &target).await?;
        }
        // Symlinks are skipped: source folders are untrusted, and following one
        // could copy files from anywhere on the disk into the sources folder.
    }
    Ok(())
}
