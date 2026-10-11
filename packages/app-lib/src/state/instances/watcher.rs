use crate::State;
use crate::api::instance::CONFIG_DIRECTORY;
use crate::event::InstancePayloadType;
use crate::event::emit::{emit_instance, emit_warning};
use crate::state::{
    DirectoryInfo, InstanceInstallStage, ProjectType, attached_world_data,
    libraries,
};
use crate::worlds::WorldType;
use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{DebounceEventResult, Debouncer, new_debouncer};
use std::{
    collections::{HashMap, HashSet},
    ffi::OsStr,
    path::Path,
    sync::Arc,
    time::Duration,
};
use tokio::sync::{RwLock, mpsc::channel};

use super::adapters::sqlite::instance_rows;

/// The instance a filesystem event belongs to, and the top-level entry inside
/// it that changed.
///
/// Matching is done by directory prefix against the absolute instance
/// directories the watcher registered, rather than by looking for a `profiles`
/// path component. The old approach only recognized Modrinth-format instances:
/// a `.minecraft` instance lives at `<library>/versions/<name>`, which has no
/// `profiles` component, so every one of its events was dropped. Prefix
/// matching works for both layouts and for any library root.
///
/// The longest matching directory wins, so an instance nested under another
/// path would still resolve to the closer one.
fn resolve_event_instance<'a>(
    instance_ids: &HashMap<String, String>,
    event_path: &'a Path,
) -> Option<(String, Option<&'a OsStr>)> {
    let mut best: Option<(&String, &String, usize)> = None;
    for (dir, id) in instance_ids {
        let dir_path = Path::new(dir);
        if event_path.starts_with(dir_path) {
            let depth = dir_path.components().count();
            if best
                .as_ref()
                .is_none_or(|(_, _, best_depth)| depth > *best_depth)
            {
                best = Some((dir, id, depth));
            }
        }
    }

    let (dir, id, _) = best?;
    let rest = event_path.strip_prefix(Path::new(dir)).ok()?;
    let first = rest.components().next().map(|component| component.as_os_str());
    Some((id.clone(), first))
}

pub struct FileWatcher {
    watcher: RwLock<Debouncer<RecommendedWatcher>>,
    instance_ids: Arc<RwLock<HashMap<String, String>>>,
}

pub async fn init_watcher() -> crate::Result<FileWatcher> {
    let (tx, mut rx) = channel(1);
    let instance_ids = Arc::new(RwLock::new(HashMap::<String, String>::new()));
    let event_instance_ids = instance_ids.clone();

    let file_watcher = new_debouncer(
        Duration::from_secs_f32(1.0),
        move |res: DebounceEventResult| {
            tx.blocking_send(res).ok();
        },
    )?;

    tokio::task::spawn(async move {
        let span = tracing::span!(tracing::Level::INFO, "init_watcher");
        tracing::info!(parent: &span, "Initing watcher");
        while let Some(res) = rx.recv().await {
            let _span = span.enter();

            match res {
                Ok(events) => {
                    let instance_ids = event_instance_ids.read().await;
                    let mut visited_instances = Vec::new();
                    let mut visited_screenshot_instances = Vec::new();
                    let mut pack_instances = HashSet::new();
                    let mut synced_option_files =
                        HashMap::<String, HashSet<String>>::new();

                    for e in &events {
                        let Some((instance_id, first_file_name)) =
                            resolve_event_instance(&instance_ids, &e.path)
                        else {
                            continue;
                        };
                        if first_file_name.as_ref().is_some_and(|name| {
                            *name == "resourcepacks" || *name == "datapacks"
                        }) {
                            pack_instances.insert(instance_id.clone());
                        }
                        let is_screenshot_event = first_file_name
                            .as_ref()
                            .is_some_and(|name| *name == "screenshots");
                        if let Some(file_name) = first_file_name
                            .as_ref()
                            .and_then(|name| name.to_str())
                            .filter(|name| {
                                matches!(
                                    *name,
                                    "command_history.txt"
                                        | "hotbar.nbt"
                                        | "options.txt"
                                        | "servers.dat"
                                )
                            })
                        {
                            synced_option_files
                                .entry(instance_id.clone())
                                .or_default()
                                .insert(file_name.to_owned());
                            if file_name == "options.txt" {
                                crate::api::instance::queue_game_locale_index();
                            }
                        }
                        if first_file_name
                            .as_ref()
                            .is_some_and(|x| *x == "crash-reports")
                            && e.path
                                .extension()
                                .as_ref()
                                .is_some_and(|x| *x == "txt")
                        {
                            crash_task(instance_id);
                        } else if (is_screenshot_event
                            && !visited_screenshot_instances
                                .contains(&instance_id))
                            || (!is_screenshot_event
                                && !visited_instances
                                    .contains(&instance_id))
                        {
                            let event = if first_file_name
                                .as_ref()
                                .is_some_and(|x| *x == "servers.dat")
                            {
                                Some(InstancePayloadType::ServersUpdated)
                            } else if first_file_name
                                .as_ref()
                                .is_some_and(|x| *x == "screenshots")
                            {
                                Some(
                                    InstancePayloadType::ScreenshotsUpdated,
                                )
                            } else if first_file_name.as_ref().is_some_and(
                                |x| {
                                    *x == "saves"
                                        && e.path
                                            .file_name()
                                            .as_ref()
                                            .is_some_and(|x| {
                                                *x == "level.dat"
                                            })
                                },
                            ) {
                                tracing::info!(
                                    "World updated: {}",
                                    e.path.display()
                                );
                                let world = e
                                    .path
                                    .parent()
                                    .unwrap()
                                    .file_name()
                                    .unwrap()
                                    .to_string_lossy()
                                    .to_string();
                                if !e.path.is_file() {
                                    let instance_id = instance_id.clone();
                                    let world = world.clone();
                                    tokio::spawn(async move {
                                        if let Ok(state) = State::get().await
												&& let Err(e) = attached_world_data::AttachedWorldData::remove_for_world(
													&instance_id,
													WorldType::Singleplayer,
													&world,
													&state.pool
												).await {
													tracing::warn!("Failed to remove AttachedWorldData for '{world}': {e}")
												}
                                    });
                                }
                                Some(InstancePayloadType::WorldUpdated {
                                    world,
                                })
                            } else if first_file_name.as_ref().is_none_or(
                                |x| *x != "saves" && *x != CONFIG_DIRECTORY,
                            ) {
                                Some(InstancePayloadType::Synced)
                            } else {
                                None
                            };
                            if let Some(event) = event {
                                let emit_instance_id = instance_id.clone();
                                let reconcile_screenshots = matches!(
                                    &event,
                                    InstancePayloadType::ScreenshotsUpdated
                                );
                                let sync_content = first_file_name
                                    .as_ref()
                                    .is_some_and(|name| {
                                        ProjectType::iterator().any(
                                            |project_type| {
                                                *name
                                                    == project_type
                                                        .get_folder()
                                            },
                                        )
                                    });
                                tokio::spawn(async move {
                                    if sync_content
                                        && let Ok(state) =
                                            State::get().await
                                        && let Err(error) =
                                            crate::state::resync_content_files(
                                                &emit_instance_id,
                                                &state,
                                            )
                                            .await
										{
                                        tracing::error!(
                                            "Failed to sync instance content after filesystem change: {error}"
                                        );
										}
                                    if reconcile_screenshots
                                        && let Err(error) =
                                            crate::api::instance::reconcile_screenshots(
                                                &emit_instance_id,
                                            )
                                            .await
                                    {
                                        tracing::error!(
                                            "Failed to reconcile screenshots after filesystem change: {error}"
                                        );
                                    }
                                    let _ = emit_instance(
                                        &emit_instance_id,
                                        event,
                                    )
                                    .await;
                                });
                                if is_screenshot_event {
                                    visited_screenshot_instances
                                        .push(instance_id);
                                } else {
                                    visited_instances.push(instance_id);
                                }
                            }
                        }
                    }

                    for instance_id in pack_instances {
                        crate::api::instance::queue_synced_pack_reconciliation(
                            &instance_id,
                        );
                    }
                    for (instance_id, file_names) in synced_option_files {
                        tokio::spawn(async move {
                            for file_name in file_names {
                                if let Err(error) =
                                    crate::api::instance::reconcile_synced_option_file(
                                        &instance_id,
                                        &file_name,
                                    )
                                    .await
                                {
                                    tracing::error!(
                                        "Failed to reconcile {file_name} after filesystem change: {error}"
                                    );
                                }
                            }
                        });
                    }
                }
                Err(error) => tracing::warn!("Unable to watch file: {error}"),
            }
        }
    });

    Ok(FileWatcher {
        watcher: RwLock::new(file_watcher),
        instance_ids,
    })
}

pub(crate) async fn watch_instances_init(
    watcher: &FileWatcher,
    dirs: &DirectoryInfo,
    pool: &sqlx::SqlitePool,
) {
    // DB-backed instances (legacy) come from the table.
    if let Ok(instances) = instance_rows::list_instances(pool).await {
        for instance in instances {
            watch_instance_folder(&instance.id, &instance.path, watcher, dirs)
                .await;
            if let Err(error) =
                crate::api::instance::reconcile_screenshots(&instance.id).await
            {
                tracing::warn!(
                    "Failed to reconcile screenshots for {} during watcher initialization: {error}",
                    instance.id
                );
            }
        }
    }

    // JSON-backed instances (every library instance) have no row in the table,
    // so the loop above never sees them. Without this, an instance that existed
    // before the app started is not watched at all: dropping a mod into it
    // would not refresh the content list, and world or screenshot changes would
    // not be noticed until the app restarted.
    let state = match State::get().await {
        Ok(state) => state,
        Err(error) => {
            tracing::warn!(
                "Could not enumerate JSON-backed instances to watch: {error}"
            );
            return;
        }
    };
    let json_instances = match libraries::list_instances_from_json(&state).await
    {
        Ok(instances) => instances,
        Err(error) => {
            tracing::warn!(
                "Could not enumerate JSON-backed instances to watch: {error}"
            );
            return;
        }
    };
    for instance in json_instances {
        if instance.install_stage == InstanceInstallStage::Broken {
            continue;
        }
        watch_instance_folder(&instance.id, &instance.path, watcher, dirs)
            .await;
    }
}

pub(crate) async fn watch_instance_folder(
    instance_id: &str,
    instance_path: &str,
    watcher: &FileWatcher,
    dirs: &DirectoryInfo,
) {
    let full_instance_path = libraries::resolve_instance_dir_with_dirs(
        &dirs,
        instance_path,
    );

    let Ok(metadata) = tokio::fs::metadata(&full_instance_path).await else {
        return;
    };

    if !metadata.is_dir() {
        return;
    }

    let mut to_watch = Vec::new();
    for sub_path in ProjectType::iterator().map(|x| x.get_folder()).chain([
        "crash-reports",
        "saves",
        "screenshots",
        CONFIG_DIRECTORY,
    ]) {
        let full_path = full_instance_path.join(sub_path);

        let meta = tokio::fs::symlink_metadata(&full_path).await;
        let exists = meta.is_ok();
        let is_symlink = meta.ok().is_some_and(|m| m.file_type().is_symlink());

        if !exists
            && !is_symlink
            && !sub_path.contains(".")
            && let Err(e) = crate::util::io::create_dir_all(&full_path).await
        {
            tracing::error!(
                "Failed to create directory for watcher {full_path:?}: {e}"
            );
            return;
        }

        to_watch.push(full_path);
    }

    let mut debouncer = watcher.watcher.write().await;
    for full_path in &to_watch {
        if let Err(e) = debouncer
            .watcher()
            .watch(full_path, RecursiveMode::Recursive)
        {
            tracing::error!(
                "Failed to watch directory for watcher {full_path:?}: {e}"
            );
            return;
        }
    }

    if let Err(e) = debouncer
        .watcher()
        .watch(&full_instance_path, RecursiveMode::NonRecursive)
    {
        tracing::error!(
            "Failed to watch root instance directory for watcher {full_instance_path:?}: {e}"
        );
    }

    // Keyed by the absolute directory the events arrive under, since the
    // handler matches an event path by prefix. A relative path would never
    // match a notify event, which is always absolute.
    watcher
        .instance_ids
        .write()
        .await
        .insert(
            full_instance_path.to_string_lossy().to_string(),
            instance_id.to_string(),
        );
}

fn crash_task(instance_id: String) {
    tokio::task::spawn(async move {
        let res = async {
            let state = State::get().await?;
            let Some(instance) =
                instance_rows::get_instance_by_id(&instance_id, &state.pool)
                    .await?
            else {
                return Ok(());
            };

            if instance.install_stage == InstanceInstallStage::Installed {
                emit_warning(&format!(
					"Instance {} has crashed! Visit the logs page to see a crash report.",
					instance.name
				))
				.await?;
            }

            Ok::<(), crate::Error>(())
        }
        .await;

        match res {
            Ok(()) => {}
            Err(err) => {
                tracing::warn!("Unable to send crash report to frontend: {err}")
            }
        };
    });
}
