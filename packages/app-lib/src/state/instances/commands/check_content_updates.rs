use crate::state::instances::{
    ContentEntry, InstanceFile,
    adapters::sqlite::{content_rows, instance_rows},
};
use crate::state::{
    CacheBehaviour, CachedEntry, ProjectType, ReleaseChannel, State,
    libraries,
};
use std::collections::HashMap;

use super::sync_content_files::{
    ContentSyncFreshness, project_type_for_file,
    sync_instance_content_files_with_freshness,
};

/// One installed file that has a newer version available.
///
/// Serialized straight to the frontend so an update check can patch the flags on
/// the content list it already has, instead of invalidating it and paying for the
/// whole pipeline again.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ContentUpdate {
    pub relative_path: String,
    pub current_version_id: String,
    pub update_version_id: String,
}

#[derive(Clone, Debug)]
struct UpdateCandidate {
    entry: Option<ContentEntry>,
    file: InstanceFile,
    project_type: ProjectType,
    current_version_id: String,
}

pub(crate) async fn check_content_updates(
    instance_id: &str,
    cache_behaviour: Option<CacheBehaviour>,
    state: &State,
) -> crate::Result<Vec<ContentUpdate>> {
    check_content_updates_with_cache_behaviours(
        instance_id,
        cache_behaviour,
        cache_behaviour,
        state,
    )
    .await
}

/// Re-check every installed file for a newer version.
///
/// `MustRevalidate` rather than `Bypass`: this runs automatically when an
/// instance page is opened, and `Bypass` made that a guaranteed network round
/// trip per loader/channel group every single time. `MustRevalidate` still
/// refuses to serve an expired answer, so an actual update is never missed —
/// it just reuses entries that are still fresh.
pub(crate) async fn refresh_content_updates(
    instance_id: &str,
    state: &State,
) -> crate::Result<Vec<ContentUpdate>> {
    check_content_updates_with_cache_behaviours(
        instance_id,
        None,
        Some(CacheBehaviour::MustRevalidate),
        state,
    )
    .await
}

async fn check_content_updates_with_cache_behaviours(
    instance_id: &str,
    cache_behaviour: Option<CacheBehaviour>,
    update_cache_behaviour: Option<CacheBehaviour>,
    state: &State,
) -> crate::Result<Vec<ContentUpdate>> {
    let instance = match instance_rows::get_instance_by_id(instance_id, &state.pool)
        .await?
    {
        Some(inst) => inst,
        None => {
            // JSON-backed instances have no DB record — resolve from JSON.
            let json_instances =
                libraries::list_instances_from_json(state).await?;
            json_instances
                .into_iter()
                .find(|i| i.id == instance_id)
                .ok_or_else(|| {
                    crate::ErrorKind::InputError("Unknown instance".to_string())
                })?
        }
    };
    let content_set = content_rows::get_applied_content_set(
        &instance.id, &state.pool,
    )
    .await?
    .or_else(|| {
        // JSON-backed instances have no DB content set.
        None
    });
    let content_set = match content_set {
        Some(cs) => cs,
        None => super::list_content::create_json_content_set(
            instance_id, state,
        )
        .await?,
    };
    let entries =
        content_rows::get_content_entries(&content_set.id, &state.pool).await?;
    let entries_by_file_id = entries
        .iter()
        .filter_map(|entry| {
            entry.file_id.as_deref().map(|file_id| (file_id, entry))
        })
        .collect::<HashMap<_, _>>();
    let files = sync_instance_content_files_with_freshness(
        &instance,
        ContentSyncFreshness::from_cache_behaviour(cache_behaviour),
        state,
    )
    .await?;
    let hashes = files
        .iter()
        .map(|file| file.sha1.as_str())
        .collect::<Vec<_>>();
    let file_info = CachedEntry::get_file_many(
        &hashes,
        cache_behaviour,
        &state.pool,
        &state.api_semaphore,
    )
    .await?;
    let file_info_by_hash = file_info
        .into_iter()
        .map(|file| (file.hash.clone(), file))
        .collect::<HashMap<_, _>>();
    let candidates = files
        .into_iter()
        .filter_map(|file| {
            let project_type = project_type_for_file(&file)?;
            let metadata = file_info_by_hash.get(&file.sha1)?;
            Some(UpdateCandidate {
                entry: entries_by_file_id
                    .get(file.id.as_str())
                    .copied()
                    .cloned(),
                file,
                project_type,
                current_version_id: metadata.version_id.clone(),
            })
        })
        .collect::<Vec<_>>();

    if candidates.is_empty() {
        return Ok(Vec::new());
    }

    let installed_channels =
        installed_update_channels(&candidates, cache_behaviour, state).await?;
    let update_keys = candidates
        .iter()
        .map(|candidate| {
            update_cache_key(
                &candidate.file,
                candidate.project_type,
                effective_update_channel(
                    instance.update_channel,
                    installed_channels.get(&candidate.file.sha1).copied(),
                ),
                &content_set.game_version,
                content_set.loader.as_str(),
            )
        })
        .collect::<Vec<_>>();
    let update_key_refs = update_keys
        .iter()
        .map(|key| key.as_str())
        .collect::<Vec<_>>();
    let updates = CachedEntry::get_file_update_many(
        &update_key_refs,
        update_cache_behaviour,
        &state.pool,
        &state.api_semaphore,
    )
    .await?;
    let mut updates_by_hash: HashMap<String, Vec<String>> = HashMap::new();
    for update in updates {
        updates_by_hash
            .entry(update.hash)
            .or_default()
            .push(update.update_version_id);
    }

    let mut output = Vec::new();
    for candidate in candidates {
        let update_version_id = updates_by_hash
            .remove(&candidate.file.sha1)
            .unwrap_or_default()
            .into_iter()
            .find(|update_version_id| {
                update_version_id != &candidate.current_version_id
            });

        if let Some(entry) = &candidate.entry {
            content_rows::upsert_content_update_check(
                &entry.id,
                instance.update_channel,
                update_version_id.as_deref(),
                &state.pool,
            )
            .await?;
        }

        if let Some(update_version_id) = update_version_id {
            output.push(ContentUpdate {
                relative_path: candidate.file.relative_path,
                current_version_id: candidate.current_version_id,
                update_version_id,
            });
        }
    }

    Ok(output)
}

async fn installed_update_channels(
    candidates: &[UpdateCandidate],
    cache_behaviour: Option<CacheBehaviour>,
    state: &State,
) -> crate::Result<HashMap<String, ReleaseChannel>> {
    let version_ids = candidates
        .iter()
        .map(|candidate| candidate.current_version_id.as_str())
        .collect::<Vec<_>>();
    let versions = CachedEntry::get_version_many(
        &version_ids,
        cache_behaviour,
        &state.pool,
        &state.api_semaphore,
    )
    .await?;
    let channels_by_version_id = versions
        .into_iter()
        .map(|version| {
            (
                version.id,
                ReleaseChannel::from_version_type(&version.version_type),
            )
        })
        .collect::<HashMap<_, _>>();

    Ok(candidates
        .iter()
        .filter_map(|candidate| {
            channels_by_version_id
                .get(&candidate.current_version_id)
                .copied()
                .map(|channel| (candidate.file.sha1.clone(), channel))
        })
        .collect())
}

fn effective_update_channel(
    preferred: ReleaseChannel,
    installed: Option<ReleaseChannel>,
) -> ReleaseChannel {
    installed.map_or(preferred, |channel| preferred.least_stable(channel))
}

fn update_cache_key(
    file: &InstanceFile,
    project_type: ProjectType,
    channel: ReleaseChannel,
    game_version: &str,
    loader: &str,
) -> String {
    format!(
        "{}-{}-{}-{}",
        file.sha1,
        if project_type == ProjectType::Mod {
            loader.to_string()
        } else {
            project_type.get_loaders().join("+")
        },
        channel.key(),
        game_version
    )
}
