#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]
#![recursion_limit = "256"]

use native_dialog::{DialogBuilder, MessageLevel};
use std::{env, io};
#[cfg(feature = "updater")]
use std::sync::atomic::Ordering;
use tauri::{Emitter, Listener, Manager};
use tauri_plugin_fs::FsExt;
use theseus::prelude::*;
use std::fs;
use base64::{Engine as _, engine::general_purpose};

mod api;
mod update_security;

#[cfg(target_os = "macos")]
mod macos;

// Should be called in launcher initialization
#[tracing::instrument(skip_all)]
#[tauri::command]
async fn initialize_state(
    app: tauri::AppHandle,
    events: tauri::ipc::Channel<tauri::ipc::InvokeResponseBody>,
) -> api::Result<()> {
    tracing::info!("Initializing app event state...");
    theseus::EventState::init(app.clone(), events).await?;

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

// ── celestial.db 上的界面偏好存取 ──────────────────────────────────────────
// 原先散落在 custom_backgrounds/celestial_settings.json、interface/*.json、
// translation/*.json 里的 fork 自有偏好，统一迁到 celestial.db 的 ui_settings
// 表（KV）。前端命令与 schema 完全不变，Rust 只是从"整文件读写"换成"按键读写"。
async fn ui_settings_get(key: &str) -> Result<Option<String>, String> {
    let state = State::get()
        .await
        .map_err(|e| format!("获取应用状态失败: {}", e))?;
    theseus::celestial_db::ui_settings::get_json(state.celestial.pool(), key)
        .await
        .map_err(|e| format!("读取界面设置失败: {}", e))
}

async fn ui_settings_set(key: &str, value: String) -> Result<(), String> {
    let state = State::get()
        .await
        .map_err(|e| format!("获取应用状态失败: {}", e))?;
    theseus::celestial_db::ui_settings::set_json(
        state.celestial.pool(),
        key,
        &value,
    )
    .await
    .map_err(|e| format!("写入界面设置失败: {}", e))
}

/// celestial_settings.json 里共享文档的按键，迁库后各自成为独立 KV 键。
async fn get_bool_setting(key: &str, default: bool) -> Result<bool, String> {
    Ok(ui_settings_get(key)
        .await?
        .map(|v| v == "true")
        .unwrap_or(default))
}

async fn set_bool_setting(key: &str, value: bool) -> Result<(), String> {
    ui_settings_set(key, value.to_string()).await
}

/// State 里的 settings_dir。生产环境下与 Tauri 的 app_data_dir 相同；dev 下
/// THESEUS_CONFIG_DIR 会把数据目录整体隔离出去，所以涉及数据目录的文件类命令
/// 必须走这里，而不是 app_data_dir。State 未就绪时等待初始化完成。
async fn settings_dir() -> Result<std::path::PathBuf, String> {
    let state = State::get()
        .await
        .map_err(|e| format!("获取应用状态失败: {}", e))?;
    Ok(state.directories.settings_dir.clone())
}

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
    let new_data_dir = settings_dir().await?;

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
    let old_base = new_data_dir.clone();

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
) -> Result<(), String> {
    let data_dir = settings_dir().await?;

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
    value: bool,
) -> Result<(), String> {
    set_bool_setting("new_import_banner", value).await
}


#[tauri::command]
async fn get_import_banner_setting(
) -> Result<bool, String> {
    // new_import_banner 为 true 表示"不再显示"
    get_bool_setting("new_import_banner", false).await
}

#[tauri::command]
async fn do_import_and_restart(
) -> Result<(), String> {
    let data_dir = settings_dir().await?;

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

#[tauri::command]
async fn save_background_image(
    background_blob: Vec<u8>,
    file_name: String,
) -> Result<String, String> {
    // 1. 获取应用专属的本地数据存储目录
    let mut target_dir = settings_dir().await?;

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
async fn save_bg_blur_status(_app_handle: tauri::AppHandle, is_active: bool) -> Result<(), String> {
    set_bool_setting("blur_enabled", is_active).await
}

// 2. 读取开关状态
#[tauri::command]
async fn load_bg_blur_status(_app_handle: tauri::AppHandle) -> Result<bool, String> {
    // 缺省为 true —— 模糊是默认外观
    get_bool_setting("blur_enabled", true).await
}

// 实例卡片背景：把图标做成卡片底纹。和 blur_enabled 一样是独立 KV 键。
#[tauri::command(rename_all = "camelCase")]
async fn save_instance_card_icon_bg(
    _app_handle: tauri::AppHandle,
    is_active: bool,
) -> Result<(), String> {
    set_bool_setting("instance_card_icon_bg", is_active).await
}

/// 键缺失时返回 true —— 这个底纹是默认外观，没写过配置的用户也该看到。
#[tauri::command]
async fn load_instance_card_icon_bg(
    _app_handle: tauri::AppHandle,
) -> Result<bool, String> {
    get_bool_setting("instance_card_icon_bg", true).await
}

// 3. 保存色相值
#[tauri::command]
async fn save_hue_value(_app_handle: tauri::AppHandle, hue_value: u32) -> Result<(), String> {
    // 确保 hue 范围在 0-360
    if hue_value > 360 {
        return Err("Hue value must be between 0 and 360".to_string());
    }

    ui_settings_set("hue_value", hue_value.to_string()).await
}

// 4. 加载色相值
#[tauri::command]
async fn load_hue_value(_app_handle: tauri::AppHandle) -> Result<u32, String> {
    // 无记录时返回默认值 38
    let raw = ui_settings_get("hue_value").await?.unwrap_or_else(|| "38".to_string());
    Ok(raw.parse::<u32>().unwrap_or(38))
}

// 5. 保存 Library 每种排序方式的正序/倒序状态
#[tauri::command]
async fn save_library_sort_directions(
    _app_handle: tauri::AppHandle,
    directions: std::collections::HashMap<String, String>,
) -> Result<(), String> {
    let json = serde_json::to_string(&directions)
        .map_err(|e| format!("序列化失败: {}", e))?;
    ui_settings_set("library_sort_directions", json).await
}

// 6. 读取 Library 每种排序方式的正序/倒序状态
#[tauri::command]
async fn load_library_sort_directions(
    _app_handle: tauri::AppHandle,
) -> Result<std::collections::HashMap<String, String>, String> {
    // 无记录时返回空表，前端会退回到每种排序方式的默认方向
    let Some(raw) = ui_settings_get("library_sort_directions").await? else {
        return Ok(std::collections::HashMap::new());
    };

    serde_json::from_str(&raw).map_err(|e| format!("JSON解析失败: {}", e))
}

// 7. 翻译功能的存储：译文服务设置进 ui_settings 表，译文缓存进
//    celestial.db 的 translation_cache 表（取代 translation/*.json）。
//    只负责存取整个文档，schema 和 7 天过期清理都由前端负责，
//    这样以后加字段不用改 Rust。

#[tauri::command]
async fn load_translation_settings(
) -> Result<String, String> {
    // 无记录时返回空串，前端据此回退到默认值。
    Ok(ui_settings_get(theseus::celestial_db::translation_cache::KEY_TRANSLATION_SETTINGS)
        .await?
        .unwrap_or_default())
}

#[tauri::command]
async fn save_translation_settings(
    contents: String,
) -> Result<(), String> {
    ui_settings_set(
        theseus::celestial_db::translation_cache::KEY_TRANSLATION_SETTINGS,
        contents,
    )
    .await
}

#[tauri::command]
async fn load_translation_cache(
) -> Result<String, String> {
    let state = State::get()
        .await
        .map_err(|e| format!("获取应用状态失败: {}", e))?;
    theseus::celestial_db::translation_cache::load_json(state.celestial.pool())
        .await
        .map_err(|e| format!("读取译文缓存失败: {}", e))
}

#[tauri::command]
async fn save_translation_cache(
    contents: String,
) -> Result<(), String> {
    let state = State::get()
        .await
        .map_err(|e| format!("获取应用状态失败: {}", e))?;
    theseus::celestial_db::translation_cache::replace_json(
        state.celestial.pool(),
        &contents,
    )
    .await
    .map_err(|e| format!("写入译文缓存失败: {}", e))
}

#[tauri::command]
async fn clear_translation_cache(
) -> Result<(), String> {
    let state = State::get()
        .await
        .map_err(|e| format!("获取应用状态失败: {}", e))?;
    theseus::celestial_db::translation_cache::clear(state.celestial.pool())
        .await
        .map_err(|e| format!("清空译文缓存失败: {}", e))
}

// 8. 界面布局状态：分屏开关与界面偏好存 ui_settings 表的两个键
//    （取代 interface/*.json）。和翻译一样只读写整个文档，
//    schema 由前端拥有，所以以后加别的布局状态不用改 Rust。

/// 无记录时返回空串，前端据此回退到默认值（关闭）。
#[tauri::command]
async fn load_split_view_settings(
) -> Result<String, String> {
    Ok(ui_settings_get(theseus::celestial_db::ui_settings::KEY_SPLIT_VIEW)
        .await?
        .unwrap_or_default())
}

#[tauri::command]
async fn save_split_view_settings(
    contents: String,
) -> Result<(), String> {
    ui_settings_set(theseus::celestial_db::ui_settings::KEY_SPLIT_VIEW, contents)
        .await
}

// 界面偏好（原先 interface/ui-preferences.json）
//
// 刻意不放进 app.db 的 settings.feature_flags：那个字段在 Rust 侧是
// HashMap<FeatureFlag, bool>，FeatureFlag 是个严格枚举，出现未知 key 会让整个 map
// 反序列化失败并被 unwrap_or_default() 静默清空——等于把用户所有开关一起丢掉。
// ui_settings 表是纯 KV，没有这个问题；同样只读写整个文档、schema 由前端拥有。
/// 无记录时返回空串，前端据此回退到默认值。
#[tauri::command]
async fn load_ui_preferences(
) -> Result<String, String> {
    Ok(ui_settings_get(theseus::celestial_db::ui_settings::KEY_UI_PREFERENCES)
        .await?
        .unwrap_or_default())
}

#[tauri::command]
async fn save_ui_preferences(
    contents: String,
) -> Result<(), String> {
    ui_settings_set(theseus::celestial_db::ui_settings::KEY_UI_PREFERENCES, contents)
        .await
}

#[tauri::command]
async fn get_background_as_base64() -> Result<String, String> {
    let mut path = settings_dir().await?;

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

// msi静默安装
#[tauri::command]
async fn run_silent_msi(path: String) -> Result<(), String> {
    std::process::Command::new("msiexec")
        .args(["/i", &path, "/qn", "/norestart"])
        .status()
        .map_err(|e| e.to_string())?;
    Ok(())
}

// 拉取 release 元数据（latest release JSON）。放在 Rust 里而非前端 fetch，是为了
// 绕开 webview 的 CORS 限制：自部署的 Gitea / CDN 未必回 Access-Control-Allow-Origin，
// 前端 fetch 会 "Failed to fetch"，而 reqwest 不受 CORS 约束。返回原始 JSON 字符串，
// 由前端解析。
#[tauri::command]
async fn fetch_release_metadata(url: String) -> Result<String, String> {
    let parsed = url::Url::parse(&url)
        .map_err(|error| format!("Invalid release URL: {error}"))?;
    if parsed.scheme() != "https" {
        return Err("Release metadata must be fetched over https".to_string());
    }
    if !update_security::metadata_host_allowed(&parsed) {
        return Err(format!(
            "Refusing to fetch release metadata from an untrusted host: {parsed}"
        ));
    }

    let client = update_security::client(update_security::metadata_host_allowed)?;

    let resp = client
        .get(parsed)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| format!("Read body failed: {}", e))?;

    if !status.is_success() {
        // 把状态码和响应片段一起带回前端，方便打印详细的跳过原因。
        let snippet: String = body.chars().take(200).collect();
        return Err(format!("HTTP {}: {}", status.as_u16(), snippet));
    }

    Ok(body)
}

// 下载安装包，验签通过后写入缓存目录，等用户确认重启再安装
#[tauri::command]
async fn download_and_run_msi(
    asset_url: String,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let parsed = url::Url::parse(&asset_url)
        .map_err(|error| format!("Invalid update URL: {error}"))?;
    if parsed.scheme() != "https" {
        return Err("Update downloads must be https".to_string());
    }
    if !update_security::download_host_allowed(&parsed) {
        return Err(format!(
            "Refusing an update from an untrusted host: {parsed}"
        ));
    }

    let client = update_security::client(update_security::download_host_allowed)?;

    let resp = client
        .get(parsed.clone())
        .send()
        .await
        .map_err(|e| format!("Download failed: {e:?}"))?;

    if !resp.status().is_success() {
        return Err(format!("HTTP error: {}", resp.status()));
    }

    // Stream the body so we can report download progress to the frontend, the
    // way the original Modrinth updater did. `resp.bytes()` would swallow the
    // whole file in one await and leave the progress bar stuck at 0.
    let total = resp.content_length().unwrap_or(0);
    let mut downloaded: u64 = 0;
    let mut bytes: Vec<u8> = Vec::with_capacity(total as usize);
    let mut stream = resp.bytes_stream();
    let mut last_emit = 0.0_f64;

    use futures::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Read failed: {}", e))?;
        bytes.extend_from_slice(&chunk);
        downloaded += chunk.len() as u64;
        if total > 0 {
            let fraction = downloaded as f64 / total as f64;
            // Throttle to whole-percent steps to avoid flooding the event loop.
            if fraction - last_emit >= 0.01 || fraction >= 1.0 {
                last_emit = fraction;
                let _ = app_handle.emit_to("main", "app-update-progress", fraction);
            }
        }
    }

    // The signature sits beside the asset, on the same host and through the
    // same redirect policy. A release without one is not an update anyone can
    // install, so the refusal says why rather than reporting a network error.
    let mut signature_url = parsed;
    let signature_path = format!("{}.sig", signature_url.path());
    signature_url.set_path(&signature_path);
    let signature = client
        .get(signature_url)
        .send()
        .await
        .map_err(|e| format!("Signature download failed: {e:?}"))?;
    if !signature.status().is_success() {
        return Err(format!(
            "This update has no signature ({}), so it cannot be verified",
            signature.status()
        ));
    }
    let signature = signature
        .text()
        .await
        .map_err(|e| format!("Signature read failed: {e}"))?;

    update_security::verify_signature(&bytes, &signature)?;

    let cache_dir = app_handle.path().cache_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&cache_dir).map_err(|e| e.to_string())?;
    let full_path = cache_dir.join(update_security::installer_file_name(&asset_url));
    std::fs::write(&full_path, &bytes).map_err(|e| e.to_string())?;

    // The path stays in this process. Anything that can name a path can name
    // any path, and this one is executed.
    *app_handle
        .state::<update_security::PendingInstaller>()
        .0
        .lock()
        .map_err(|e| e.to_string())? = Some(full_path);

    app_handle.emit_to("main", "app-update-event", "downloaded")
        .map_err(|e| e.to_string())?;

    Ok(())
}

// 运行已验签的安装程序并退出 app
#[tauri::command]
async fn install_cached_msi(app_handle: tauri::AppHandle) -> Result<(), String> {
    // Bound to a variable: `state()` hands back a temporary that the lock guard
    // borrows from, so chaining it straight into the guard drops the temporary
    // too early.
    let installer_state = app_handle.state::<update_security::PendingInstaller>();
    let pending = installer_state
        .0
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    let pending = pending.ok_or_else(|| {
        "No verified update is waiting to be installed".to_string()
    })?;

    // The path came from this process, so this can only fail if something
    // replaced it — but "only if" is not a guarantee to start a program on.
    let cache_dir = app_handle.path().cache_dir().map_err(|e| e.to_string())?;
    let cache_dir = std::fs::canonicalize(&cache_dir).map_err(|e| e.to_string())?;
    let installer = std::fs::canonicalize(&pending).map_err(|e| e.to_string())?;
    if !installer.starts_with(&cache_dir) {
        return Err(
            "Refusing to run an installer from outside the cache directory"
                .to_string(),
        );
    }

    std::process::Command::new(&installer)
        .arg("/S")
        .spawn()
        .map_err(|e| e.to_string())?;
    app_handle.exit(0);
    Ok(())
}

#[tauri::command]
async fn delete_background() -> Result<(), String> {
    let mut path = settings_dir().await?;
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
    #[cfg(feature = "export-app-events")]
    theseus::export_app_event_bindings(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../app-frontend/src/generated/app-events"),
    )
    .expect("failed to export app event TypeScript bindings");

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

    #[cfg(target_os = "macos")]
    {
        builder = builder
            .menu(macos::menu::create)
            .on_menu_event(macos::menu::handle_event);
    }

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
                tracing::info!("Handling command-line deep link");
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
                .with_denylist(&["signin", "source-download"])
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
                        tracing::info!("Handling macOS deep link");

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
                tracing::info!("Handling deep link");
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
        .manage(update_security::PendingInstaller::default())
        .plugin(api::auth::init())
        .plugin(api::mr_auth::init())
        .plugin(api::onboarding_checklist::init())
        .plugin(api::import::init())
        .plugin(api::install::init())
        .plugin(api::instance::init())
        .plugin(api::logs::init())
        .plugin(api::jre::init())
        .plugin(api::metadata::init())
        .plugin(api::minecraft_skins::init())
        .plugin(api::process::init())
        .plugin(api::reports::init())
        .plugin(api::settings::init())
        .plugin(api::shortcuts::init())
        .plugin(api::tags::init())
        .plugin(api::users::init())
        .plugin(api::utils::init())
        .plugin(api::cache::init())
        .plugin(api::files::init())
        .plugin(api::ads::init())
        .plugin(api::friends::init())
        .plugin(api::worlds::init())
        .plugin(api::plugins::init())
        .plugin(api::content_sources::init())
		.plugin(api::announcements::init())
        .invoke_handler(tauri::generate_handler![
            initialize_state,
            is_dev,
            toggle_decorations,
            show_window,
            restart_app,
            save_background_image,
            get_background_as_base64,
            delete_background,
            save_bg_blur_status,
            load_bg_blur_status,
            save_instance_card_icon_bg,
            load_instance_card_icon_bg,
            save_hue_value,
            load_hue_value,
            save_library_sort_directions,
            load_library_sort_directions,
            load_translation_settings,
            save_translation_settings,
            load_translation_cache,
            save_translation_cache,
            clear_translation_cache,
            load_split_view_settings,
            save_split_view_settings,
            load_ui_preferences,
            save_ui_preferences,
            check_for_import,
            import_old_data,
            set_dont_show_import_modal,
            get_import_banner_setting,
            do_import_and_restart,
            run_silent_msi,
            download_and_run_msi,
            fetch_release_metadata,
            install_cached_msi,
        ]);

    tracing::info!("Initializing app...");
    let app = builder.build(tauri_context);

    match app {
        Ok(app) => {
            app.run(|app, event| {
                #[cfg(not(target_os = "macos"))]
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

                // Kill any plugin-spawned native sidecars and stop LAN
                // announcers so nothing outlives the launcher.
                if matches!(&event, tauri::RunEvent::ExitRequested { .. }) {
                    tauri::async_runtime::block_on(async {
                        theseus::plugins::sidecar::shutdown_all().await;
                        theseus::plugins::lan::shutdown_all().await;
                    });
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

						let current_version = &app.package_info().version;
						let mut version_parts = update.version.split('.');
						let major = version_parts.next().and_then(|part| part.parse::<u64>().ok());
						let minor = version_parts.next().and_then(|part| part.parse::<u64>().ok());
						let is_major_update = major.zip(minor).is_some_and(|version| {
							version > (current_version.major, current_version.minor)
						});
						set_changelog_toast(is_major_update.then(|| update.version.clone()));
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
