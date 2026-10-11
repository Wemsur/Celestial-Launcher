//! Installing a file a content source produced.
//!
//! A source never writes to an instance itself. It answers `resolve_download`
//! with a {@link InstallDescriptor} — a URL, the headers the fetch needs, an
//! optional name and hash — and the launcher performs the install here. That
//! split is what keeps a source's reach to "propose content": the launcher owns
//! where a file lands and what counts as a valid archive.
//!
//! Placement is per content type, which is the launcher's business, not the
//! source's:
//!
//! - mod / resourcepack / shader / datapack go into the matching instance folder
//! - a world is unzipped into the instance's `saves/`, with the level found
//!   inside the archive rather than assumed at its root
//! - a skin lands in the launcher's own `skins/` folder, which belongs to no
//!   instance

use super::manifest::ContentType;
use super::network::is_forbidden_header;
use crate::State;
use crate::util::io;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Where locally downloaded skins live, under the launcher's settings directory.
pub const SKINS_DIR: &str = "skins";

/// Cap on one downloaded file. A world can be large, but an unbounded download
/// from third-party content is a way to fill the disk.
const MAX_DOWNLOAD_BYTES: u64 = 512 * 1024 * 1024;

/// Cap on the expanded size of a world archive, so a zip bomb cannot fill the
/// disk through decompression.
const MAX_WORLD_UNPACKED_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Cap on a pack manifest read out of a staged archive. A real CurseForge or
/// Modrinth manifest is well under a megabyte; the limit only stops a crafted
/// entry from expanding to gigabytes in memory.
const MAX_MANIFEST_BYTES: u64 = 8 * 1024 * 1024;

/// The file a content source hands the launcher to install.
///
/// camelCase on the wire: this crosses the Tauri boundary from a source's
/// `resolve_download`, whose contract is JavaScript-shaped.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallDescriptor {
    pub url: String,
    /// Headers the fetch needs (Referer, User-Agent, Cookie). Connection-level
    /// headers are dropped, the same as the network proxy does.
    #[serde(default)]
    pub headers: Option<HashMap<String, String>>,
    /// Suggested name; the URL's last segment is used when absent.
    #[serde(default)]
    pub file_name: Option<String>,
    /// Present only when the source could supply one. A source that cannot is
    /// downloaded and recorded without verification.
    #[serde(default)]
    pub hash: Option<FileHash>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct FileHash {
    pub algorithm: String,
    pub value: String,
}

/// Where a file came from, plus what the UI needs to name it. Recorded after a
/// successful install so a later update check knows what to ask about.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallMetadata {
    #[serde(default)]
    pub source_id: String,
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub version_id: Option<String>,
    #[serde(default)]
    pub version_name: Option<String>,
    /// Display name shown in the installed list; the file name is the fallback.
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub icon_url: Option<String>,
    /// Opaque rule the source matched its download with, so a later check can
    /// match the same kind of asset again.
    #[serde(default)]
    pub install_rule: Option<serde_json::Value>,
}

/// What an install produced, for the caller to report back.
#[derive(Clone, Debug, Serialize)]
pub struct InstalledContent {
    /// Where it landed: relative to the instance, or to the launcher's data
    /// directory for a skin.
    pub relative_path: String,
    pub file_name: String,
    /// The record id, when one was written (absent for a skin).
    pub record_id: Option<String>,
}

/// Where an install put a file, before it is wrapped up for the caller.
struct PlacedFile {
    relative_path: String,
    file_name: String,
}

/// Install one file into an instance (or, for a skin, into the launcher).
pub async fn install_content(
    instance_id: &str,
    content_type: ContentType,
    descriptor: InstallDescriptor,
    meta: InstallMetadata,
    state: &State,
) -> crate::Result<InstalledContent> {
    let file_name = file_name_for(&descriptor)?;
    let bytes = download(&descriptor, &meta.source_id, state).await?;

    // A skin belongs to the launcher and has no version to track, so it is not
    // recorded; everything else is.
    if content_type == ContentType::Skin {
        let placed = install_skin(&file_name, &bytes, state).await?;
        return Ok(InstalledContent {
            relative_path: placed.relative_path,
            file_name: placed.file_name,
            record_id: None,
        });
    }

    let placed = match content_type {
        ContentType::World => {
            let instance_dir = instance_dir(instance_id, state).await?;
            install_world(&instance_dir, &file_name, &bytes).await?
        }
        ContentType::Mod
        | ContentType::ResourcePack
        | ContentType::Shader
        | ContentType::DataPack => {
            let folder = folder_for(content_type).ok_or_else(|| {
                crate::ErrorKind::InputError(format!(
                    "{} is not installed as a loose file",
                    content_type.as_str()
                ))
            })?;
            let instance_dir = instance_dir(instance_id, state).await?;
            let previous = super::installed::list_installed(instance_id, state)
                .await
                .unwrap_or_default()
                .into_iter()
                .find(|record| {
                    record.content_type == content_type
                        && record.source_id == meta.source_id
                        && record.project_id == meta.project_id
                });
            let placed =
                install_loose_file(&instance_dir, folder, &file_name, &bytes).await?;
            supersede_previous_file(&instance_dir, folder, previous.as_ref(), &placed)
                .await;
            placed
        }
        ContentType::Modpack => {
            return Err(crate::ErrorKind::InputError(
                "Modpacks are installed through the modpack installer, not a \
                 single-file install"
                    .to_string(),
            )
            .into());
        }
        ContentType::Skin => unreachable!("skins are handled above"),
    };

    let record = super::installed::SourceContentRecord {
        id: String::new(),
        source_id: meta.source_id,
        project_id: meta.project_id,
        version_id: meta.version_id,
        version_name: meta.version_name,
        content_type,
        relative_path: placed.relative_path.clone(),
        name: meta.name.unwrap_or_else(|| placed.file_name.clone()),
        icon_url: meta.icon_url,
        install_rule: meta.install_rule,
        installed_at: None,
    };
    let record =
        super::installed::record_install(instance_id, record, state).await?;

    Ok(InstalledContent {
        relative_path: placed.relative_path,
        file_name: placed.file_name,
        record_id: Some(record.id),
    })
}

/// The instance folder a loose content type goes into.
fn folder_for(content_type: ContentType) -> Option<&'static str> {
    Some(match content_type {
        ContentType::Mod => "mods",
        ContentType::ResourcePack => "resourcepacks",
        ContentType::Shader => "shaderpacks",
        ContentType::DataPack => "datapacks",
        _ => return None,
    })
}

/// The name to save under: the descriptor's suggestion, else the URL's last
/// segment. A name that could escape its folder is refused rather than repaired.
fn file_name_for(descriptor: &InstallDescriptor) -> crate::Result<String> {
    let candidate = descriptor
        .file_name
        .clone()
        .filter(|name| !name.trim().is_empty())
        .or_else(|| last_url_segment(&descriptor.url))
        .ok_or_else(|| {
            crate::ErrorKind::InputError(
                "The download has no file name and its URL has no path \
                 segment to take one from"
                    .to_string(),
            )
        })?;

    let candidate = sanitize_file_name(&candidate);
    if !path_util::is_safe_file_name(&candidate) {
        return Err(crate::ErrorKind::InputError(format!(
            "Refusing to install a file named '{candidate}'"
        ))
        .into());
    }
    Ok(candidate)
}

/// The final path segment of a URL, percent-decoded enough to be usable as a
/// file name. Query and fragment are dropped.
fn last_url_segment(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    let segment = parsed.path_segments()?.next_back()?.to_string();
    if segment.is_empty() {
        return None;
    }
    Some(percent_decode(&segment))
}

/// Minimal percent-decoding, enough to turn `a%20b.jar` into `a b.jar`. An
/// invalid escape is left as written rather than failing the install.
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) = (
                (bytes[index + 1] as char).to_digit(16),
                (bytes[index + 2] as char).to_digit(16),
            )
        {
            out.push((high * 16 + low) as u8);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Replace characters that cannot appear in a file name on some platform, so a
/// title like `Map: v2` does not fail on Windows.
fn sanitize_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|character| {
            if matches!(
                character,
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
            ) || character.is_control()
            {
                '-'
            } else {
                character
            }
        })
        .collect();
    let trimmed = cleaned.trim().trim_end_matches('.').to_string();
    if trimmed.is_empty() {
        "download".to_string()
    } else {
        trimmed
    }
}

/// Fetch the descriptor's URL, checking the hash when the source supplied one.
///
/// When the install names a source, the download host is checked against the
/// hosts that source declared — the same rule the network proxy applies. The
/// download itself is made by the launcher rather than through the proxy (it
/// needs to stream to disk and report progress), so the check has to be made
/// here too, or a manifest's `hosts` list would be a promise the download path
/// does not keep.
pub(crate) async fn download(
    descriptor: &InstallDescriptor,
    source_id: &str,
    state: &State,
) -> crate::Result<Vec<u8>> {
	download_inner(descriptor, source_id, state, None).await
}

/// `progress_title` asks for a native pack-import bar, created once the response
/// arrives so it can be sized by the announced length and fed the bytes as they
/// stream. A pack is far too big for the window to sit silently through.
async fn download_inner(
	descriptor: &InstallDescriptor,
	source_id: &str,
	state: &State,
	progress_title: Option<&str>,
) -> crate::Result<Vec<u8>> {
    let url = url::Url::parse(&descriptor.url).map_err(|error| {
        crate::ErrorKind::InputError(format!("Invalid download URL: {error}"))
    })?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(crate::ErrorKind::InputError(
            "Content downloads must be http or https".to_string(),
        )
        .into());
    }

    let _permit = state.fetch_semaphore.0.acquire().await.map_err(|_| {
        crate::ErrorKind::OtherError("Download semaphore closed".to_string())
    })?;

    let mut headers = HashMap::new();
    if let Some(requested) = &descriptor.headers {
        for (name, value) in requested {
            if is_forbidden_header(name) {
                continue;
            }
            headers.insert(name.clone(), value.clone());
        }
    }

    // Follow redirects by hand so every hop's host is checked. A source's
    // declared `hosts` list is only a boundary if a granted host cannot bounce
    // the request to an ungranted one, and reqwest's default policy would do
    // exactly that.
    let mut response = crate::util::fetch::send_checked(
        &crate::util::fetch::NO_REDIRECT_REQWEST_CLIENT,
        reqwest::Method::GET,
        url,
        &headers,
        None,
        |host| async move {
            if source_id.is_empty() {
                Ok(())
            } else {
                super::network::ensure_host_allowed(source_id, &host).await
            }
        },
    )
    .await?;
    if !response.status().is_success() {
        return Err(crate::ErrorKind::OtherError(format!(
            "Download failed with HTTP {}",
            response.status().as_u16()
        ))
        .into());
    }
    if let Some(length) = response.content_length()
        && length > MAX_DOWNLOAD_BYTES
    {
        return Err(crate::ErrorKind::InputError(format!(
            "Download is {length} bytes, over the {} MiB limit",
            MAX_DOWNLOAD_BYTES / (1024 * 1024)
        ))
        .into());
    }

	// Dropped when this function returns, which is what takes the bar away.
	let progress = match progress_title {
		Some(title) => Some(
			crate::event::emit::init_loading(
				crate::event::LoadingBarType::PackImport {
					pack_name: title.to_string(),
				},
				response.content_length().unwrap_or(0) as f64,
				title,
			)
			.await?,
		),
		None => None,
	};

	let mut bytes = Vec::new();
	while let Some(chunk) = response.chunk().await? {
		if bytes.len() as u64 + chunk.len() as u64 > MAX_DOWNLOAD_BYTES {
			return Err(crate::ErrorKind::InputError(format!(
				"Download exceeds the {} MiB limit",
				MAX_DOWNLOAD_BYTES / (1024 * 1024)
			)).into());
		}
		bytes.extend_from_slice(&chunk);
		// The bar counts bytes, so it is fed each chunk's own length rather than a
		// fraction. A server that announced no length leaves it in the waiting
		// state instead of reporting a percentage it cannot know.
		if let Some(progress) = &progress {
			crate::event::emit::emit_loading(progress, chunk.len() as f64, None)?;
		}
	}

    verify_hash(&bytes, descriptor.hash.as_ref())?;
    Ok(bytes.to_vec())
}

/// Check a downloaded payload against the source's hash, when it gave one.
///
/// The digest is compared case-insensitively; an algorithm this launcher does
/// not implement is reported rather than silently skipped, because a source
/// asking for a check it does not get is worse than no check at all.
fn verify_hash(
    bytes: &[u8],
    expected: Option<&FileHash>,
) -> crate::Result<()> {
    let Some(expected) = expected else {
        return Ok(());
    };

    let actual = match expected.algorithm.to_ascii_lowercase().as_str() {
        "sha1" => {
            let mut hasher = sha1_smol::Sha1::new();
            hasher.update(bytes);
            hasher.hexdigest()
        }
        "sha512" => {
            use sha2::Digest as _;
            let mut hasher = sha2::Sha512::new();
            hasher.update(bytes);
            hex_encode(&hasher.finalize())
        }
        other => {
            return Err(crate::ErrorKind::InputError(format!(
                "Unsupported hash algorithm '{other}'"
            ))
            .into());
        }
    };

    if !actual.eq_ignore_ascii_case(expected.value.trim()) {
        return Err(crate::ErrorKind::OtherError(format!(
            "Downloaded file does not match its {} hash",
            expected.algorithm
        ))
        .into());
    }
    Ok(())
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Remove the file an earlier install of the same project left behind, when the
/// new version arrived under a different name.
///
/// Only loose content is superseded this way. A jar is not something the player
/// edits, and two jars of one mod would both be loaded by the game; a world, by
/// contrast, is theirs to build in and is never replaced.
async fn supersede_previous_file(
    instance_dir: &Path,
    folder: &str,
    previous: Option<&super::installed::SourceContentRecord>,
    placed: &PlacedFile,
) {
    let Some(previous) = previous else {
        return;
    };
    if previous.relative_path == placed.relative_path {
        return;
    }
    let prefix = format!("{folder}/");
    let Some(name) = previous.relative_path.strip_prefix(&prefix) else {
        return;
    };
    if !path_util::is_safe_file_name(name) {
        return;
    }
    let stale = instance_dir.join(folder).join(name);
    if stale.is_file()
        && let Err(error) = io::remove_file(&stale).await
    {
        // The new file is already in place, so this is a leftover, not a failure:
        // say so and carry on rather than failing an install that worked.
        tracing::warn!(
            path = %stale.display(),
            "Could not remove the file a previous version left behind: {error}"
        );
    }
}

/// Write a loose content file into `<instance>/<folder>/`.
async fn install_loose_file(
    instance_dir: &Path,
    folder: &str,
    file_name: &str,
    bytes: &[u8],
) -> crate::Result<PlacedFile> {
    let target_dir = instance_dir.join(folder);
    io::create_dir_all(&target_dir).await?;

    // A file already there is replaced: installing the same mod twice should
    // update it, not leave two copies that the game would both load.
    let path = target_dir.join(file_name);
    io::write(&path, bytes).await?;

    tracing::info!(
        path = %path.display(),
        "Installed content source file"
    );

    Ok(PlacedFile {
        relative_path: format!("{folder}/{file_name}"),
        file_name: file_name.to_string(),
    })
}

/// Unzip a world archive into `<instance>/saves/<name>`, choosing a free name
/// and finding the level inside the archive rather than assuming its layout.
///
/// A world is **never** installed over an existing one. Players build in their
/// worlds, so "re-downloading the same map" would otherwise destroy the copy
/// they have been playing; a fresh folder with a `(n)` suffix is always the
/// right answer. The only cleanup on failure is the folder this call created.
async fn install_world(
    instance_dir: &Path,
    file_name: &str,
    bytes: &[u8],
) -> crate::Result<PlacedFile> {
    let saves_dir = instance_dir.join("saves");
    io::create_dir_all(&saves_dir).await?;

    let base = world_name_from(file_name);
    let world_dir = unique_dir(&saves_dir, &base)?;
    let world_name = world_dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| base.clone());

    let archive_bytes = bytes.to_vec();
    let target = world_dir.clone();
    let extracted =
        tokio::task::spawn_blocking(move || extract_world_zip(&archive_bytes, &target)).await;
    match extracted {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            let _ = io::remove_dir_all(&world_dir).await;
            return Err(error);
        }
        Err(error) => {
            let _ = io::remove_dir_all(&world_dir).await;
            return Err(error.into());
        }
    }

    if let Err(error) = normalize_world_root(&world_dir).await {
        let _ = io::remove_dir_all(&world_dir).await;
        return Err(error);
    }

    tracing::info!(
        path = %world_dir.display(),
        "Installed content source world"
    );

    Ok(PlacedFile {
        relative_path: format!("saves/{world_name}"),
        file_name: world_name,
    })
}

/// The world's name: the archive's name without its extension.
fn world_name_from(file_name: &str) -> String {
    let stem = file_name
        .strip_suffix(".zip")
        .or_else(|| file_name.strip_suffix(".ZIP"))
        .unwrap_or(file_name);
    let stem = stem.trim();
    if stem.is_empty() {
        "World".to_string()
    } else {
        stem.to_string()
    }
}

/// The first free `<base>`, `<base> (1)`, `<base> (2)`… under `parent`.
fn unique_dir(parent: &Path, base: &str) -> crate::Result<PathBuf> {
    let candidate = parent.join(base);
    if !candidate.exists() {
        return Ok(candidate);
    }
    for index in 1..1000 {
        let candidate = parent.join(format!("{base} ({index})"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(crate::ErrorKind::InputError(format!(
        "There are already too many worlds named '{base}'"
    ))
    .into())
}

/// Extract a world zip, creating `dest` first. Entries that would escape `dest`
/// are refused, and the expanded size is capped.
fn extract_world_zip(bytes: &[u8], dest: &Path) -> crate::Result<()> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|error| {
            crate::ErrorKind::InputError(format!(
                "The world archive is not a valid zip: {error}"
            ))
        })?;

    std::fs::create_dir_all(dest)?;

    let mut total: u64 = 0;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| {
            crate::ErrorKind::InputError(format!("Bad archive entry: {error}"))
        })?;

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
        if total > MAX_WORLD_UNPACKED_BYTES {
            return Err(crate::ErrorKind::InputError(
                "The world archive expands to more than the allowed size"
                    .to_string(),
            )
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

/// Move a world up a level when the archive wrapped it in a folder.
///
/// A zip made from a world folder puts `level.dat` one directory down; the game
/// only reads `saves/<world>/level.dat`, so the wrapper has to go. Only a single
/// wrapper containing `level.dat` is flattened — anything else is left as the
/// author packed it.
async fn normalize_world_root(world_dir: &Path) -> crate::Result<()> {
    if world_dir.join("level.dat").is_file() {
        return Ok(());
    }

    let mut entries = io::read_dir(world_dir).await?;
    let mut only_dir: Option<PathBuf> = None;
    let mut dir_count = 0;
    while let Some(entry) = entries.next_entry().await? {
        if entry.file_type().await?.is_dir() {
            dir_count += 1;
            only_dir = Some(entry.path());
        }
    }

    let Some(inner) = only_dir else {
        return Ok(());
    };
    if dir_count != 1 || !inner.join("level.dat").is_file() {
        return Ok(());
    }

    let mut inner_entries = io::read_dir(&inner).await?;
    while let Some(entry) = inner_entries.next_entry().await? {
        let target = world_dir.join(entry.file_name());
        io::rename_or_move(&entry.path(), &target).await?;
    }
    let _ = io::remove_dir_all(&inner).await;
    Ok(())
}

/// Save a skin into the launcher's own `skins/` folder.
///
/// Skins belong to the launcher rather than to an instance, so they are kept
/// beside the rest of the launcher's data and shown on the skins page.
async fn install_skin(
    file_name: &str,
    bytes: &[u8],
    state: &State,
) -> crate::Result<PlacedFile> {
    let dir = skins_dir(state);
    io::create_dir_all(&dir).await?;

    let named = if file_name.to_ascii_lowercase().ends_with(".png") {
        file_name.to_string()
    } else {
        format!("{file_name}.png")
    };

    let path = unique_file(&dir, &named)?;
    io::write(&path, bytes).await?;

    let saved = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or(named);

    tracing::info!(path = %path.display(), "Installed content source skin");

    Ok(PlacedFile {
        relative_path: saved.clone(),
        file_name: saved,
    })
}

/// The first free name, `<stem>`, `<stem> (1)`, … for a file.
fn unique_file(dir: &Path, file_name: &str) -> crate::Result<PathBuf> {
    let candidate = dir.join(file_name);
    if !candidate.exists() {
        return Ok(candidate);
    }
    let path = Path::new(file_name);
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| file_name.to_string());
    let extension = path.extension().map(|ext| ext.to_string_lossy().into_owned());
    for index in 1..1000 {
        let name = match &extension {
            Some(extension) => format!("{stem} ({index}).{extension}"),
            None => format!("{stem} ({index})"),
        };
        let candidate = dir.join(name);
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(crate::ErrorKind::InputError(format!(
        "There are already too many files named '{file_name}'"
    ))
    .into())
}

/// The launcher's own skins folder.
pub fn skins_dir(state: &State) -> PathBuf {
    state.directories.settings_dir.join(SKINS_DIR)
}

/// A skin downloaded through a content source, as the skins page lists it.
#[derive(Clone, Debug, Serialize)]
pub struct DownloadedSkin {
    pub name: String,
    /// Absolute path, so the frontend can turn it into a displayable URL.
    pub path: String,
}

/// Every skin file downloaded through a content source.
pub async fn list_downloaded_skins(
    state: &State,
) -> crate::Result<Vec<DownloadedSkin>> {
    let dir = skins_dir(state);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut entries = io::read_dir(&dir).await?;
    let mut skins = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        if !entry.file_type().await?.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.to_ascii_lowercase().ends_with(".png") {
            skins.push(DownloadedSkin {
                name,
                path: entry.path().to_string_lossy().into_owned(),
            });
        }
    }
    skins.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
    Ok(skins)
}

/// The absolute directory of an instance, resolving both DB-backed and
/// JSON-backed instances the same way the content system does.
async fn instance_dir(instance_id: &str, state: &State) -> crate::Result<PathBuf> {
    let scope =
        crate::state::instances::commands::resolve_content_scope(
            instance_id, None, state,
        )
        .await?;
    let path = scope.instance.path.clone();
    Ok(crate::state::libraries::resolve_instance_dir(state, &path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_come_from_the_descriptor_then_the_url() {
        let from_descriptor = InstallDescriptor {
            url: "https://example.com/a/b.jar".to_string(),
            headers: None,
            file_name: Some("nice name.jar".to_string()),
            hash: None,
        };
        assert_eq!(file_name_for(&from_descriptor).unwrap(), "nice name.jar");

        let from_url = InstallDescriptor {
            url: "https://example.com/a/cool%20mod.jar?x=1".to_string(),
            headers: None,
            file_name: None,
            hash: None,
        };
        assert_eq!(file_name_for(&from_url).unwrap(), "cool mod.jar");
    }

    #[test]
    fn a_name_that_could_escape_its_folder_is_refused() {
        let escaping = InstallDescriptor {
            url: "https://example.com/x".to_string(),
            headers: None,
            file_name: Some("../evil.jar".to_string()),
            hash: None,
        };
        assert!(file_name_for(&escaping).is_err());
    }

    #[test]
    fn characters_illegal_on_windows_are_replaced() {
        assert_eq!(sanitize_file_name("Map: v2?"), "Map- v2-");
        assert_eq!(sanitize_file_name("trailing..."), "trailing");
        assert_eq!(sanitize_file_name("   "), "download");
    }

    #[test]
    fn a_world_name_drops_the_zip_extension() {
        assert_eq!(world_name_from("My City.zip"), "My City");
        assert_eq!(world_name_from("Adventure"), "Adventure");
        assert_eq!(world_name_from(".zip"), "World");
    }

    #[test]
    fn hashes_are_verified_when_given() {
        let sha1 = FileHash {
            algorithm: "sha1".to_string(),
            value: "da39a3ee5e6b4b0d3255bfef95601890afd80709".to_string(),
        };
        assert!(verify_hash(b"", Some(&sha1)).is_ok());
        assert!(verify_hash(b"x", Some(&sha1)).is_err());
        assert!(verify_hash(b"x", None).is_ok());
    }
}

/// What staging a source's modpack produced.
///
/// camelCase on the wire: this crosses the Tauri boundary to the frontend,
/// which hands the manifest to the source's provider for resolution.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StagedModpack {
    /// Absolute path of the staged archive; the install reads it from disk.
    pub staged_path: String,
    /// `curseforge` or `modrinth`, by which manifest the archive carries.
    pub format: String,
    /// The manifest text, present for CurseForge packs — that is what the
    /// source's provider resolves into an install plan. A Modrinth pack needs
    /// no resolution; its index is read by the mrpack installer.
    pub manifest_json: Option<String>,
}

/// Download a source's modpack archive and read its manifest, so the frontend
/// can have the source's provider resolve the pack's files before the install
/// runs. The archive stays on disk under the launcher's metadata directory —
/// a pack is too big to round-trip through the webview.
pub async fn stage_modpack(
    source_id: &str,
    descriptor: &InstallDescriptor,
    state: &State,
) -> crate::Result<StagedModpack> {
	let pack_name = descriptor
		.file_name
		.clone()
		.unwrap_or_else(|| "整合包".to_string());
	let bytes = download_inner(descriptor, source_id, state, Some(&pack_name)).await?;

    let staging = super::super::pack::install_source_pack::staged_modpacks_dir(state);
    io::create_dir_all(&staging).await?;
    let name = descriptor
        .file_name
        .clone()
        .unwrap_or_else(|| descriptor.url.split('/').next_back().unwrap_or_default().to_string());
    let safe_name: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' })
        .collect();
    let staged_path = staging.join(format!(
        "{}-{}",
        chrono::Utc::now().timestamp_millis(),
        if safe_name.is_empty() { "pack.zip".to_string() } else { safe_name }
    ));
    io::write(&staged_path, &bytes).await?;

    // Identify the pack by its manifest, the way the installer will.
    let mut probe = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).map_err(|error| {
        crate::ErrorKind::InputError(format!("Staged modpack is not a readable zip: {error}"))
    })?;
    let format;
    let mut manifest_json = None;
    if probe.by_name("manifest.json").is_ok() {
        format = "curseforge".to_string();
        let file = probe.by_name("manifest.json").map_err(|error| {
            crate::ErrorKind::InputError(format!("Failed to read pack manifest: {error}"))
        })?;
        // The manifest is attacker-controlled, so the read is bounded: a small
        // compressed entry can expand to gigabytes, and the archive-level cap
        // does not cover that. `take` stops at the limit; a manifest larger
        // than a few MiB is not one this launcher would honour anyway.
        use std::io::Read;
        let mut text = String::new();
        file.take(MAX_MANIFEST_BYTES)
            .read_to_string(&mut text)
            .map_err(|error| {
                crate::ErrorKind::InputError(format!(
                    "Failed to read pack manifest: {error}"
                ))
            })?;
        manifest_json = Some(text);
    } else if probe.by_name("modrinth.index.json").is_ok() {
        format = "modrinth".to_string();
    } else {
        return Err(crate::ErrorKind::InputError(
            "The archive carries neither manifest.json nor modrinth.index.json".to_string(),
        )
        .into());
    }

    Ok(StagedModpack {
        staged_path: staged_path.to_string_lossy().to_string(),
        format,
        manifest_json,
    })
}
