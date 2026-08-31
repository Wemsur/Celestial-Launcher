use crate::event::InstancePayloadType;
use crate::event::emit::emit_instance;
use crate::state::{
    CacheBehaviour, ContentFile, ContentItem, ContentSet, Dependency,
    InstanceInstallCandidate, InstanceInstallTarget, LinkedModpackInfo,
    ProjectType, State,
};
use dashmap::DashMap;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

#[tracing::instrument]
pub async fn sync_content_files(
    instance_id: &str,
) -> crate::Result<Vec<crate::state::instances::InstanceFile>> {
    let state = State::get().await?;
    crate::state::sync_content_files(instance_id, &state).await
}

#[tracing::instrument]
pub async fn list_content_sets(
    instance_id: &str,
) -> crate::Result<Vec<ContentSet>> {
    let state = State::get().await?;
    crate::state::list_content_sets(instance_id, &state.pool).await
}

#[tracing::instrument]
pub async fn get_projects(
    instance_id: &str,
    cache_behaviour: Option<CacheBehaviour>,
) -> crate::Result<DashMap<String, ContentFile>> {
    let state = State::get().await?;
    crate::state::get_content_projects(
        instance_id,
        None,
        cache_behaviour,
        &state,
    )
    .await
}

#[tracing::instrument]
pub async fn get_installed_project_ids(
    instance_id: &str,
) -> crate::Result<Vec<String>> {
    let state = State::get().await?;
    crate::state::get_installed_project_ids_for_instance(
        instance_id,
        None,
        &state,
    )
    .await
}

#[tracing::instrument]
pub async fn get_install_candidates(
    project_id: &str,
    project_type: ProjectType,
    targets: Vec<InstanceInstallTarget>,
) -> crate::Result<Vec<InstanceInstallCandidate>> {
    let state = State::get().await?;
    crate::state::get_instance_install_candidates(
        project_id,
        project_type,
        &targets,
        &state.pool,
    )
    .await
}

/// Minimum gap between two background metadata completion passes for the same
/// key. A pass runs the whole content pipeline against the API, and every
/// navigation into an instance would otherwise start another one.
const METADATA_COMPLETION_INTERVAL: Duration = Duration::from_secs(60);

/// When each completion pass last started. Doubles as an in-flight guard: the
/// timestamp is recorded on claim, so a pass that takes less than the interval
/// can never be started twice.
static LAST_METADATA_COMPLETION: LazyLock<DashMap<String, Instant>> =
    LazyLock::new(DashMap::new);

fn claim_metadata_completion(guard: &str) -> bool {
    let now = Instant::now();
    if let Some(previous) = LAST_METADATA_COMPLETION.get(guard)
        && now.duration_since(*previous) < METADATA_COMPLETION_INTERVAL
    {
        return false;
    }
    LAST_METADATA_COMPLETION.insert(guard.to_string(), now);
    true
}

#[tracing::instrument]
pub async fn get_content_items(
    instance_id: &str,
    cache_behaviour: Option<CacheBehaviour>,
) -> crate::Result<Vec<ContentItem>> {
    let state = State::get().await?;
    tracing::info!(
        "get_content_items called for instance '{}', cache={:?}",
        instance_id,
        cache_behaviour
    );

    // Nothing on the first-paint path may wait on the network. A cold metadata
    // cache otherwise costs a round trip per batch, which on a slow link is the
    // entire page load; the missing pieces are fetched in the background and the
    // frontend is told once they arrive. Callers that named a cache behaviour
    // (an install finishing, an explicit refresh) keep the one they asked for.
    let (local_first, effective_behaviour) =
        local_first_behaviour(cache_behaviour);

    let started = std::time::Instant::now();
    let result = crate::state::list_content(
        instance_id,
        None,
        effective_behaviour,
        &state,
    )
    .await;
    match &result {
        Ok(items) => tracing::info!(
            "content_timing: TOTAL get_content_items {} ms ({} items, local_first={}) for instance '{}'",
            started.elapsed().as_millis(),
            items.len(),
            local_first,
            instance_id
        ),
        Err(e) => tracing::error!(
            "content_timing: TOTAL get_content_items FAILED after {} ms for instance '{}': {}",
            started.elapsed().as_millis(),
            instance_id,
            e
        ),
    }

    if local_first && let Ok(items) = &result {
        spawn_metadata_completion(
            "content",
            instance_id.to_string(),
            metadata_fingerprint(items),
            |instance_id| async move {
                let state = State::get().await?;
                let items = crate::state::list_content(
                    &instance_id,
                    None,
                    Some(CacheBehaviour::StaleWhileRevalidateSkipOffline),
                    &state,
                )
                .await?;
                Ok(metadata_fingerprint(&items))
            },
        );
    }

    result
}

/// Whether a caller left the cache behaviour open, in which case the read is on
/// the first-paint path and must not wait on the network. See
/// [`CacheBehaviour::LocalOnly`].
fn local_first_behaviour(
    cache_behaviour: Option<CacheBehaviour>,
) -> (bool, Option<CacheBehaviour>) {
    if cache_behaviour.is_none() {
        (true, Some(CacheBehaviour::LocalOnly))
    } else {
        (false, cache_behaviour)
    }
}

/// Fingerprint of the parts of a content list that a network fetch can fill in.
///
/// Deliberately narrow: the point is to answer "did the background pass learn
/// anything the local-first read did not know", not to detect every difference.
fn metadata_fingerprint(items: &[ContentItem]) -> u64 {
    hash_with(|hasher| {
        use std::hash::Hash;

        items.len().hash(hasher);
        for item in items {
            item.id.hash(hasher);
            item.project.as_ref().map(|project| &project.id).hash(hasher);
            item.version.as_ref().map(|version| &version.id).hash(hasher);
            item.update_version_id.hash(hasher);
        }
    })
}

fn modpack_info_fingerprint(info: Option<&LinkedModpackInfo>) -> u64 {
    hash_with(|hasher| {
        use std::hash::Hash;

        match info {
            None => 0u8.hash(hasher),
            Some(info) => {
                1u8.hash(hasher);
                info.project.id.hash(hasher);
                info.version.as_ref().map(|version| &version.id).hash(hasher);
                info.update_version_id.hash(hasher);
            }
        }
    })
}

fn hash_with(
    write: impl FnOnce(&mut std::collections::hash_map::DefaultHasher),
) -> u64 {
    use std::hash::Hasher;

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    write(&mut hasher);
    hasher.finish()
}

/// Fill in whatever a local-first read could not answer, then tell the frontend —
/// but only if the answer actually changed, so a list whose files simply are not
/// on Modrinth does not cause a refresh on every open.
///
/// `key` separates the passes that can run for one instance; each notifies
/// independently and the frontend coalesces them into a single refetch.
fn spawn_metadata_completion<F, Fut>(
    key: &str,
    instance_id: String,
    previous: u64,
    complete: F,
) where
    F: FnOnce(String) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = crate::Result<u64>> + Send + 'static,
{
    let guard = format!("{key}:{instance_id}");
    if !claim_metadata_completion(&guard) {
        return;
    }
    tokio::spawn(async move {
        let started = std::time::Instant::now();
        let outcome = complete(instance_id.clone()).await;

        match outcome {
            Ok(completed) if completed != previous => {
                tracing::info!(
                    "content_timing: [bg] metadata completion ({guard}) {} ms changed the result, notifying frontend",
                    started.elapsed().as_millis(),
                );
                let _ =
                    emit_instance(&instance_id, InstancePayloadType::Synced)
                        .await;
            }
            Ok(_) => tracing::info!(
                "content_timing: [bg] metadata completion ({guard}) {} ms, nothing new",
                started.elapsed().as_millis(),
            ),
            Err(error) => tracing::warn!(
                "Background metadata completion ({guard}) failed: {error}"
            ),
        }
    });
}

#[tracing::instrument]
pub async fn refresh_content_updates(instance_id: &str) -> crate::Result<()> {
    let state = State::get().await?;
    let started = std::time::Instant::now();
    let result =
        crate::state::refresh_content_updates(instance_id, &state).await;
    tracing::info!(
        "content_timing: TOTAL refresh_content_updates {} ms (ok={}) for instance '{}'",
        started.elapsed().as_millis(),
        result.is_ok(),
        instance_id
    );
    result
}

#[tracing::instrument]
pub async fn get_linked_modpack_content(
    instance_id: &str,
    cache_behaviour: Option<CacheBehaviour>,
) -> crate::Result<Vec<ContentItem>> {
    let state = State::get().await?;
    crate::state::list_linked_modpack_content(
        instance_id,
        None,
        cache_behaviour,
        &state,
    )
    .await
}

#[tracing::instrument]
pub async fn get_dependencies_as_content_items(
    dependencies: Vec<Dependency>,
    cache_behaviour: Option<CacheBehaviour>,
) -> crate::Result<Vec<ContentItem>> {
    let state = State::get().await?;
    crate::state::dependencies_to_content_items(
        &dependencies,
        cache_behaviour,
        &state.pool,
        &state.api_semaphore,
    )
    .await
}

#[tracing::instrument]
pub async fn get_linked_modpack_info(
    instance_id: &str,
    cache_behaviour: Option<CacheBehaviour>,
) -> crate::Result<Option<LinkedModpackInfo>> {
    let state = State::get().await?;
    // Same rule as `get_content_items`: this runs alongside it on the first
    // paint, so it may not block on the API either.
    let (local_first, effective_behaviour) =
        local_first_behaviour(cache_behaviour);
    let result = crate::state::get_linked_modpack_info(
        instance_id,
        None,
        effective_behaviour,
        &state,
    )
    .await;

    if local_first && let Ok(info) = &result {
        spawn_metadata_completion(
            "modpack",
            instance_id.to_string(),
            modpack_info_fingerprint(info.as_ref()),
            |instance_id| async move {
                let state = State::get().await?;
                let info = crate::state::get_linked_modpack_info(
                    &instance_id,
                    None,
                    Some(CacheBehaviour::StaleWhileRevalidateSkipOffline),
                    &state,
                )
                .await?;
                Ok(modpack_info_fingerprint(info.as_ref()))
            },
        );
    }

    result
}
