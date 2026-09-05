use super::sync_content_files::{
    ContentSyncFreshness, cached_content_hashes, instance_files_fingerprint,
    load_cached_content_items, project_type_for_file,
    spawn_content_refresh_for_instance, store_cached_content_items,
    sync_instance_content_files_with_freshness,
};
use crate::State;
use crate::pack::install_from::{PackFileHash, PackFormat};
use crate::state::instances::adapters::sqlite;
use crate::state::instances::{
    ContentEntry, ContentSet, ContentSetStatus, ContentSourceKind, Instance,
    InstanceInstallCandidate, InstanceInstallTarget, InstanceLink,
    InstanceMetadata,
};
use crate::state::{
    CacheBehaviour, CachedEntry, CachedFile, ContentFile, ContentItem,
    ContentItemOwner, ContentItemProject, ContentItemVersion, Dependency,
    LinkedModpackInfo, ModLoader, Organization, OwnerType, Project,
    ProjectType, ReleaseChannel, TeamMember, Version, VersionEnvironment,
    VersionV3, libraries,
};
use crate::util::fetch::{
    DownloadMeta, DownloadReason, FetchSemaphore, fetch_mirrors, sha1_async,
};
use async_zip::base::read::seek::ZipFileReader;
use dashmap::DashMap;
use sqlx::SqlitePool;
use std::collections::{HashMap, HashSet};
use std::io::Cursor;

#[derive(Clone, Debug)]
struct ResolvedContentScope {
    instance: Instance,
    content_set: ContentSet,
}

#[derive(Clone, Copy, Debug)]
enum ContentFilter<'a> {
    All,
    ExcludeModpack(&'a ModpackIdentifiers),
    ExcludeSourceKind {
        source_kind: ContentSourceKind,
        exclude_untracked: bool,
    },
    OnlyModpack(&'a ModpackIdentifiers),
    OnlySourceKind {
        source_kind: ContentSourceKind,
        include_untracked: bool,
    },
}

pub(crate) async fn list_content_sets(
    instance_id: &str,
    pool: &SqlitePool,
) -> crate::Result<Vec<ContentSet>> {
    let instance = sqlite::instance_rows::get_instance_by_id(instance_id, pool)
        .await?;

    if instance.is_none() {
        // JSON-backed instances have no DB content sets — return empty
        // We can't check libraries here without State, but sync_content_files
        // will return empty for JSON instances anyway, so we just error
        // as "no content sets" rather than "unknown instance"
        return Ok(Vec::new());
    }

    let instance = instance.unwrap();
    sqlite::content_rows::get_content_sets_for_instance(&instance.id, pool)
        .await
}

pub(crate) async fn get_content_projects(
    instance_id: &str,
    content_set_id: Option<&str>,
    cache_behaviour: Option<CacheBehaviour>,
    state: &State,
) -> crate::Result<DashMap<String, ContentFile>> {
    let resolved = match resolve_content_scope_with_instance(
        instance_id,
        content_set_id,
        &state.pool,
    )
    .await
    {
        Ok(resolved) => resolved,
        Err(_) => {
            resolve_content_scope_for_json(instance_id, state).await?
        }
    };

    Ok(content_projects_for_scope(
        &resolved,
        cache_behaviour,
        state,
        ContentFilter::All,
    )
    .await?
    .projects)
}

pub(crate) async fn get_installed_project_ids_for_instance(
    instance_id: &str,
    content_set_id: Option<&str>,
    state: &State,
) -> crate::Result<Vec<String>> {
    let projects =
        get_content_projects(instance_id, content_set_id, None, state).await?;

    Ok(projects
        .into_iter()
        .filter_map(|(_, file)| {
            file.metadata.map(|metadata| metadata.project_id)
        })
        .collect::<HashSet<_>>()
        .into_iter()
        .collect())
}

#[derive(sqlx::FromRow)]
struct InstanceInstallCandidateRow {
    id: String,
    name: String,
    icon_path: Option<String>,
    game_version: String,
    loader: String,
    installed: i64,
}

/// Instances the frontend may offer as install targets for a project.
///
/// Sourced from [`crate::api::instance::list`] rather than the `instances` table:
/// instances are JSON-backed now (`libraries.json` plus a per-instance
/// `instance.json`), and creating one through a library never writes a DB row. The
/// old DB-only query therefore returned nothing at all for them, which the install
/// dialog reported as "0 compatible instances" for every project type.
pub(crate) async fn get_instance_install_candidates(
    project_id: &str,
    project_type: ProjectType,
    targets: &[InstanceInstallTarget],
    state: &State,
) -> crate::Result<Vec<InstanceInstallCandidate>> {
    let mut instances = crate::api::instance::list(None).await?;
    // Server projects manage their own content; they were excluded by the old
    // query's `link_kind NOT IN (...)` and stay excluded here.
    instances.retain(|meta| {
        !matches!(
            meta.link,
            InstanceLink::ServerProject { .. }
                | InstanceLink::ServerProjectModpack { .. }
        )
    });
    instances.sort_by(|left, right| {
        left.instance
            .name
            .to_lowercase()
            .cmp(&right.instance.name.to_lowercase())
            .then_with(|| left.instance.name.cmp(&right.instance.name))
    });

    let installed =
        installed_instance_ids(project_id, &instances, state).await?;

    Ok(instances
        .into_iter()
        .map(|meta| {
            let loader = meta.applied_content_set.loader;
            let game_version = meta.applied_content_set.game_version;
            InstanceInstallCandidate {
                compatible: instance_matches_targets(
                    project_type,
                    &game_version,
                    loader.as_str(),
                    targets,
                ),
                installed: installed.contains(&meta.instance.id),
                id: meta.instance.id,
                name: meta.instance.name,
                icon_path: meta.instance.icon_path,
                game_version,
                loader,
            }
        })
        .collect())
}

/// Which of `instances` already have `project_id` installed.
///
/// Deliberately cheap: this runs while an install dialog is opening, so it reads
/// what is already on disk instead of syncing or hashing anything, and never waits
/// on the network. An instance whose content has never been resolved simply reports
/// "not installed" — the flag only greys out a row, and it corrects itself once the
/// instance's content tab has been opened.
async fn installed_instance_ids(
    project_id: &str,
    instances: &[InstanceMetadata],
    state: &State,
) -> crate::Result<HashSet<String>> {
    let mut installed = HashSet::new();
    let mut pending_hashes: Vec<(String, Vec<String>)> = Vec::new();

    for meta in instances {
        if !meta.instance.is_json_backed() {
            continue;
        }

        // Resolved items carry project ids directly, so when they are cached this
        // needs neither SQL nor a hash lookup.
        if let Some(items) = load_cached_content_items(&meta.instance, state) {
            if items.iter().any(|item| {
                item.project
                    .as_ref()
                    .is_some_and(|project| project.id == project_id)
            }) {
                installed.insert(meta.instance.id.clone());
            }
            continue;
        }

        if let Some(hashes) = cached_content_hashes(&meta.instance, state) {
            pending_hashes.push((meta.instance.id.clone(), hashes));
        }
    }

    if !pending_hashes.is_empty() {
        let unique = pending_hashes
            .iter()
            .flat_map(|(_, hashes)| hashes.iter().map(String::as_str))
            .collect::<HashSet<_>>();
        let hash_refs = unique.into_iter().collect::<Vec<_>>();
        // `LocalOnly`: an install dialog must not block on the API to decide
        // whether a row is greyed out. Missing hashes are fetched in the
        // background, so the next open is accurate.
        let files = CachedEntry::get_file_many(
            &hash_refs,
            Some(CacheBehaviour::LocalOnly),
            &state.pool,
            &state.api_semaphore,
        )
        .await?;
        let matching = files
            .into_iter()
            .filter(|file| file.project_id == project_id)
            .map(|file| file.hash)
            .collect::<HashSet<_>>();

        for (instance_id, hashes) in pending_hashes {
            if hashes.iter().any(|hash| matching.contains(hash)) {
                installed.insert(instance_id);
            }
        }
    }

    // DB-backed instances still keep their files in `instance_content_entries`.
    // The query below is the original candidate query, reused untouched purely for
    // its `installed` column, and only when such an instance is actually present.
    if instances.iter().any(|meta| !meta.instance.is_json_backed()) {
        let rows = sqlx::query_as!(
            InstanceInstallCandidateRow,
            r#"
		SELECT
			i.id,
			i.name,
			i.icon_path,
			cs.game_version,
			cs.loader,
			CASE
				WHEN EXISTS (
					SELECT 1
					FROM instance_content_entries entry
					INNER JOIN instance_files file
						ON file.id = entry.file_id
					WHERE entry.content_set_id = cs.id
						AND entry.project_id = ?
						AND file.missing = 0
				)
					THEN 1
				ELSE 0
			END AS "installed!: i64"
		FROM instances i
		INNER JOIN instance_content_sets cs
			ON cs.id = i.applied_content_set_id
		LEFT JOIN instance_links link
			ON link.instance_id = i.id
		WHERE COALESCE(link.link_kind, 'unmanaged') NOT IN (
			'server_project',
			'server_project_modpack'
		)
		ORDER BY i.name ASC
		"#,
            project_id,
        )
        .fetch_all(&state.pool)
        .await?;

        for row in rows {
            if row.installed != 0 {
                installed.insert(row.id);
            }
        }
    }

    Ok(installed)
}

fn instance_matches_targets(
    project_type: ProjectType,
    game_version: &str,
    loader: &str,
    targets: &[InstanceInstallTarget],
) -> bool {
    targets.iter().any(|target| {
        target.game_version == game_version
            && (project_type != ProjectType::Mod
                || target.loader == loader
                || target.loader == "datapack")
    })
}

pub(crate) async fn list_content(
    instance_id: &str,
    content_set_id: Option<&str>,
    cache_behaviour: Option<CacheBehaviour>,
    state: &State,
) -> crate::Result<Vec<ContentItem>> {
    tracing::info!(
        "list_content called for instance '{}', content_set={:?}",
        instance_id, content_set_id
    );
    let t_scope = std::time::Instant::now();
    let resolved = match resolve_content_scope_with_instance(
        instance_id,
        content_set_id,
        &state.pool,
    )
    .await
    {
        Ok(resolved) => {
            tracing::info!(
                "list_content: using DB scope for instance '{}', content_set='{}'",
                resolved.instance.id,
                resolved.content_set.id
            );
            resolved
        }
        Err(e) => {
            tracing::info!(
                "list_content: DB scope failed for '{}': {}, trying JSON fallback",
                instance_id, e
            );
            let json_resolved =
                resolve_content_scope_for_json(instance_id, state).await?;
            tracing::info!(
                "list_content: using JSON scope for instance '{}', content_set='{}'",
                json_resolved.instance.id,
                json_resolved.content_set.id
            );
            json_resolved
        }
    };
    tracing::info!(
        "content_timing: [1/6] resolve scope {} ms for '{}'",
        t_scope.elapsed().as_millis(),
        instance_id
    );
    let t_link = std::time::Instant::now();
    let link = sqlite::instance_rows::get_instance_link(
        &resolved.instance.id,
        &state.pool,
    )
    .await?;
    let imported_modpack_scope = is_imported_modpack_scope(&link);
    let linked_modpack_source_kind = linked_modpack_source_kind(&link);
    let modpack_ids = if imported_modpack_scope {
        None
    } else {
        match linked_modpack_ids(&link) {
            Some((_, version_id)) => {
                get_cached_modpack_identifiers(
                    &version_id,
                    cache_behaviour,
                    &state.pool,
                    &state.api_semaphore,
                )
                .await?
            }
            None => None,
        }
    };
    tracing::info!(
        "content_timing: [2/6] link + modpack identifiers {} ms for '{}'",
        t_link.elapsed().as_millis(),
        instance_id
    );
    let filter = if imported_modpack_scope {
        ContentFilter::ExcludeSourceKind {
            source_kind: ContentSourceKind::ImportedModpack,
            exclude_untracked: resolved.instance.install_stage
                != crate::state::InstanceInstallStage::Installed,
        }
    } else if let Some(ids) = modpack_ids.as_ref() {
        ContentFilter::ExcludeModpack(ids)
    } else if let Some(source_kind) = linked_modpack_source_kind {
        ContentFilter::ExcludeSourceKind {
            source_kind,
            exclude_untracked: true,
        }
    } else {
        ContentFilter::All
    };

    // Second and later opens: a previous run stored the fully resolved items for
    // this exact file list, so there is nothing left to ask SQLite or the API.
    // Only the unfiltered, local-first read may use it — a caller that asked to
    // revalidate wants the pipeline, and a modpack filter changes what belongs
    // in the list. The file list still gets revalidated, just off this thread.
    let unfiltered = matches!(filter, ContentFilter::All);
    if unfiltered
        && cache_behaviour == Some(CacheBehaviour::LocalOnly)
        && let Some(items) =
            load_cached_content_items(&resolved.instance, state)
    {
        tracing::info!(
            "content_timing: [F] resolved item cache HIT ({} items) for '{}'",
            items.len(),
            instance_id
        );
        spawn_content_refresh_for_instance(&resolved.instance, state);
        return Ok(items);
    }

    let scope_content =
        content_projects_for_scope(&resolved, cache_behaviour, state, filter)
            .await?;
    let files_fingerprint = scope_content.files_fingerprint;
    let installed_versions = scope_content.versions;
    let files = scope_content.projects.into_iter().collect::<Vec<_>>();

    let t_items = std::time::Instant::now();
    let items = content_files_to_content_items(
        &resolved.instance,
        resolved.content_set.loader,
        &files,
        installed_versions,
        cache_behaviour,
        state,
    )
    .await;
    tracing::info!(
        "content_timing: [6/6] content_files_to_content_items {} ms ({} files) for '{}'",
        t_items.elapsed().as_millis(),
        files.len(),
        instance_id
    );
    if unfiltered && let Ok(items) = &items {
        store_cached_content_items(
            &resolved.instance,
            state,
            files_fingerprint,
            items.clone(),
        )
        .await;
    }
    items
}

/// The cheapest possible answer for "what content does this instance have".
///
/// Reads nothing but the on-disk content cache: no SQLite, no network, no
/// filesystem walk. Meant as placeholder data so the content tab can paint rows
/// immediately while [`list_content`] resolves the Modrinth metadata behind it.
///
/// Returns the fully resolved items when the item cache happens to hold them
/// (then this *is* the final answer), otherwise bare rows carrying only what the
/// filesystem knows: file name, size, enabled state and project type.
pub(crate) async fn list_content_skeleton(
    instance_id: &str,
    state: &State,
) -> crate::Result<Vec<ContentItem>> {
    let started = std::time::Instant::now();
    let resolved = match resolve_content_scope_with_instance(
        instance_id, None, &state.pool,
    )
    .await
    {
        Ok(resolved) => resolved,
        Err(_) => resolve_content_scope_for_json(instance_id, state).await?,
    };

    if let Some(items) = load_cached_content_items(&resolved.instance, state) {
        tracing::info!(
            "content_timing: [S] skeleton {} ms ({} items, from item cache) for '{}'",
            started.elapsed().as_millis(),
            items.len(),
            instance_id
        );
        return Ok(items);
    }

    let files = sync_instance_content_files_with_freshness(
        &resolved.instance,
        ContentSyncFreshness::UseCache,
        state,
    )
    .await?;

    let mut items = files
        .into_iter()
        .filter(|file| !file.missing)
        .filter_map(|file| {
            let project_type = project_type_for_file(&file)?;
            Some(ContentItem {
                file_name: file.file_name,
                file_path: file.relative_path,
                id: file.sha1,
                size: file.size,
                enabled: file.enabled,
                locked: false,
                project_type,
                project: None,
                version: None,
                environment: None,
                owner: None,
                has_update: false,
                update_version_id: None,
                date_added: None,
                source_kind: None,
                embedded_metadata: None,
            })
        })
        .collect::<Vec<_>>();
    sort_content_items(&mut items);

    tracing::info!(
        "content_timing: [S] skeleton {} ms ({} bare items) for '{}'",
        started.elapsed().as_millis(),
        items.len(),
        instance_id
    );

    Ok(items)
}

pub(crate) async fn list_linked_modpack_content(
    instance_id: &str,
    content_set_id: Option<&str>,
    cache_behaviour: Option<CacheBehaviour>,
    state: &State,
) -> crate::Result<Vec<ContentItem>> {
    let resolved = match resolve_content_scope_with_instance(
        instance_id,
        content_set_id,
        &state.pool,
    )
    .await
    {
        Ok(resolved) => resolved,
        Err(_) => {
            resolve_content_scope_for_json(instance_id, state).await?
        }
    };
    let link = sqlite::instance_rows::get_instance_link(
        &resolved.instance.id,
        &state.pool,
    )
    .await?;
    if is_imported_modpack_scope(&link) {
        let scope_content = content_projects_for_scope(
            &resolved,
            cache_behaviour,
            state,
            ContentFilter::OnlySourceKind {
                source_kind: ContentSourceKind::ImportedModpack,
                include_untracked: resolved.instance.install_stage
                    != crate::state::InstanceInstallStage::Installed,
            },
        )
        .await?;
        let installed_versions = scope_content.versions;
        let files = scope_content.projects.into_iter().collect::<Vec<_>>();

        return content_files_to_content_items(
            &resolved.instance,
            resolved.content_set.loader,
            &files,
            installed_versions,
            cache_behaviour,
            state,
        )
        .await;
    }

    let Some((_, version_id)) = linked_modpack_ids(&link) else {
        return Ok(Vec::new());
    };
    let modpack_ids = match get_modpack_identifiers(
        &version_id,
        &resolved.content_set,
        &state.pool,
        &state.api_semaphore,
    )
    .await
    {
        Ok(ids) => Some(ids),
        Err(err) => {
            tracing::warn!("Failed to fetch modpack identifiers: {}", err);
            None
        }
    };
    let filter = if let Some(ids) = modpack_ids.as_ref() {
        ContentFilter::OnlyModpack(ids)
    } else if let Some(source_kind) = linked_modpack_source_kind(&link) {
        ContentFilter::OnlySourceKind {
            source_kind,
            include_untracked: true,
        }
    } else {
        return Ok(Vec::new());
    };
    let scope_content =
        content_projects_for_scope(&resolved, cache_behaviour, state, filter)
            .await?;
    let installed_versions = scope_content.versions;
    let files = scope_content.projects.into_iter().collect::<Vec<_>>();

    content_files_to_content_items(
        &resolved.instance,
        resolved.content_set.loader,
        &files,
        installed_versions,
        cache_behaviour,
        state,
    )
    .await
}

pub(crate) async fn get_linked_modpack_info(
    instance_id: &str,
    content_set_id: Option<&str>,
    cache_behaviour: Option<CacheBehaviour>,
    state: &State,
) -> crate::Result<Option<LinkedModpackInfo>> {
    let resolved = match resolve_content_scope_with_instance(
        instance_id,
        content_set_id,
        &state.pool,
    )
    .await
    {
        Ok(resolved) => resolved,
        Err(_) => {
            resolve_content_scope_for_json(instance_id, state).await?
        }
    };
    let Some((project_id, version_id)) =
        linked_modpack_ids_for_instance(&resolved.instance.id, &state.pool)
            .await?
    else {
        return Ok(None);
    };
    let (project, version, all_versions) = tokio::try_join!(
        CachedEntry::get_project(
            &project_id,
            cache_behaviour,
            &state.pool,
            &state.api_semaphore,
        ),
        CachedEntry::get_version(
            &version_id,
            cache_behaviour,
            &state.pool,
            &state.api_semaphore,
        ),
        CachedEntry::get_project_versions(
            &project_id,
            cache_behaviour,
            &state.pool,
            &state.api_semaphore,
        ),
    )?;
    let version_project_id = version
        .as_ref()
        .filter(|version| version.project_id != project_id)
        .map(|version| version.project_id.clone());
    let (project, all_versions) =
        if let Some(version_project_id) = version_project_id {
            let (modpack_project, modpack_versions) = tokio::try_join!(
                CachedEntry::get_project(
                    &version_project_id,
                    cache_behaviour,
                    &state.pool,
                    &state.api_semaphore,
                ),
                CachedEntry::get_project_versions(
                    &version_project_id,
                    cache_behaviour,
                    &state.pool,
                    &state.api_semaphore,
                ),
            )?;
            (modpack_project.or(project), modpack_versions)
        } else {
            (project, all_versions)
        };
    let project = project.ok_or_else(|| {
        crate::ErrorKind::InputError(format!(
            "Linked modpack project {project_id} not found"
        ))
    })?;
    let owner = if let Some(org_id) = &project.organization {
        let org = CachedEntry::get_organization(
            org_id,
            cache_behaviour,
            &state.pool,
            &state.api_semaphore,
        )
        .await?;
        org.map(|org| ContentItemOwner {
            id: org.id,
            name: org.name,
            avatar_url: org.icon_url,
            owner_type: OwnerType::Organization,
        })
    } else {
        let team = CachedEntry::get_team(
            &project.team,
            cache_behaviour,
            &state.pool,
            &state.api_semaphore,
        )
        .await?;
        team.and_then(|team| {
            team.into_iter()
                .find(|member| member.is_owner)
                .map(|member| ContentItemOwner {
                    id: member.user.id,
                    name: member.user.username,
                    avatar_url: member.user.avatar_url,
                    owner_type: OwnerType::User,
                })
        })
    };
    let (has_update, update_version_id, update_version) = version
        .as_ref()
        .map(|version| {
            check_modpack_update(
                &version_id,
                version,
                all_versions,
                resolved.instance.update_channel,
            )
        })
        .unwrap_or((false, None, None));

    Ok(Some(LinkedModpackInfo {
        project,
        version,
        owner,
        has_update,
        update_version_id,
        update_version,
    }))
}

pub(crate) async fn dependencies_to_content_items(
    dependencies: &[Dependency],
    cache_behaviour: Option<CacheBehaviour>,
    pool: &SqlitePool,
    fetch_semaphore: &FetchSemaphore,
) -> crate::Result<Vec<ContentItem>> {
    let project_ids = dependencies
        .iter()
        .filter_map(|dependency| dependency.project_id.clone())
        .collect::<HashSet<_>>();
    if project_ids.is_empty() {
        return Ok(Vec::new());
    }
    let version_ids = dependencies
        .iter()
        .filter_map(|dependency| dependency.version_id.clone())
        .collect::<HashSet<_>>();
    let meta = resolve_metadata(
        &project_ids,
        &version_ids,
        Vec::new(),
        cache_behaviour,
        pool,
        fetch_semaphore,
    )
    .await?;
    let mut items = dependencies
        .iter()
        .filter_map(|dependency| {
            let project_id = dependency.project_id.as_ref()?;
            let project = meta
                .projects
                .iter()
                .find(|project| &project.id == project_id)?;
            let version =
                dependency.version_id.as_ref().and_then(|version_id| {
                    meta.versions
                        .iter()
                        .find(|version| &version.id == version_id)
                });
            let owner =
                resolve_owner(project, &meta.teams, &meta.organizations);
            let project_type =
                project_type_from_api_name(&project.project_type);

            Some(ContentItem {
                file_name: version
                    .and_then(|version| version.files.first())
                    .map(|file| file.filename.clone())
                    .unwrap_or_else(|| {
                        format!(
                            "{}.jar",
                            project.slug.as_deref().unwrap_or(&project.id)
                        )
                    }),
                file_path: String::new(),
                id: String::new(),
                size: version
                    .and_then(|version| version.files.first())
                    .map(|file| file.size as u64)
                    .unwrap_or(0),
                enabled: true,
                locked: false,
                project_type,
                project: Some(content_item_project(project)),
                version: version.map(|version| ContentItemVersion {
                    id: version.id.clone(),
                    version_number: version.version_number.clone(),
                    file_name: version
                        .files
                        .first()
                        .map(|file| file.filename.clone())
                        .unwrap_or_default(),
                    date_published: Some(version.date_published.to_rfc3339()),
                }),
                environment: resolve_environment(
                    dependency.version_id.as_deref(),
                    &meta.versions_v3,
                ),
                owner,
                has_update: false,
                update_version_id: None,
                date_added: None,
                source_kind: None,
                embedded_metadata: None,
            })
        })
        .collect::<Vec<_>>();
    sort_content_items(&mut items);

    Ok(items)
}

async fn resolve_content_scope_with_instance(
    instance_id: &str,
    content_set_id: Option<&str>,
    pool: &SqlitePool,
) -> crate::Result<ResolvedContentScope> {
    let instance = sqlite::instance_rows::get_instance_by_id(instance_id, pool)
        .await?
        .ok_or_else(|| {
            crate::ErrorKind::InputError("Unknown instance".to_string())
        })?;
    let content_set = match content_set_id {
        Some(content_set_id) => {
            let content_set =
                sqlite::content_rows::get_content_set(content_set_id, pool)
                    .await?
                    .ok_or_else(|| {
                        crate::ErrorKind::InputError(format!(
                            "Unknown content set {content_set_id}"
                        ))
                    })?;

            if content_set.instance_id != instance.id {
                return Err(crate::ErrorKind::InputError(format!(
					"Content set {content_set_id} does not belong to instance {}",
					instance.id
				))
				.into());
            }

            content_set
        }
        None => {
            sqlite::content_rows::get_applied_content_set(&instance.id, pool)
                .await?
                .ok_or_else(|| {
                    crate::ErrorKind::InputError(format!(
                        "Instance {} has no applied content set",
                        instance.id
                    ))
                })?
        }
    };

    Ok(ResolvedContentScope {
        instance,
        content_set,
    })
}

/// Resolve a content scope for JSON-backed instances that have no DB content set.
async fn resolve_content_scope_for_json(
    instance_id: &str,
    state: &State,
) -> crate::Result<ResolvedContentScope> {
    let json_instances =
        libraries::list_instances_from_json(state).await?;
    let instance = json_instances
        .into_iter()
        .find(|i| i.id == instance_id)
        .ok_or_else(|| {
            crate::ErrorKind::InputError("Unknown instance".to_string())
        })?;
    // Build a synthetic content set — same structure used by get.rs for JSON
    // instances; content_entries will be empty so only CachedFile lookups apply.
    let dir = libraries::resolve_instance_dir(&state, &instance.path);
    let (game_version, loader, loader_version) =
        if let Ok(Some(json)) = libraries::InstanceJson::read_from_dir(&dir) {
            (json.game_version, json.loader, json.loader_version)
        } else {
            (None, None, None)
        };
    let game_version = game_version.or_else(|| {
        libraries::detect_game_version_from_dir(&dir)
    });
    let loader = loader.or_else(|| {
        if game_version.as_ref().is_some_and(|gv| gv.is_empty()) {
            None
        } else {
            Some(libraries::detect_loader_from_dir(&dir).as_str().to_string())
        }
    });
    let content_set = ContentSet {
        id: format!("json-cs-{}", instance.id),
        instance_id: instance.id.clone(),
        name: "Applied Content Set".to_string(),
        source_kind: ContentSourceKind::Local,
        status: ContentSetStatus::Available,
        game_version: game_version.unwrap_or_default(),
        protocol_version: None,
        loader: ModLoader::from_string(&loader.unwrap_or_default()),
        loader_version,
        created: chrono::Utc::now(),
        modified: chrono::Utc::now(),
    };
    Ok(ResolvedContentScope { instance, content_set })
}

/// Build a synthetic `ContentSet` for a JSON-backed instance.
///
/// Tries to read game_version/loader from the sidecar file; falls back to
/// filesystem detection if the sidecar has no stored values.
pub(crate) async fn create_json_content_set(
    instance_id: &str,
    state: &State,
) -> crate::Result<ContentSet> {
    let json_instances =
        libraries::list_instances_from_json(state).await?;
    let instance = json_instances
        .into_iter()
        .find(|i| i.id == instance_id)
        .ok_or_else(|| {
            crate::ErrorKind::InputError("Unknown instance".to_string())
        })?;
    let dir = libraries::resolve_instance_dir(&state, &instance.path);
    let (game_version, loader, loader_version) =
        if let Ok(Some(json)) = libraries::InstanceJson::read_from_dir(&dir) {
            (json.game_version, json.loader, json.loader_version)
        } else {
            (None, None, None)
        };
    let game_version = game_version.or_else(|| {
        libraries::detect_game_version_from_dir(&dir)
    });
    let loader = loader.or_else(|| {
        if game_version.as_ref().is_some_and(|gv| gv.is_empty()) {
            None
        } else {
            Some(libraries::detect_loader_from_dir(&dir).as_str().to_string())
        }
    });
    Ok(ContentSet {
        id: format!("json-cs-{}", instance.id),
        instance_id: instance.id.clone(),
        name: "Applied Content Set".to_string(),
        source_kind: ContentSourceKind::Local,
        status: ContentSetStatus::Available,
        game_version: game_version.unwrap_or_default(),
        protocol_version: None,
        loader: ModLoader::from_string(&loader.unwrap_or_default()),
        loader_version,
        created: chrono::Utc::now(),
        modified: chrono::Utc::now(),
    })
}

/// Build a synthetic `ContentScope` for a JSON-backed instance.
///
/// Same logic as `create_json_content_set`, but returns a full `ContentScope`.
pub(crate) async fn create_json_content_scope(
    instance_id: &str,
    state: &State,
) -> crate::Result<
    crate::state::instances::commands::apply_content_install::ContentScope,
> {
    use super::apply_content_install::ContentScope;

    let json_instances =
        libraries::list_instances_from_json(state).await?;
    let instance = json_instances
        .into_iter()
        .find(|i| i.id == instance_id)
        .ok_or_else(|| {
            crate::ErrorKind::InputError("Unknown instance".to_string())
        })?;
    let dir = libraries::resolve_instance_dir(&state, &instance.path);
    let (game_version, loader, loader_version) =
        if let Ok(Some(json)) = libraries::InstanceJson::read_from_dir(&dir) {
            (json.game_version, json.loader, json.loader_version)
        } else {
            (None, None, None)
        };
    let game_version = game_version.or_else(|| {
        libraries::detect_game_version_from_dir(&dir)
    });
    let loader = loader.or_else(|| {
        if game_version.as_ref().is_some_and(|gv| gv.is_empty()) {
            None
        } else {
            Some(libraries::detect_loader_from_dir(&dir).as_str().to_string())
        }
    });
    let content_set = ContentSet {
        id: format!("json-cs-{}", instance.id),
        instance_id: instance.id.clone(),
        name: "Applied Content Set".to_string(),
        source_kind: ContentSourceKind::Local,
        status: ContentSetStatus::Available,
        game_version: game_version.unwrap_or_default(),
        protocol_version: None,
        loader: ModLoader::from_string(&loader.unwrap_or_default()),
        loader_version,
        created: chrono::Utc::now(),
        modified: chrono::Utc::now(),
    };
    Ok(ContentScope {
        instance,
        content_set_id: content_set.id,
        game_version: content_set.game_version,
        loader: content_set.loader.as_str().to_string(),
    })
}

/// A scope's content files, plus the identity of the synced file list they came
/// from. The fingerprint is what lets the resolved items be cached: it says
/// exactly which file list the items describe, so a later read can tell whether
/// they still apply without trusting a timestamp.
struct ScopeContent {
    projects: DashMap<String, ContentFile>,
    files_fingerprint: String,
    /// Versions already read out of the cache to work out each file's release
    /// channel. Handed on so the metadata resolve does not deserialize the same
    /// blobs a second time — on a 90-mod instance that was ~70ms of pure waste.
    versions: Vec<Version>,
}

async fn content_projects_for_scope(
    resolved: &ResolvedContentScope,
    cache_behaviour: Option<CacheBehaviour>,
    state: &State,
    filter: ContentFilter<'_>,
) -> crate::Result<ScopeContent> {
    tracing::info!(
        "content_projects_for_scope: starting for instance '{}', content_set='{}'",
        resolved.instance.id,
        resolved.content_set.id
    );
    let t_sync = std::time::Instant::now();
    let files = sync_instance_content_files_with_freshness(
        &resolved.instance,
        ContentSyncFreshness::from_cache_behaviour(cache_behaviour),
        state,
    )
    .await?;
    tracing::info!(
        "content_timing: [3/6] sync_content_files {} ms ({} files) for '{}'",
        t_sync.elapsed().as_millis(),
        files.len(),
        resolved.instance.id
    );
    let files_fingerprint = instance_files_fingerprint(&files);
    let t_db = std::time::Instant::now();
    let entries = sqlite::content_rows::get_content_entries(
        &resolved.content_set.id,
        &state.pool,
    )
    .await?;
    let entries_by_file_id = entries
        .iter()
        .filter_map(|entry| {
            entry.file_id.as_deref().map(|file_id| (file_id, entry))
        })
        .collect::<HashMap<_, _>>();
    let locked_file_ids = sqlite::content_rows::get_locked_instance_file_ids(
        &resolved.instance.id,
        &state.pool,
    )
    .await?;
    let hashes = files
        .iter()
        .map(|file| file.sha1.as_str())
        .collect::<Vec<_>>();
    tracing::info!(
        "content_timing: [4/6] local db rows {} ms for '{}'",
        t_db.elapsed().as_millis(),
        resolved.instance.id
    );
    let t_files_api = std::time::Instant::now();
    let file_info = CachedEntry::get_file_many(
        &hashes,
        cache_behaviour,
        &state.pool,
        &state.api_semaphore,
    )
    .await?;
    tracing::info!(
        "content_timing: [5a/6] get_file_many {} ms ({} hashes in, {} matched)",
        t_files_api.elapsed().as_millis(),
        hashes.len(),
        file_info.len()
    );
    let file_info_by_hash = file_info
        .into_iter()
        .map(|file| (file.hash.clone(), file))
        .collect::<HashMap<_, _>>();
    let t_channels = std::time::Instant::now();
    let (installed_channels, installed_versions) =
        get_installed_update_channels(
            &file_info_by_hash,
            cache_behaviour,
            &state.pool,
            &state.api_semaphore,
        )
        .await?;
    tracing::info!(
        "content_timing: [5b/6] get_installed_update_channels {} ms ({} versions read)",
        t_channels.elapsed().as_millis(),
        installed_versions.len()
    );
    let update_keys = files
        .iter()
        .filter(|file| file_info_by_hash.contains_key(&file.sha1))
        .filter_map(|file| {
            let project_type = project_type_for_file(file)?;
            let channel = resolved.instance.update_channel.least_stable(
                installed_channels
                    .get(&file.sha1)
                    .copied()
                    .unwrap_or(resolved.instance.update_channel),
            );
            Some(file_update_cache_key(
                &file.sha1,
                project_type,
                &resolved.content_set,
                channel,
            ))
        })
        .collect::<Vec<_>>();
    let update_key_refs =
        update_keys.iter().map(String::as_str).collect::<Vec<_>>();
    let t_updates = std::time::Instant::now();
    let file_updates = CachedEntry::get_file_update_many(
        &update_key_refs,
        cache_behaviour,
        &state.pool,
        &state.api_semaphore,
    )
    .await?;
    tracing::info!(
        "content_timing: [5c/6] get_file_update_many {} ms ({} keys)",
        t_updates.elapsed().as_millis(),
        update_key_refs.len()
    );
    let mut updates_by_hash: HashMap<String, Vec<String>> = HashMap::new();
    for update in file_updates {
        updates_by_hash
            .entry(update.hash)
            .or_default()
            .push(update.update_version_id);
    }
    let output = DashMap::new();

    for file in files {
        if file.missing {
            continue;
        }

        let Some(project_type) = project_type_for_file(&file) else {
            continue;
        };
        let metadata = file_info_by_hash.get(&file.sha1).cloned();
        let entry = entries_by_file_id.get(file.id.as_str()).copied();

        match filter {
            ContentFilter::All => {}
            ContentFilter::ExcludeModpack(ids) => {
                if ids.is_modpack_file(
                    &file.sha1,
                    metadata.as_ref(),
                    entry.and_then(|entry| entry.project_id.as_deref()),
                ) {
                    continue;
                }
            }
            ContentFilter::ExcludeSourceKind {
                source_kind,
                exclude_untracked,
            } => {
                if entry.is_some_and(|entry| entry.source_kind == source_kind)
                    || (exclude_untracked && entry.is_none())
                {
                    continue;
                }
            }
            ContentFilter::OnlyModpack(ids) => {
                if !ids.is_modpack_file(
                    &file.sha1,
                    metadata.as_ref(),
                    entry.and_then(|entry| entry.project_id.as_deref()),
                ) {
                    continue;
                }
            }
            ContentFilter::OnlySourceKind {
                source_kind,
                include_untracked,
            } => {
                if !(entry
                    .is_some_and(|entry| entry.source_kind == source_kind)
                    || include_untracked && entry.is_none())
                {
                    continue;
                }
            }
        }

        let update_version_id = metadata.as_ref().and_then(|metadata| {
            let update_ids =
                updates_by_hash.remove(&file.sha1).unwrap_or_default();
            if !update_ids.contains(&metadata.version_id) {
                update_ids.into_iter().next()
            } else {
                None
            }
        });

        output.insert(
            file.relative_path.clone(),
            ContentFile {
                update_version_id,
                hash: file.sha1,
                file_name: file.file_name,
                enabled: entry.map_or(file.enabled, |entry| {
                    entry.enabled && file.enabled
                }),
                locked: locked_file_ids.contains(&file.id),
                size: file.size,
                metadata: file_metadata_from_entry_or_cache(entry, metadata),
                project_type,
                source_kind: entry.map(|entry| entry.source_kind),
            },
        );
    }

    Ok(ScopeContent {
        projects: output,
        files_fingerprint,
        versions: installed_versions,
    })
}

/// Release channel per file hash, plus the `Version` rows that were read to work
/// it out. The rows are returned rather than dropped because the metadata resolve
/// needs the very same versions moments later.
async fn get_installed_update_channels(
    file_info_by_hash: &HashMap<String, CachedFile>,
    cache_behaviour: Option<CacheBehaviour>,
    pool: &SqlitePool,
    fetch_semaphore: &FetchSemaphore,
) -> crate::Result<(HashMap<String, ReleaseChannel>, Vec<Version>)> {
    let version_ids = file_info_by_hash
        .values()
        .map(|file| file.version_id.as_str())
        .collect::<HashSet<_>>();
    if version_ids.is_empty() {
        return Ok((HashMap::new(), Vec::new()));
    }
    let version_id_refs = version_ids.iter().copied().collect::<Vec<_>>();
    let versions = CachedEntry::get_version_many(
        &version_id_refs,
        cache_behaviour,
        pool,
        fetch_semaphore,
    )
    .await?;
    let channels_by_version_id = versions
        .iter()
        .map(|version| {
            (
                version.id.clone(),
                ReleaseChannel::from_version_type(&version.version_type),
            )
        })
        .collect::<HashMap<_, _>>();

    let channels = file_info_by_hash
        .iter()
        .filter_map(|(hash, file)| {
            channels_by_version_id
                .get(&file.version_id)
                .copied()
                .map(|channel| (hash.clone(), channel))
        })
        .collect();

    Ok((channels, versions))
}

fn file_update_cache_key(
    hash: &str,
    project_type: ProjectType,
    content_set: &ContentSet,
    channel: ReleaseChannel,
) -> String {
    let loader_key = if project_type == ProjectType::Mod {
        content_set.loader.as_str().to_string()
    } else {
        project_type.get_loaders().join("+")
    };

    format!(
        "{}-{}-{}-{}",
        hash,
        loader_key,
        channel.key(),
        content_set.game_version
    )
}

async fn content_files_to_content_items(
    instance: &Instance,
    loader: ModLoader,
    files: &[(String, ContentFile)],
    preloaded_versions: Vec<Version>,
    cache_behaviour: Option<CacheBehaviour>,
    state: &State,
) -> crate::Result<Vec<ContentItem>> {
    let project_ids = files
        .iter()
        .filter_map(|(_, file)| {
            file.metadata
                .as_ref()
                .map(|metadata| metadata.project_id.clone())
        })
        .collect::<HashSet<_>>();
    let version_ids = files
        .iter()
        .filter_map(|(_, file)| {
            file.metadata
                .as_ref()
                .map(|metadata| metadata.version_id.clone())
        })
        .collect::<HashSet<_>>();
    let t_meta = std::time::Instant::now();
    let meta = resolve_metadata(
        &project_ids,
        &version_ids,
        preloaded_versions,
        cache_behaviour,
        &state.pool,
        &state.api_semaphore,
    )
    .await?;
    tracing::info!(
        "content_timing: [6a/6] resolve_metadata {} ms ({} projects, {} versions)",
        t_meta.elapsed().as_millis(),
        project_ids.len(),
        version_ids.len()
    );
    let t_embedded = std::time::Instant::now();
    let embedded_metadata =
        super::embedded_content_metadata::resolve_embedded_content_metadata(
            instance, loader, files, state,
        )
            .await?;
    tracing::info!(
        "content_timing: [6b/6] resolve_embedded_content_metadata {} ms ({} resolved)",
        t_embedded.elapsed().as_millis(),
        embedded_metadata.len()
    );
    let instance_path =
        libraries::resolve_instance_dir(&state, &instance.path);
    let paths = files
        .iter()
        .map(|(path, _)| instance_path.join(path))
        .collect::<Vec<_>>();
    let modification_times: Vec<Option<String>> =
        tokio::task::spawn_blocking(move || {
            paths
                .iter()
                .map(|path| {
                    std::fs::metadata(path)
                        .and_then(|metadata| metadata.modified())
                        .ok()
                        .map(|time| {
                            chrono::DateTime::<chrono::Utc>::from(time)
                                .to_rfc3339()
                        })
                })
                .collect()
        })
        .await?;
    let mut items = files
        .iter()
        .enumerate()
        .map(|(index, (path, file))| {
            let project = file.metadata.as_ref().and_then(|metadata| {
                meta.projects
                    .iter()
                    .find(|project| project.id == metadata.project_id)
            });
            let version = file.metadata.as_ref().and_then(|metadata| {
                meta.versions
                    .iter()
                    .find(|version| version.id == metadata.version_id)
            });
            let owner = project.and_then(|project| {
                resolve_owner(project, &meta.teams, &meta.organizations)
            });

            ContentItem {
                file_name: file.file_name.clone(),
                file_path: path.clone(),
                id: file.hash.clone(),
                size: file.size,
                enabled: file.enabled,
                locked: file.locked,
                project_type: file.project_type,
                project: project.map(content_item_project),
                version: version.map(|version| ContentItemVersion {
                    id: version.id.clone(),
                    version_number: version.version_number.clone(),
                    file_name: file.file_name.clone(),
                    date_published: Some(version.date_published.to_rfc3339()),
                }),
                environment: resolve_environment(
                    file.metadata
                        .as_ref()
                        .map(|metadata| metadata.version_id.as_str()),
                    &meta.versions_v3,
                ),
                owner,
                has_update: file.update_version_id.is_some(),
                update_version_id: file.update_version_id.clone(),
                date_added: modification_times[index].clone(),
                source_kind: file.source_kind,
                embedded_metadata: embedded_metadata.get(&file.hash).cloned(),
            }
        })
        .collect::<Vec<_>>();
    sort_content_items(&mut items);

    Ok(items)
}

struct ResolvedMetadata {
    projects: Vec<Project>,
    versions: Vec<Version>,
    versions_v3: Vec<VersionV3>,
    teams: Vec<Vec<TeamMember>>,
    organizations: Vec<Organization>,
}

async fn resolve_metadata(
    project_ids: &HashSet<String>,
    version_ids: &HashSet<String>,
    preloaded_versions: Vec<Version>,
    cache_behaviour: Option<CacheBehaviour>,
    pool: &SqlitePool,
    fetch_semaphore: &FetchSemaphore,
) -> crate::Result<ResolvedMetadata> {
    let project_id_refs =
        project_ids.iter().map(String::as_str).collect::<Vec<_>>();
    // Whatever the caller already read stays read: only the ids it did not cover
    // go back to the cache.
    let preloaded_version_ids = preloaded_versions
        .iter()
        .map(|version| version.id.as_str())
        .collect::<HashSet<_>>();
    let missing_version_ids = version_ids
        .iter()
        .map(String::as_str)
        .filter(|id| !preloaded_version_ids.contains(id))
        .collect::<Vec<_>>();
    let version_id_refs =
        version_ids.iter().map(String::as_str).collect::<Vec<_>>();
    let (projects, fetched_versions, versions_v3) =
        if !project_ids.is_empty() || !version_ids.is_empty() {
            tokio::try_join!(
                async {
                    if project_ids.is_empty() {
                        Ok(Vec::new())
                    } else {
                        CachedEntry::get_project_many(
                            &project_id_refs,
                            cache_behaviour,
                            pool,
                            fetch_semaphore,
                        )
                        .await
                    }
                },
                async {
                    if missing_version_ids.is_empty() {
                        Ok(Vec::new())
                    } else {
                        CachedEntry::get_version_many(
                            &missing_version_ids,
                            cache_behaviour,
                            pool,
                            fetch_semaphore,
                        )
                        .await
                    }
                },
                async {
                    if version_ids.is_empty() {
                        Ok(Vec::new())
                    } else {
                        CachedEntry::get_version_v3_many(
                            &version_id_refs,
                            cache_behaviour,
                            pool,
                            fetch_semaphore,
                        )
                        .await
                    }
                }
            )?
        } else {
            (Vec::new(), Vec::new(), Vec::new())
        };
    let mut versions = preloaded_versions;
    versions.extend(fetched_versions);
    let team_ids = projects
        .iter()
        .map(|project| project.team.clone())
        .collect::<HashSet<_>>();
    let org_ids = projects
        .iter()
        .filter_map(|project| project.organization.clone())
        .collect::<HashSet<_>>();
    let team_id_refs = team_ids.iter().map(String::as_str).collect::<Vec<_>>();
    let org_id_refs = org_ids.iter().map(String::as_str).collect::<Vec<_>>();
    let (teams, organizations) = if !team_ids.is_empty() || !org_ids.is_empty()
    {
        tokio::try_join!(
            async {
                if team_ids.is_empty() {
                    Ok(Vec::new())
                } else {
                    CachedEntry::get_team_many(
                        &team_id_refs,
                        cache_behaviour,
                        pool,
                        fetch_semaphore,
                    )
                    .await
                }
            },
            async {
                if org_ids.is_empty() {
                    Ok(Vec::new())
                } else {
                    CachedEntry::get_organization_many(
                        &org_id_refs,
                        cache_behaviour,
                        pool,
                        fetch_semaphore,
                    )
                    .await
                }
            }
        )?
    } else {
        (Vec::new(), Vec::new())
    };

    Ok(ResolvedMetadata {
        projects,
        versions,
        versions_v3,
        teams,
        organizations,
    })
}

fn resolve_environment(
    version_id: Option<&str>,
    versions: &[VersionV3],
) -> Option<VersionEnvironment> {
    let version_id = version_id?;
    versions
        .iter()
        .find(|version| version.id == version_id)
        .and_then(|version| version.environment)
}

fn resolve_owner(
    project: &Project,
    teams: &[Vec<TeamMember>],
    organizations: &[Organization],
) -> Option<ContentItemOwner> {
    if let Some(org_id) = &project.organization {
        organizations
            .iter()
            .find(|organization| &organization.id == org_id)
            .map(|organization| ContentItemOwner {
                id: organization.id.clone(),
                name: organization.name.clone(),
                avatar_url: organization.icon_url.clone(),
                owner_type: OwnerType::Organization,
            })
    } else {
        teams
            .iter()
            .find(|team| {
                team.first()
                    .is_some_and(|member| member.team_id == project.team)
            })
            .and_then(|team| team.iter().find(|member| member.is_owner))
            .map(|member| ContentItemOwner {
                id: member.user.id.clone(),
                name: member.user.username.clone(),
                avatar_url: member.user.avatar_url.clone(),
                owner_type: OwnerType::User,
            })
    }
}

fn content_item_project(project: &Project) -> ContentItemProject {
    ContentItemProject {
        id: project.id.clone(),
        slug: project.slug.clone(),
        title: project.title.clone(),
        icon_url: project.icon_url.clone(),
        license: project.license.clone(),
        categories: project.categories.clone(),
        additional_categories: project.additional_categories.clone(),
    }
}

fn file_metadata_from_entry_or_cache(
    entry: Option<&ContentEntry>,
    cached: Option<CachedFile>,
) -> Option<crate::state::FileMetadata> {
    let project_id = entry
        .and_then(|entry| entry.project_id.clone())
        .or_else(|| cached.as_ref().map(|file| file.project_id.clone()))?;
    let version_id = entry
        .and_then(|entry| entry.version_id.clone())
        .or_else(|| cached.as_ref().map(|file| file.version_id.clone()))?;

    Some(crate::state::FileMetadata {
        project_id,
        version_id,
    })
}

fn is_imported_modpack_scope(link: &InstanceLink) -> bool {
    matches!(link, InstanceLink::ImportedModpack { .. })
}

async fn linked_modpack_ids_for_instance(
    instance_id: &str,
    pool: &SqlitePool,
) -> crate::Result<Option<(String, String)>> {
    let link =
        sqlite::instance_rows::get_instance_link(instance_id, pool).await?;
    Ok(linked_modpack_ids(&link))
}

fn linked_modpack_ids(link: &InstanceLink) -> Option<(String, String)> {
    match link {
        InstanceLink::ModrinthModpack {
            project_id,
            version_id,
        } => Some((project_id.clone(), version_id.clone())),
        InstanceLink::ServerProjectModpack {
            content_project_id,
            content_version_id,
            ..
        } => Some((content_project_id.clone(), content_version_id.clone())),
        InstanceLink::ImportedModpack {
            project_id: Some(project_id),
            version_id: Some(version_id),
            ..
        } => Some((project_id.clone(), version_id.clone())),
        InstanceLink::SharedInstance {
            modpack_project_id: Some(project_id),
            modpack_version_id: Some(version_id),
        } => Some((project_id.clone(), version_id.clone())),
        _ => None,
    }
}

fn linked_modpack_source_kind(
    link: &InstanceLink,
) -> Option<ContentSourceKind> {
    match link {
        InstanceLink::ModrinthModpack { .. } => {
            Some(ContentSourceKind::ModrinthModpack)
        }
        InstanceLink::ServerProjectModpack { .. } => {
            Some(ContentSourceKind::ServerProject)
        }
        InstanceLink::SharedInstance {
            modpack_project_id: Some(_),
            modpack_version_id: Some(_),
        } => Some(ContentSourceKind::ModrinthModpack),
        _ => None,
    }
}

fn check_modpack_update(
    installed_version_id: &str,
    installed_version: &Version,
    all_versions: Option<Vec<Version>>,
    preferred_update_channel: ReleaseChannel,
) -> (bool, Option<String>, Option<Version>) {
    let Some(versions) = all_versions else {
        return (false, None, None);
    };
    let installed_channel =
        ReleaseChannel::from_version_type(&installed_version.version_type);
    let effective_channel =
        preferred_update_channel.least_stable(installed_channel);

    for version_types in effective_channel.version_type_fallbacks() {
        if !versions.iter().any(|version| {
            version_types.contains(&version.version_type.as_str())
        }) {
            continue;
        }

        let mut newer_versions = versions
            .iter()
            .filter(|version| {
                version.id != installed_version_id
                    && version.date_published > installed_version.date_published
                    && version_types.contains(&version.version_type.as_str())
            })
            .collect::<Vec<_>>();
        newer_versions
            .sort_by_key(|version| std::cmp::Reverse(version.date_published));

        if let Some(newest) = newer_versions.first() {
            return (true, Some(newest.id.clone()), Some((*newest).clone()));
        }

        return (false, None, None);
    }

    (false, None, None)
}

#[derive(Clone, Debug)]
struct ModpackIdentifiers {
    hashes: HashSet<String>,
    project_ids: HashSet<String>,
}

impl ModpackIdentifiers {
    fn is_modpack_file(
        &self,
        hash: &str,
        file: Option<&CachedFile>,
        entry_project_id: Option<&str>,
    ) -> bool {
        self.hashes.contains(hash)
            || entry_project_id
                .is_some_and(|project_id| self.project_ids.contains(project_id))
            || file
                .is_some_and(|file| self.project_ids.contains(&file.project_id))
    }
}

async fn get_cached_modpack_identifiers(
    version_id: &str,
    cache_behaviour: Option<CacheBehaviour>,
    pool: &SqlitePool,
    fetch_semaphore: &FetchSemaphore,
) -> crate::Result<Option<ModpackIdentifiers>> {
    let Some(cached) = CachedEntry::get_modpack_files(
        version_id,
        cache_behaviour,
        pool,
        fetch_semaphore,
    )
    .await?
    else {
        return Ok(None);
    };

    if cached.project_ids.is_empty() {
        return Ok(None);
    }

    Ok(Some(ModpackIdentifiers {
        hashes: cached.file_hashes.into_iter().collect(),
        project_ids: cached.project_ids.into_iter().collect(),
    }))
}

async fn get_modpack_identifiers(
    version_id: &str,
    content_set: &ContentSet,
    pool: &SqlitePool,
    fetch_semaphore: &FetchSemaphore,
) -> crate::Result<ModpackIdentifiers> {
    if let Some(cached) = CachedEntry::get_modpack_files(
        version_id,
        None,
        pool,
        fetch_semaphore,
    )
    .await?
    {
        if !cached.project_ids.is_empty() {
            return Ok(ModpackIdentifiers {
                hashes: cached.file_hashes.into_iter().collect(),
                project_ids: cached.project_ids.into_iter().collect(),
            });
        }

        let hash_refs = cached
            .file_hashes
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        let files =
            CachedEntry::get_file_many(&hash_refs, None, pool, fetch_semaphore)
                .await?;
        let project_ids = files
            .iter()
            .map(|file| file.project_id.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        CachedEntry::cache_modpack_files(
            version_id,
            cached.file_hashes.clone(),
            project_ids.clone(),
            pool,
        )
        .await?;

        return Ok(ModpackIdentifiers {
            hashes: cached.file_hashes.into_iter().collect(),
            project_ids: project_ids.into_iter().collect(),
        });
    }

    let version =
        CachedEntry::get_version(version_id, None, pool, fetch_semaphore)
            .await?
            .ok_or_else(|| {
                crate::ErrorKind::InputError(format!(
                    "Modpack version {version_id} not found"
                ))
            })?;
    let primary_file = version
        .files
        .iter()
        .find(|file| file.primary)
        .or_else(|| version.files.first())
        .ok_or_else(|| {
            crate::ErrorKind::InputError(format!(
                "No files found for modpack version {version_id}"
            ))
        })?;
    let download_meta = DownloadMeta {
        reason: DownloadReason::Modpack,
        game_version: content_set.game_version.clone(),
        loader: content_set.loader.as_str().to_string(),
        dependent_on: Some(version_id.to_string()),
    };
    let mrpack_bytes = fetch_mirrors(
        &[&primary_file.url],
        primary_file.hashes.get("sha1").map(String::as_str),
        Some(&download_meta),
        None,
        fetch_semaphore,
        pool,
    )
    .await?;
    let reader = Cursor::new(&mrpack_bytes);
    let mut zip_reader =
        ZipFileReader::with_tokio(reader).await.map_err(|_| {
            crate::ErrorKind::InputError(
                "Failed to read modpack zip".to_string(),
            )
        })?;
    let manifest_idx = zip_reader
        .file()
        .entries()
        .iter()
        .position(|file| {
            matches!(file.filename().as_str(), Ok("modrinth.index.json"))
        })
        .ok_or_else(|| {
            crate::ErrorKind::InputError(
                "No modrinth.index.json found in mrpack".to_string(),
            )
        })?;
    let mut manifest = String::new();
    let mut entry_reader = zip_reader.reader_with_entry(manifest_idx).await?;
    entry_reader.read_to_string_checked(&mut manifest).await?;
    let pack: PackFormat = serde_json::from_str(&manifest)?;
    let mut hashes = pack
        .files
        .iter()
        .filter_map(|file| file.hashes.get(&PackFileHash::Sha1).cloned())
        .collect::<Vec<_>>();
    let project_ids = pack
        .files
        .iter()
        .filter_map(|file| {
            file.downloads.iter().find_map(|url| {
                let parts = url.split('/').collect::<Vec<_>>();
                let data_idx = parts.iter().position(|part| *part == "data")?;
                parts.get(data_idx + 1).map(|part| part.to_string())
            })
        })
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let override_entries = zip_reader
        .file()
        .entries()
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            let filename = entry.filename().as_str().ok()?;
            let is_override = (filename.starts_with("overrides/")
                || filename.starts_with("client-overrides/")
                || filename.starts_with("server-overrides/"))
                && !filename.ends_with('/');
            is_override.then_some(index)
        })
        .collect::<Vec<_>>();

    for index in override_entries {
        let mut file_bytes = Vec::new();
        let mut entry_reader = zip_reader.reader_with_entry(index).await?;
        entry_reader.read_to_end_checked(&mut file_bytes).await?;
        hashes.push(sha1_async(bytes::Bytes::from(file_bytes)).await?);
    }

    CachedEntry::cache_modpack_files(
        version_id,
        hashes.clone(),
        project_ids.clone(),
        pool,
    )
    .await?;

    Ok(ModpackIdentifiers {
        hashes: hashes.into_iter().collect(),
        project_ids: project_ids.into_iter().collect(),
    })
}

fn project_type_from_api_name(project_type: &str) -> ProjectType {
    ProjectType::from_name(project_type).unwrap_or(ProjectType::Mod)
}

fn sort_content_items(items: &mut [ContentItem]) {
    items.sort_by(|left, right| {
        let left_name = left
            .project
            .as_ref()
            .map(|project| project.title.as_str())
            .unwrap_or(&left.file_name);
        let right_name = right
            .project
            .as_ref()
            .map(|project| project.title.as_str())
            .unwrap_or(&right.file_name);

        left_name
            .to_lowercase()
            .cmp(&right_name.to_lowercase())
            .then_with(|| left.file_name.cmp(&right.file_name))
    });
}
