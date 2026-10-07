//! Installing a content source's modpack.
//!
//! The pack arrives staged: the archive is on disk, and its per-file URLs were
//! resolved by the source's provider into a [`SourceModpackPlan`]. This module
//! does what the launcher alone may do — create the instance's loader setup,
//! fetch the files, and unpack the archive's overrides — in either pack
//! format: a CurseForge-style `manifest.json` plan, or a Modrinth
//! `modrinth.index.json` archive, which delegates to the regular mrpack
//! installer. A source itself never writes to an instance.

use super::install_from::{CreatePack, SourceModpackPlan};
use super::install_mrpack::{MrpackZipReader, install_zipped_mrpack_files_with_reporter};
use crate::install::{InstallErrorContext, InstallPhaseId, InstallProgressReporter};
use crate::state::{AppliedContentSetPatch, EditInstance, ModLoader, State};
use crate::util::fetch::{DownloadReason, fetch};
use serde::Deserialize;
use std::path::PathBuf;

/// A minimal slice of a CurseForge pack manifest — only what the unpack needs.
#[derive(Deserialize)]
struct CurseforgeManifest {
    #[serde(default = "default_overrides_dir")]
    overrides: String,
    #[serde(default)]
    minecraft: cf::Minecraft,
}

mod cf {
    use serde::Deserialize;

    #[derive(Default, Deserialize)]
    pub struct Minecraft {
        #[serde(default)]
        pub version: String,
    }
}

fn default_overrides_dir() -> String {
    "overrides".to_string()
}

pub(crate) async fn install_source_modpack_files_with_reporter(
    create_pack: CreatePack,
    plan: SourceModpackPlan,
    ignore_lock: bool,
    reason: DownloadReason,
    reporter: InstallProgressReporter,
) -> crate::Result<String> {
    // A staged archive may be either format: a source can host a Modrinth
    // pack as easily as a CurseForge one. The mrpack path is the mature one —
    // hashing, external-file handling, lock updating — so it wins whenever the
    // archive declares itself.
    let probe = MrpackZipReader::new(&create_pack.file).await?;
    let is_mrpack = probe
        .file()
        .entries()
        .iter()
        .any(|entry| matches!(entry.filename().as_str(), Ok("modrinth.index.json")));
    drop(probe);

    if is_mrpack {
        return Box::pin(install_zipped_mrpack_files_with_reporter(
            create_pack,
            ignore_lock,
            reason,
            reporter,
        ))
        .await;
    }

    install_curseforge_pack(create_pack, plan, reason, reporter).await
}

/// The CurseForge branch: apply the plan's loader setup, unpack `overrides/`,
/// and fetch every file the plan names into its folder.
async fn install_curseforge_pack(
    create_pack: CreatePack,
    plan: SourceModpackPlan,
    _reason: DownloadReason,
    reporter: InstallProgressReporter,
) -> crate::Result<String> {
    let state = &State::get().await?;
    let description = create_pack.description.clone();
    let mut icon_exists = create_pack.description.icon.is_some();
    let instance_id = create_pack.description.instance_id.clone();
    let source_path = pack_source_path(&create_pack.file);
    let instance_full_path = crate::api::instance::get_full_path(&instance_id).await?;

    let loader = ModLoader::from_string(&plan.loader);
    crate::api::instance::edit(
        &instance_id,
        EditInstance {
            content_set_patch: Some(AppliedContentSetPatch {
                source_kind: None,
                game_version: Some(plan.game_version.clone()),
                protocol_version: Some(None),
                loader: Some(loader),
                loader_version: Some(plan.loader_version.clone()),
            }),
            ..EditInstance::default()
        },
    )
    .await?;

    let mut zip_reader = MrpackZipReader::new(&create_pack.file).await?;

    reporter
        .set_context(
            InstallErrorContext::new("read modpack manifest")
                .maybe_project_id(description.project_id.clone())
                .maybe_version_id(description.version_id.clone())
                .source_path(source_path.clone())
                .entry_path("manifest.json")
                .build(),
        )
        .await?;

    // The manifest's own fields override what the plan says only for the
    // overrides directory name; the loader setup was resolved against the API
    // and the manifest cannot be checked against it anyway.
    let manifest_idx = zip_reader
        .file()
        .entries()
        .iter()
        .position(|entry| matches!(entry.filename().as_str(), Ok("manifest.json")));
    let overrides_dir = if let Some(index) = manifest_idx {
        let manifest: CurseforgeManifest =
            serde_json::from_str(&zip_reader.read_entry_to_string(index).await?)?;
        manifest.overrides
    } else {
        default_overrides_dir()
    };

    if !icon_exists {
        let icon_entry = zip_reader
            .file()
            .entries()
            .iter()
            .enumerate()
            .find_map(|(index, entry)| {
                matches!(
                    entry.filename().as_str(),
                    Ok("overrides/icon.png" | "icon.png")
                )
                .then_some(index)
            });
        if let Some(index) = icon_entry {
            let icon_path = instance_full_path.join("icon.png");
            zip_reader
                .extract_entry(index, &icon_path, &state.io_semaphore, None)
                .await?;
            crate::api::instance::edit_icon(&instance_id, Some(&icon_path)).await?;
            icon_exists = true;
        }
    }

    // Unpack the whole overrides directory into the instance root, preserving
    // relative paths. Configs, options files, and pre-placed worlds all ride
    // in here.
    let overrides_prefix = format!("{overrides_dir}/");
    let override_indices: Vec<usize> = zip_reader
        .file()
        .entries()
        .iter()
        .enumerate()
        .filter(|(_, entry)| {
            matches!(entry.filename().as_str(), Ok(name) if name.starts_with(&overrides_prefix))
        })
        .map(|(index, _)| index)
        .collect();

    reporter
        .update(
            InstallPhaseId::ExtractingOverrides,
            Some(crate::install::InstallProgress {
                current: 0,
                total: override_indices.len() as u64,
                secondary: None,
            }),
            modpack_details(&description),
        )
        .await?;

    for (done, index) in override_indices.iter().enumerate() {
        let name = zip_reader.file().entries()[*index]
            .filename()
            .as_str()
            .unwrap_or_default()
            .to_string();
        // Zip entries carry their own directory records; extracting those
        // through extract_entry would produce a file named like a directory.
        if name.ends_with('/') {
            continue;
        }
        let relative = name
            .strip_prefix(&overrides_prefix)
            .unwrap_or_default()
            .to_string();
        if relative.is_empty() {
            continue;
        }
        let target = instance_full_path.join(&relative);
        if let Some(parent) = target.parent() {
            crate::util::io::create_dir_all(parent).await?;
        }
        zip_reader
            .extract_entry(*index, &target, &state.io_semaphore, None)
            .await?;
        reporter
            .update(
                InstallPhaseId::ExtractingOverrides,
                Some(crate::install::InstallProgress {
                    current: done as u64 + 1,
                    total: override_indices.len() as u64,
                    secondary: None,
                }),
                modpack_details(&description),
            )
            .await?;
    }

    // The plan's files. One at a time keeps the progress honest and the
    // failure naming precise; a pack's worth of small jars does not need the
    // mrpack path's concurrency.
    reporter
        .update(
            InstallPhaseId::DownloadingContent,
            Some(crate::install::InstallProgress {
                current: 0,
                total: plan.files.len() as u64,
                secondary: None,
            }),
            modpack_details(&description),
        )
        .await?;

    for (done, file) in plan.files.iter().enumerate() {
        let sha1 = file.hash_sha1.as_deref();
        reporter
            .set_context(
                InstallErrorContext::new("download modpack content")
                    .urls(vec![file.url.clone()])
                    .maybe_project_id(description.project_id.clone())
                    .maybe_version_id(description.version_id.clone())
                    .build(),
            )
            .await?;

        let bytes = fetch(
            &file.url,
            sha1,
            None,
            None,
            &state.fetch_semaphore,
            &state.pool,
        )
        .await?;

        let folder = file.folder.as_deref().unwrap_or("mods");
        let directory = instance_full_path.join(folder);
        crate::util::io::create_dir_all(&directory).await?;
        let target = directory.join(sanitize_file_name(&file.file_name));
        crate::util::io::write(&target, &bytes).await?;

        reporter
            .update(
                InstallPhaseId::DownloadingContent,
                Some(crate::install::InstallProgress {
                    current: done as u64 + 1,
                    total: plan.files.len() as u64,
                    secondary: None,
                }),
                modpack_details(&description),
            )
            .await?;
    }

    let _ = icon_exists;
    Ok(instance_id)
}

fn modpack_details(
    description: &super::install_from::CreatePackDescription,
) -> crate::install::InstallPhaseDetails {
    crate::install::InstallPhaseDetails::Modpack {
        project_id: description.project_id.clone(),
        version_id: description.version_id.clone(),
        title: description.override_title.clone(),
    }
}

fn pack_source_path(file: &super::install_from::CreatePackFile) -> String {
    match file {
        super::install_from::CreatePackFile::Path(path) => {
            path.display().to_string()
        }
        super::install_from::CreatePackFile::Downloaded(file) => {
            file.path().display().to_string()
        }
        super::install_from::CreatePackFile::Bytes(_) => String::new(),
    }
}

/// Zip extraction paths come from the archive, so a crafted name must not
/// climb out of the folder it was assigned.
fn sanitize_file_name(name: &str) -> String {
    name.replace(['\\', '/'], "_")
}

/// Where staging keeps downloaded pack archives until their install runs.
pub fn staged_modpacks_dir(state: &State) -> PathBuf {
    state
        .directories
        .metadata_dir()
        .join("staged-modpacks")
}
