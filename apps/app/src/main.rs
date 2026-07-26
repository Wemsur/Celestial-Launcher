#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]
#![recursion_limit = "256"]

use native_dialog::{DialogBuilder, MessageLevel};
use std::{env, io};
use std::sync::atomic::Ordering;
use tauri::{Listener, Manager};
use tauri_plugin_fs::FsExt;
use theseus::prelude::*;
use std::fs;
use std::path::PathBuf;
use base64::{Engine as _, engine::general_purpose};

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


#[derive(serde::Serialize)]
struct CheckImportResponse {
    should_show: bool,
}
// ──────────────── Import commands ────────────────

#[tauri::command]
async fn check_for_import(
    app_handle: tauri::AppHandle,
) -> Result<CheckImportResponse, String> {
    // 获取 Celestial Launcher 的路径
    let new_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法获取 AppData 目录: {}", e))?;

    // 检查数据库是否存在实例（直接查实例文件夹）
    let mut instances_dir = new_data_dir.clone();
    instances_dir.push("instances");

    if instances_dir.exists() {
        if let Ok(entries) = fs::read_dir(&instances_dir) {
            let has_instances = entries.flatten().any(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false));
            if has_instances {
                return Ok(CheckImportResponse { should_show: false });
            }
        }
    }

    // 没有实例，检查 ModrinthApp 的旧路径是否存在
    let old_base = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法获取 AppData 目录: {}", e))?;

    // old_base 已经是 CelestialLauncher 所在目录
    // 需要到上一级去找 "ModrinthApp"
    let mut modrinth_app_dir = old_base.parent()
        .ok_or_else(|| "无法获取父目录".to_string())?
        .to_path_buf();
    modrinth_app_dir.push("ModrinthApp");

    // 检查 app.db 是否存在
    let db_path = modrinth_app_dir.join("app.db");
    if !db_path.exists() {
        return Ok(CheckImportResponse { should_show: false });
    }

    Ok(CheckImportResponse { should_show: true })
}

#[tauri::command]
async fn import_old_data(
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法获取 AppData 目录: {}", e))?;

    let mut modrinth_app_dir = data_dir.parent()
        .ok_or_else(|| "无法获取父目录".to_string())?
        .to_path_buf();
    modrinth_app_dir.push("ModrinthApp");

    // 复制 custom_backgrounds 目录（不被锁，直接复制）
    let old_bg = modrinth_app_dir.join("custom_backgrounds");
    if old_bg.exists() {
        copy_dir_all(&old_bg, &data_dir.join("custom_backgrounds"))
            .map_err(|e| format!("复制 custom_backgrounds 失败: {}", e))?;
    }

    Ok(())
}

fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> io::Result<()> {
    fs::create_dir_all(&dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

#[tauri::command]
async fn set_dont_show_import_modal(
    app_handle: tauri::AppHandle,
    value: bool,
) -> Result<(), String> {
    let mut config_path = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法获取 AppData 目录: {}", e))?;

    config_path.push("custom_backgrounds");
    if !config_path.exists() {
        fs::create_dir_all(&config_path)
            .map_err(|e| format!("创建配置文件夹失败: {}", e))?;
    }

    config_path.push("celestial_settings.json");

    // 读取现有 JSON
    let existing = if config_path.exists() {
        let content = fs::read_to_string(&config_path).unwrap_or_else(|_| "{}".to_string());
        serde_json::from_str::<serde_json::Value>(&content).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    // 写入 new_import_banner
    let mut updated = existing.clone();
    updated["new_import_banner"] = serde_json::json!(value); // true = 不再显示

    fs::write(&config_path, serde_json::to_string_pretty(&updated).map_err(|e| format!("序列化失败: {}", e))?)
        .map_err(|e| format!("写入配置文件失败: {}", e))?;

    println!("[Rust Backend] 已保存不再显示标记: {}", value);
    Ok(())
}


#[tauri::command]
async fn get_import_banner_setting(
    app_handle: tauri::AppHandle,
) -> Result<bool, String> {
    let mut config_path = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法获取 AppData 目录: {}", e))?;

    config_path.push("custom_backgrounds");
    config_path.push("celestial_settings.json");

    if !config_path.exists() {
        return Ok(false); // 没有设置文件，说明是首次启动或无记录
    }

    let content = fs::read_to_string(&config_path)
        .map_err(|e| format!("读取配置文件失败: {}", e))?;

    let json: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("JSON解析失败: {}", e))?;

    // new_import_banner 为 true 表示"不再显示"
    Ok(json.get("new_import_banner")
        .and_then(|v| v.as_bool())
        .unwrap_or(false))
}

#[tauri::command]
async fn do_import_and_restart(
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法获取 AppData 目录: {}", e))?;

    let mut modrinth_app_dir = data_dir.parent()
        .ok_or_else(|| "无法获取父目录".to_string())?
        .to_path_buf();
    modrinth_app_dir.push("ModrinthApp");

    // --- 阶段1: 写入 PowerShell 脚本（硬编码完整复制+关闭+重启逻辑）---
    let temp_dir = std::env::temp_dir();
    let script_path = temp_dir.join("celestial_import.ps1");
    let modrinth_str = modrinth_app_dir.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
    let data_str = data_dir.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");

    let script = format!(
        r#"Start-Sleep -Seconds 3

# 复制 app.db
$srcDb = "{modrinth}\app.db"
$dstDb = "{data}\app.db"
if (Test-Path $srcDb) {{
    if (Test-Path $dstDb) {{ Remove-Item $dstDb -Force }}
    Copy-Item $srcDb $dstDb -Force
}}

# 复制 app.db-shm
$srcShm = "{modrinth}\app.db-shm"
$dstShm = "{data}\app.db-shm"
if (Test-Path $srcShm) {{
    if (Test-Path $dstShm) {{ Remove-Item $dstShm -Force }}
    Copy-Item $srcShm $dstShm -Force
}}

# 复制 app.db-wal
$srcWal = "{modrinth}\app.db-wal"
$dstWal = "{data}\app.db-wal"
if (Test-Path $srcWal) {{
    if (Test-Path $dstWal) {{ Remove-Item $dstWal -Force }}
    Copy-Item $srcWal $dstWal -Force
}}

Write-Host "Import completed successfully"
"#,
        modrinth = modrinth_str,
        data = data_str,
    );

    std::fs::write(&script_path, &script)
        .map_err(|e| format!("写入脚本失败: {}", e))?;

    // --- 阶段2: 后台启动脚本 ---
    std::process::Command::new("powershell")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-WindowStyle")
        .arg("Hidden")
        .arg("-File")
        .arg(&script_path)
        .spawn()
        .map_err(|e| format!("启动脚本失败: {}", e))?;

    // --- 阶段3: 退出当前应用（不做 restart，因为 dev 模式下 restart 不会重启 Vite）---
    std::process::exit(0);
}

fn copy_file_exclusive(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    // 如果目标文件已被占用，先尝试删除（覆盖式复制）
    if dst.exists() {
        let _ = fs::remove_file(dst);
    }
    fs::copy(src, dst)
        .map_err(|e| format!("复制 {} 失败: {}", src.file_name().unwrap_or_default().to_string_lossy(), e))?;
    Ok(())
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
    // 1. 获取应用专属的本地数据存储目录
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
    //清除旧的背景文件
    if let Ok(entries) = fs::read_dir(&target_dir) {
        for entry in entries.flatten() {
            if let Ok(file_type) = entry.file_type() {
                if file_type.is_file() {
                    if let Some(name) = entry.file_name().to_str() {
                        if name.starts_with("current_background.") {
                            let _ = fs::remove_file(entry.path());
                        }
                    }
                }
            }
        }
    }
    // 3. 提取文件后缀
    let extension = std::path::Path::new(&file_name)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("png");

    let final_file_name = format!("current_background.{}", extension);
    target_dir.push(final_file_name);

    // 4. 写入磁盘
    fs::write(&target_dir, background_blob)
        .map_err(|e| format!("写入图片文件流失败: {}", e))?;

    println!("[Rust Backend] 成功持久化背景图至: {:?}", target_dir);

    // 5. 返回路径
    target_dir
        .to_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "路径包含非 UTF-8 字符".to_string())
}


// 1. 保存开关状态到文件
#[tauri::command(rename_all = "camelCase")]
async fn save_bg_blur_status(app_handle: tauri::AppHandle, is_active: bool) -> Result<(), String> {
    // 1. 获取应用专属的本地数据存储目录
    let mut config_path = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法获取 App Data 目录: {}", e))?;

    config_path.push("custom_backgrounds");
    if !config_path.exists() {
        fs::create_dir_all(&config_path)
            .map_err(|e| format!("创建配置文件夹失败: {}", e))?;
    }

    // 2. 读取现有配置文件（如果存在）
    config_path.push("celestial_settings.json");

    let existing = if config_path.exists() {
        let content = fs::read_to_string(&config_path).unwrap_or_else(|_| "{}".to_string());
        serde_json::from_str::<serde_json::Value>(&content).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    // 3. 更新 blur_enabled 字段后写回
    let mut updated = existing.clone();
    updated["blur_enabled"] = serde_json::json!(is_active);

    fs::write(&config_path, serde_json::to_string_pretty(&updated).map_err(|e| format!("序列化失败: {}", e))?)
        .map_err(|e| format!("写入配置文件失败: {}", e))?;

    println!("[Rust Backend] 模糊状态已保存到 celestial_settings.json: {}", is_active);
    Ok(())
}

// 2. 读取开关状态
#[tauri::command]
async fn load_bg_blur_status(app_handle: tauri::AppHandle) -> Result<bool, String> {
    // 1. 获取配置路径
    let mut config_path = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法获取 App Data 目录: {}", e))?;

    config_path.push("custom_backgrounds");
    config_path.push("celestial_settings.json");

    // 2. 文件不存在时返回默认值 true
    if !config_path.exists() {
        return Ok(true);
    }

    // 3. 读取并解析 JSON
    let content = fs::read_to_string(&config_path)
        .map_err(|e| format!("读取配置文件失败: {}", e))?;

    let json: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("JSON解析失败: {}", e))?;

    Ok(json.get("blur_enabled").and_then(|v| v.as_bool()).unwrap_or(true))
}

// 3. 保存色相值
#[tauri::command]
async fn save_hue_value(app_handle: tauri::AppHandle, hue_value: u32) -> Result<(), String> {
    // 确保 hue 范围在 0-360
    if hue_value > 360 {
        return Err("Hue value must be between 0 and 360".to_string());
    }

    let mut config_path = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法获取 App Data 目录: {}", e))?;

    config_path.push("custom_backgrounds");
    if !config_path.exists() {
        fs::create_dir_all(&config_path)
            .map_err(|e| format!("创建配置文件夹失败: {}", e))?;
    }

    config_path.push("celestial_settings.json");

    // 读取现有配置
    let existing = if config_path.exists() {
        let content = fs::read_to_string(&config_path).unwrap_or_else(|_| "{}".to_string());
        serde_json::from_str::<serde_json::Value>(&content).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    // 更新 hue_value 字段后写回
    let mut updated = existing.clone();
    updated["hue_value"] = serde_json::json!(hue_value);

    fs::write(&config_path, serde_json::to_string_pretty(&updated).map_err(|e| format!("序列化失败: {}", e))?)
        .map_err(|e| format!("写入配置文件失败: {}", e))?;

/*    println!("[Rust Backend] 色相值已保存到 celestial_settings.json: {}", hue_value);*/
    Ok(())
}

// 4. 加载色相值
#[tauri::command]
async fn load_hue_value(app_handle: tauri::AppHandle) -> Result<u32, String> {
    let mut config_path = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法获取 App Data 目录: {}", e))?;

    config_path.push("custom_backgrounds");
    config_path.push("celestial_settings.json");

    if !config_path.exists() {
        return Ok(38); // 默认值
    }

    let content = fs::read_to_string(&config_path)
        .map_err(|e| format!("读取配置文件失败: {}", e))?;

    let json: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("JSON解析失败: {}", e))?;

    Ok(json.get("hue_value").and_then(|v| v.as_u64()).unwrap_or(0) as u32)
}

#[tauri::command]
async fn get_background_as_base64(app_handle: tauri::AppHandle) -> Result<String, String> {
    let mut path = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;

    path.push("custom_backgrounds");

    if !path.exists() {
        return Err("Background folder not found".into());
    }

    // 自动寻找当前存在的任意后缀背景
    let mut found_path = None;
    let mut mime_type = String::from("jpeg"); // 默认回退值

    if let Ok(entries) = std::fs::read_dir(&path) {
        for entry in entries.flatten() {
            if let Ok(file_type) = entry.file_type() {
                if file_type.is_file() {
                    if let Some(file_name) = entry.file_name().to_str() {
                        if file_name.starts_with("current_background.") {
                            found_path = Some(entry.path());
                            // 提取后缀并构建合法的 MIME-Type
                            if let Some(ext) = entry.path().extension().and_then(|e| e.to_str()) {
                                mime_type = if ext == "jpg" { "jpeg".to_string() } else { ext.to_string() };
                            }
                            break;
                        }
                    }
                }
            }
        }
    }

    let bg_path = found_path.ok_or_else(|| "Background file not found".to_string())?;

    // 读取文件并转为 Base64
    let bytes = std::fs::read(bg_path).map_err(|e| e.to_string())?;
    let base64_str = general_purpose::STANDARD.encode(bytes);

    // 动态返回正确的 mime type（如 image/png，image/webp）
    Ok(format!("data:image/{};base64,{}", mime_type, base64_str))
}

#[tauri::command]
fn delete_background(app_handle: tauri::AppHandle) -> Result<(), String> {
    let mut path = app_handle.path().app_data_dir().map_err(|e| e.to_string())?;
    path.push("custom_backgrounds");

    if path.exists() {
        let mut deleted = false;
        // 遍历并删除以 current_background. 开头的文件
        if let Ok(entries) = fs::read_dir(&path) {
            for entry in entries.flatten() {
                if let Ok(file_type) = entry.file_type() {
                    if file_type.is_file() {
                        if let Some(file_name) = entry.file_name().to_str() {
                            if file_name.starts_with("current_background.") {
                                if fs::remove_file(entry.path()).is_ok() {
                                    deleted = true;
                                }
                            }
                        }
                    }
                }
            }
        }

        if deleted {
            Ok(())
        } else {
            Err("背景文件不存在".to_string())
        }
    } else {
        Err("背景文件夹不存在".to_string())
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
            save_bg_blur_status,
            load_bg_blur_status,
            save_hue_value,
            load_hue_value,
            check_for_import,
            import_old_data,
            set_dont_show_import_modal,
            get_import_banner_setting,
            do_import_and_restart,
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
