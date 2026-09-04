#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]
#![recursion_limit = "256"]

use native_dialog::{DialogBuilder, MessageLevel};
use std::{env, io};
use std::sync::atomic::Ordering;
use tauri::{Emitter, Listener, Manager};
use tauri_plugin_fs::FsExt;
use theseus::prelude::*;
use std::fs;
use std::path::PathBuf;
use base64::{Engine as _, engine::general_purpose};
use tauri_plugin_http::reqwest;

mod api;
mod error;

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

// 5. 保存 Library 每种排序方式的正序/倒序状态
#[tauri::command]
async fn save_library_sort_directions(
    app_handle: tauri::AppHandle,
    directions: std::collections::HashMap<String, String>,
) -> Result<(), String> {
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

    // 读取现有配置，仅覆盖 library_sort_directions 字段，保留其他设置
    let existing = if config_path.exists() {
        let content = fs::read_to_string(&config_path).unwrap_or_else(|_| "{}".to_string());
        serde_json::from_str::<serde_json::Value>(&content).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    let mut updated = existing.clone();
    updated["library_sort_directions"] = serde_json::json!(directions);

    fs::write(&config_path, serde_json::to_string_pretty(&updated).map_err(|e| format!("序列化失败: {}", e))?)
        .map_err(|e| format!("写入配置文件失败: {}", e))?;

    Ok(())
}

// 6. 读取 Library 每种排序方式的正序/倒序状态
#[tauri::command]
async fn load_library_sort_directions(
    app_handle: tauri::AppHandle,
) -> Result<std::collections::HashMap<String, String>, String> {
    let mut config_path = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法获取 App Data 目录: {}", e))?;

    config_path.push("custom_backgrounds");
    config_path.push("celestial_settings.json");

    // 无配置文件时返回空表，前端会退回到每种排序方式的默认方向
    if !config_path.exists() {
        return Ok(std::collections::HashMap::new());
    }

    let content = fs::read_to_string(&config_path)
        .map_err(|e| format!("读取配置文件失败: {}", e))?;

    let json: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("JSON解析失败: {}", e))?;

    let mut directions = std::collections::HashMap::new();
    if let Some(map) = json.get("library_sort_directions").and_then(|v| v.as_object()) {
        for (key, value) in map {
            if let Some(direction) = value.as_str() {
                directions.insert(key.clone(), direction.to_string());
            }
        }
    }

    Ok(directions)
}

// 7. 翻译功能的独立存储目录：<appdata>/translation/
//    settings.json 存翻译服务选择，cache.json 存译文缓存。
//    这里只负责读写整个文件，schema 和 7 天过期清理都由前端负责，
//    这样以后加字段不用改 Rust。
fn translation_dir(
    app_handle: &tauri::AppHandle,
) -> Result<std::path::PathBuf, String> {
    let mut dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法获取 App Data 目录: {}", e))?;

    dir.push("translation");
    if !dir.exists() {
        fs::create_dir_all(&dir)
            .map_err(|e| format!("创建翻译目录失败: {}", e))?;
    }

    Ok(dir)
}

/// 文件不存在时返回空串，前端据此回退到默认值。
fn read_translation_file(
    app_handle: &tauri::AppHandle,
    name: &str,
) -> Result<String, String> {
    let path = translation_dir(app_handle)?.join(name);
    if !path.exists() {
        return Ok(String::new());
    }

    fs::read_to_string(&path)
        .map_err(|e| format!("读取 {} 失败: {}", name, e))
}

/// 先写 .tmp 再改名：写入中途退出也不会留下被截断的坏 JSON。
fn write_translation_file(
    app_handle: &tauri::AppHandle,
    name: &str,
    contents: &str,
) -> Result<(), String> {
    let dir = translation_dir(app_handle)?;
    let tmp = dir.join(format!("{}.tmp", name));

    fs::write(&tmp, contents)
        .map_err(|e| format!("写入 {} 失败: {}", name, e))?;
    fs::rename(&tmp, dir.join(name))
        .map_err(|e| format!("替换 {} 失败: {}", name, e))
}

#[tauri::command]
async fn load_translation_settings(
    app_handle: tauri::AppHandle,
) -> Result<String, String> {
    read_translation_file(&app_handle, "settings.json")
}

#[tauri::command]
async fn save_translation_settings(
    app_handle: tauri::AppHandle,
    contents: String,
) -> Result<(), String> {
    write_translation_file(&app_handle, "settings.json", &contents)
}

#[tauri::command]
async fn load_translation_cache(
    app_handle: tauri::AppHandle,
) -> Result<String, String> {
    read_translation_file(&app_handle, "cache.json")
}

#[tauri::command]
async fn save_translation_cache(
    app_handle: tauri::AppHandle,
    contents: String,
) -> Result<(), String> {
    write_translation_file(&app_handle, "cache.json", &contents)
}

#[tauri::command]
async fn clear_translation_cache(
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let path = translation_dir(&app_handle)?.join("cache.json");
    if path.exists() {
        fs::remove_file(&path)
            .map_err(|e| format!("删除翻译缓存失败: {}", e))?;
    }

    Ok(())
}

// 8. 界面布局状态的独立存储目录：<appdata>/interface/
//    split-view.json 存 discover 分屏开关。和翻译一样只读写整个文件，
//    schema 由前端拥有，所以以后加别的布局状态不用改 Rust。
fn interface_dir(
    app_handle: &tauri::AppHandle,
) -> Result<std::path::PathBuf, String> {
    let mut dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法获取 App Data 目录: {}", e))?;

    dir.push("interface");
    if !dir.exists() {
        fs::create_dir_all(&dir)
            .map_err(|e| format!("创建界面配置目录失败: {}", e))?;
    }

    Ok(dir)
}

/// 文件不存在时返回空串，前端据此回退到默认值（关闭）。
#[tauri::command]
async fn load_split_view_settings(
    app_handle: tauri::AppHandle,
) -> Result<String, String> {
    let path = interface_dir(&app_handle)?.join("split-view.json");
    if !path.exists() {
        return Ok(String::new());
    }

    fs::read_to_string(&path)
        .map_err(|e| format!("读取分屏配置失败: {}", e))
}

/// 先写 .tmp 再改名：写入中途退出也不会留下被截断的坏 JSON。
#[tauri::command]
async fn save_split_view_settings(
    app_handle: tauri::AppHandle,
    contents: String,
) -> Result<(), String> {
    let dir = interface_dir(&app_handle)?;
    let tmp = dir.join("split-view.json.tmp");

    fs::write(&tmp, contents)
        .map_err(|e| format!("写入分屏配置失败: {}", e))?;
    fs::rename(&tmp, dir.join("split-view.json"))
        .map_err(|e| format!("替换分屏配置失败: {}", e))
}

// 界面偏好：<appdata>/interface/ui-preferences.json
//
// 刻意不放进 app.db 的 settings.feature_flags：那个字段在 Rust 侧是
// HashMap<FeatureFlag, bool>，FeatureFlag 是个严格枚举，出现未知 key 会让整个 map
// 反序列化失败并被 unwrap_or_default() 静默清空——等于把用户所有开关一起丢掉。
// 和 split-view.json 一样只读写整个文件、schema 由前端拥有，所以以后再加界面偏好
// 不需要动 Rust，也永远不会牵动数据库。
/// 文件不存在时返回空串，前端据此回退到默认值。
#[tauri::command]
async fn load_ui_preferences(
    app_handle: tauri::AppHandle,
) -> Result<String, String> {
    let path = interface_dir(&app_handle)?.join("ui-preferences.json");
    if !path.exists() {
        return Ok(String::new());
    }

    fs::read_to_string(&path)
        .map_err(|e| format!("读取界面偏好失败: {}", e))
}

/// 先写 .tmp 再改名，理由同上。
#[tauri::command]
async fn save_ui_preferences(
    app_handle: tauri::AppHandle,
    contents: String,
) -> Result<(), String> {
    let dir = interface_dir(&app_handle)?;
    let tmp = dir.join("ui-preferences.json.tmp");

    fs::write(&tmp, contents)
        .map_err(|e| format!("写入界面偏好失败: {}", e))?;
    fs::rename(&tmp, dir.join("ui-preferences.json"))
        .map_err(|e| format!("替换界面偏好失败: {}", e))
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

// msi静默安装
#[tauri::command]
async fn run_silent_msi(path: String) -> Result<(), String> {
    std::process::Command::new("msiexec")
        .args(["/i", &path, "/qn", "/norestart"])
        .status()
        .map_err(|e| e.to_string())?;
    Ok(())
}

// 下载 NSIS 安装包到缓存目录，返回文件名
#[tauri::command]
async fn download_and_run_msi(
    asset_url: String,
    app_handle: tauri::AppHandle,
) -> Result<String, String> {
    // git.gay (Forgejo) sits behind a WAF that 403s requests with no / a
    // non-browser User-Agent. `reqwest::get` sends none, so the asset download
    // was rejected even though the release-metadata fetch (from the webview,
    // which does send a UA) succeeded. Send a browser-like UA explicitly.
    let client = reqwest::Client::builder()
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
             (KHTML, like Gecko) CelestialLauncher/updater",
        )
        .build()
        .map_err(|e| format!("Client build failed: {}", e))?;

    let resp = client
        .get(&asset_url)
        .send()
        .await
        .map_err(|e| format!("Download failed: {}", e))?;

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

    let filename = asset_url
        .split('/')
        .rev()
        .nth(0)
        .unwrap_or("update.exe");

    let cache_dir = app_handle.path().cache_dir().map_err(|e| e.to_string())?;
    let full_path = cache_dir.join(filename);
    std::fs::write(&full_path, &bytes).map_err(|e| e.to_string())?;

    app_handle.emit_to("main", "app-update-event", "downloaded")
        .map_err(|e| e.to_string())?;

    Ok(filename.to_string())
}

// 运行缓存中的 NSIS 安装程序并退出 app
#[tauri::command]
async fn install_cached_msi(
    filename: String,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let cache_dir = app_handle.path().cache_dir().map_err(|e| e.to_string())?;
    let full_path = cache_dir.join(&filename);
    if !full_path.exists() {
        return Err(format!("Installer not found: {}", filename));
    }
    std::process::Command::new(&full_path)
        .arg("/S")
        .spawn()
        .map_err(|e| e.to_string())?;
    app_handle.exit(0);
    Ok(())
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
                .with_denylist(&["signin"])
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
