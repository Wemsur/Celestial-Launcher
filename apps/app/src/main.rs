#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]
#![recursion_limit = "256"]

use native_dialog::{DialogBuilder, MessageLevel};
use std::env;
use std::sync::atomic::Ordering;
use tauri::{Listener, Manager};
use tauri_plugin_fs::FsExt;
use theseus::prelude::*;
use std::fs;
use std::path::PathBuf;
use base64::{Engine as _, engine::general_purpose}; // 需要在 Cargo.toml 引入 base64 库

mod api;
mod error;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(feature = "updater")]
mod updater_impl;
#[cfg(not(feature = "updater"))]
mod updater_impl_noop;

// Should be called in launcher initialization
#[tracing::instrument(skip_all)]
#[tauri::command]
async fn initialize_state(app: tauri::AppHandle) -> api::Result<()> {
    tracing::info!("Initializing app event state...");
    theseus::EventState::init(app.clone()).await?;

    tracing::info!("Initializing app state...");
    State::init(app.config().identifier.clone()).await?;

    let state = State::get().await?;
    app.asset_protocol_scope()
        .allow_directory(state.directories.caches_dir(), true)?;
    app.asset_protocol_scope()
        .allow_directory(state.directories.caches_dir().join("icons"), true)?;
    app.fs_scope()
        .allow_directory(state.directories.instances_dir(), true)?;

    Ok(())
}

// Should be call once Vue has mounted the app
#[tracing::instrument(skip_all)]
#[tauri::command]
fn show_window(app: tauri::AppHandle) {
    let win = app.get_window("main").unwrap();
    if let Err(e) = win.show() {
        DialogBuilder::message()
            .set_level(MessageLevel::Error)
            .set_title("Initialization error")
            .set_text(format!(
                "Cannot display application window due to an error:\n{e}"
            ))
            .alert()
            .show()
            .unwrap();
        panic!("cannot display application window")
    } else {
        let _ = win.set_focus();
    }
}

#[tauri::command]
fn is_dev() -> bool {
    cfg!(debug_assertions)
}

#[tauri::command]
fn are_updates_enabled() -> bool {
    cfg!(feature = "updater")
        && env::var("MODRINTH_EXTERNAL_UPDATE_PROVIDER").is_err()
}

#[cfg(feature = "updater")]
pub use updater_impl::*;

#[cfg(not(feature = "updater"))]
pub use updater_impl_noop::*;

// Toggles decorations
#[tauri::command]
async fn toggle_decorations(b: bool, window: tauri::Window) -> api::Result<()> {
    window.set_decorations(b).map_err(|e| {
        theseus::Error::from(theseus::ErrorKind::OtherError(format!(
            "Failed to toggle decorations: {e}"
        )))
    })?;
    Ok(())
}

#[tauri::command]
fn restart_app(app: tauri::AppHandle) {
    app.restart();
}

#[tauri::command]
async fn set_restart_after_pending_update(
    should_restart: bool,
) -> api::Result<()> {
    let state = State::get().await?;
    state
        .restart_after_pending_update
        .store(should_restart, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
async fn save_background_image(
    app_handle: tauri::AppHandle,
    background_blob: Vec<u8>,
    file_name: String,
) -> Result<String, String> {
    // 1. 获取应用专属的本地数据存储目录 (例如 Windows 下的 AppData/Roaming/ theseus_gui)
    let mut target_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法获取 App Data 目录: {}", e))?;

    // 2. 创建一个名为 custom_backgrounds 的专用子文件夹
    target_dir.push("custom_backgrounds");
    if !target_dir.exists() {
        fs::create_dir_all(&target_dir)
            .map_err(|e| format!("创建背景图存储文件夹失败: {}", e))?;
    }

    // 3. 提取文件后缀，固定命名或保留原名。这里为了防止命名冲突导致覆盖，可以使用固定名称
    // 或者你可以直接用传入的 file_name
    let extension = std::path::Path::new(&file_name)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("png");

    let final_file_name = format!("current_background.{}", extension);
    target_dir.push(final_file_name);

    // 4. 将前端传过来的二进制字节流写入磁盘
    fs::write(&target_dir, background_blob)
        .map_err(|e| format!("写入图片文件流失败: {}", e))?;

    println!("[Rust Backend] 成功持久化背景图至: {:?}", target_dir);

    // 5. 返回保存成功的绝对路径字符串供前端记录或备用
    target_dir
        .to_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "路径包含非 UTF-8 字符".to_string())
}

#[tauri::command]
fn get_current_background_path(app_handle: tauri::AppHandle) -> Result<String, String> {
    let mut path = app_handle.path().app_data_dir().map_err(|e| e.to_string())?;
    path.push("custom_backgrounds");
    path.push("current_background.jpg"); // 建议这里做个简单的文件存在性检查

    if path.exists() {
        Ok(path.to_str().unwrap().to_string())
    } else {
        Err("No background found".into())
    }
}

#[tauri::command]
async fn get_background_as_base64(app_handle: tauri::AppHandle) -> Result<String, String> {
    let mut path = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;

    path.push("custom_backgrounds");
    path.push("current_background.jpg"); // 优先找 jpg

    if !path.exists() {
        // 如果 jpg 不存在，尝试 png
        path.set_extension("png");
        if !path.exists() {
            return Err("Background file not found".into());
        }
    }

    // 读取文件并转为 Base64
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let base64_str = general_purpose::STANDARD.encode(bytes);

    Ok(format!("data:image/jpeg;base64,{}", base64_str))
}

#[tauri::command]
fn delete_background(app_handle: tauri::AppHandle) -> Result<(), String> {
    let mut path = app_handle.path().app_data_dir().map_err(|e| e.to_string())?;
    path.push("custom_backgrounds");
    path.push("current_background.jpg");

    if path.exists() {
        fs::remove_file(path).map_err(|e| e.to_string())?;
        Ok(())
    } else {
        Err("背景文件不存在".to_string())
    }
}


// if Tauri app is called with arguments, then those arguments will be treated as commands
// ie: deep links or filepaths for .mrpacks
fn main() {
    /*
        tracing is set basd on the environment variable RUST_LOG=xxx, depending on the amount of logs to show
            ERROR > WARN > INFO > DEBUG > TRACE
        eg. RUST_LOG=info will show info, warn, and error logs
            RUST_LOG="theseus=trace" will show *all* messages but from theseus only (and not dependencies using similar crates)
            RUST_LOG="theseus=trace" will show *all* messages but from theseus only (and not dependencies using similar crates)

        Error messages returned to Tauri will display as traced error logs if they return an error.
        This will also include an attached span trace if the error is from a tracing error, and the level is set to info, debug, or trace

        on unix:
            RUST_LOG="theseus=trace" {run command}

    */

    let tauri_context = tauri::generate_context!();

    let _log_guard = theseus::start_logger(&tauri_context.config().identifier);

    tracing::info!("Initialized tracing subscriber. Loading Modrinth App!");

    let mut builder = tauri::Builder::default();

    #[cfg(feature = "updater")]
    {
        use tauri_plugin_http::reqwest::header::{HeaderValue, USER_AGENT};
        use theseus::launcher_user_agent;
        builder = builder.plugin(
            tauri_plugin_updater::Builder::new()
                .header(
                    USER_AGENT,
                    HeaderValue::from_str(&launcher_user_agent()).unwrap(),
                )
                .unwrap()
                .build(),
        );
    }

    builder = builder
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if let Some(payload) = args.get(1) {
                tracing::info!("Handling deep link from arg {payload}");
                let payload = payload.clone();
                tauri::async_runtime::spawn(api::utils::handle_command(
                    payload,
                ));
            }

            if let Some(win) = app.get_window("main") {
                let _ = win.set_focus();
            }
        }))
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_filename("app-window-state.json")
                // Use *only* POSITION and SIZE state flags, because saving VISIBLE causes the `visible: false` to not take effect
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::POSITION
                        | tauri_plugin_window_state::StateFlags::SIZE
                        | tauri_plugin_window_state::StateFlags::MAXIMIZED,
                )
                .build(),
        )
        .setup(|app| {
            #[cfg(target_os = "macos")]
            {
                let payload = macos::deep_link::get_or_init_payload(app);

                let mtx_copy = payload.payload;
                app.listen("deep-link://new-url", move |url| {
                    let mtx_copy_copy = mtx_copy.clone();
                    let request = url.payload().to_owned();

                    let actual_request =
                        serde_json::from_str::<Vec<String>>(&request)
                            .ok()
                            .map(|mut x| x.remove(0))
                            .unwrap_or(request);

                    tauri::async_runtime::spawn(async move {
                        tracing::info!("Handling deep link {actual_request}");

                        let mut payload = mtx_copy_copy.lock().await;
                        if payload.is_none() {
                            *payload = Some(actual_request.clone());
                        }

                        let _ =
                            api::utils::handle_command(actual_request).await;
                    });
                });
            };

            #[cfg(not(target_os = "macos"))]
            app.listen("deep-link://new-url", |url| {
                let payload = url.payload().to_owned();
                tracing::info!("Handling deep link {payload}");
                tauri::async_runtime::spawn(api::utils::handle_command(
                    payload,
                ));
            });

            #[cfg(not(target_os = "linux"))]
            if let Some(window) = app.get_window("main")
                && let Err(e) = window.set_shadow(true)
            {
                tracing::warn!("Failed to set window shadow: {e}");
            }

            Ok(())
        });

    builder = builder
        .plugin(api::auth::init())
        .plugin(api::mr_auth::init())
        .plugin(api::import::init())
        .plugin(api::install::init())
        .plugin(api::instance::init())
        .plugin(api::logs::init())
        .plugin(api::jre::init())
        .plugin(api::metadata::init())
        .plugin(api::minecraft_skins::init())
        .plugin(api::process::init())
        .plugin(api::settings::init())
        .plugin(api::shortcuts::init())
        .plugin(api::tags::init())
        .plugin(api::utils::init())
        .plugin(api::cache::init())
        .plugin(api::files::init())
        .plugin(api::ads::init())
        .plugin(api::friends::init())
        .plugin(api::worlds::init())
        .manage(PendingUpdateData::default())
        .invoke_handler(tauri::generate_handler![
            initialize_state,
            is_dev,
            are_updates_enabled,
            get_update_size,
            enqueue_update_for_installation,
            remove_enqueued_update,
            set_restart_after_pending_update,
            toggle_decorations,
            show_window,
            restart_app,
            save_background_image,
            get_background_as_base64,
            delete_background,
        ]);

    tracing::info!("Initializing app...");
    let app = builder.build(tauri_context);

    match app {
        Ok(app) => {
            app.run(|app, event| {
                #[cfg(not(any(feature = "updater", target_os = "macos")))]
                let _ = app;

                if matches!(&event, tauri::RunEvent::ExitRequested { .. })
                    && let Err(error) = tauri::async_runtime::block_on(
                        theseus::minecraft_skins::flush_pending_skin_change(),
                    )
                {
                    tracing::warn!(
                        "Failed to flush pending Minecraft skin change before exit: {error}"
                    );
                }

                #[cfg(feature = "updater")]
                if matches!(&event, tauri::RunEvent::Exit) {
                    let update_data = app.state::<PendingUpdateData>().inner();
                    let should_restart = State::get_if_initialized()
                        .map(|s| {
                            s.restart_after_pending_update.load(Ordering::Relaxed)
                        })
                        .unwrap_or(false);
                    if let Some((update, data)) = &*update_data.0.lock().unwrap()
                    {
                        fn set_changelog_toast(version: Option<String>) {
                            let toast_result: theseus::Result<()> = tauri::async_runtime::block_on(async move {
                                let mut settings = settings::get().await?;
                                settings.pending_update_toast_for_version = version;
                                settings::set(settings).await?;
                                Ok(())
                            });
                            if let Err(e) = toast_result {
                                tracing::warn!(
                                    "Failed to set pending_update_toast: {e}"
                                )
                            }
                        }

                        set_changelog_toast(Some(update.version.clone()));
                        let update = if should_restart {
                            (**update).clone()
                        } else {
                            (**update).clone().restart_after_install(false)
                        };
                        match update.install(data) {
                            Ok(()) => {
                                if should_restart {
                                    tracing::info!(
                                        "Pending update installed successfully (version {}); restarting because user requested reload",
                                        update.version
                                    );
                                    app.restart();
                                } else {
                                    tracing::info!(
                                        "Pending update installed successfully (version {}); exiting without relaunch (user did not request reload)",
                                        update.version
                                    );
                                }
                            }
                            Err(e) => {
                                tracing::error!(
                                    "Pending update install failed (version {}): {e}",
                                    update.version
                                );
                                set_changelog_toast(None);

                                DialogBuilder::message()
                                    .set_level(MessageLevel::Error)
                                    .set_title("Update error")
                                    .set_text(format!("Failed to install update due to an error:\n{e}"))
                                    .alert()
                                    .show()
                                    .unwrap();
                            }
                        }
                    }
                }
                #[cfg(target_os = "macos")]
                if let tauri::RunEvent::Opened { urls } = event {
                    tracing::info!("Handling webview open {urls:?}");

                    let file = urls
                        .into_iter()
                        .find_map(|url| url.to_file_path().ok());

                    if let Some(file) = file {
                        let payload =
                            macos::deep_link::get_or_init_payload(app);

                        let mtx_copy = payload.payload;
                        let request = file.to_string_lossy().to_string();
                        tauri::async_runtime::spawn(async move {
                            let mut payload = mtx_copy.lock().await;
                            if payload.is_none() {
                                *payload = Some(request.clone());
                            }

                            let _ = api::utils::handle_command(request).await;
                        });
                    }
                }
            });
        }
        Err(e) => {
            tracing::error!("Error while running tauri application: {:?}", e);

            #[cfg(target_os = "windows")]
            {
                // tauri doesn't expose runtime errors, so matching a string representation seems like the only solution
                if format!("{e:?}").contains(
                    "Runtime(CreateWebview(WebView2Error(WindowsError",
                ) {
                    DialogBuilder::message()
                        .set_level(MessageLevel::Error)
                        .set_title("Initialization error")
                        .set_text("Your Microsoft Edge WebView2 installation is corrupt.\n\nMicrosoft Edge WebView2 is required to run Modrinth App.\n\nLearn how to repair it at https://support.modrinth.com/en/articles/8797765-corrupted-microsoft-edge-webview2-installation")
                        .alert()
                        .show()
                        .unwrap();

                    panic!("webview2 initialization failed")
                }
            }

            DialogBuilder::message()
                .set_level(MessageLevel::Error)
                .set_title("Initialization error")
                .set_text(format!(
                    "Cannot initialize application due to an error:\n{e:?}"
                ))
                .alert()
                .show()
                .unwrap();

            panic!("{1}: {:?}", e, "error while running tauri application")
        }
    }
}
