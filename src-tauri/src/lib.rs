//! kimi-rabbit-widget 库：配置、数据源适配层与 Tauri 命令。

pub mod config;
pub mod sources;

use config::Config;
use sources::{Orchestrator, UsageSnapshot};
use std::sync::Mutex;

/// 应用状态：配置 + 数据源编排器（含增量/结果缓存）
pub struct AppState {
    pub cfg: Config,
    pub orchestrator: Orchestrator,
}

impl AppState {
    fn new() -> Self {
        Self {
            cfg: config::load(),
            orchestrator: Orchestrator::new(),
        }
    }
}

/// 确保周锚点存在：首次使用时写入当前时间并落盘
fn ensure_week_anchor(state: &mut AppState) {
    if state.cfg.week_anchor.is_none() {
        state.cfg.week_anchor = Some(sources::now_millis());
        config::save(&state.cfg);
    }
}

#[tauri::command]
fn get_usage(state: tauri::State<'_, Mutex<AppState>>) -> UsageSnapshot {
    let mut guard = match state.lock() {
        Ok(g) => g,
        // 锁中毒也不允许 panic：返回保底快照
        Err(_) => return UsageSnapshot::empty("none"),
    };
    ensure_week_anchor(&mut guard);
    let AppState { cfg, orchestrator } = &mut *guard;
    orchestrator.fetch(cfg)
}

#[tauri::command]
fn get_config(state: tauri::State<'_, Mutex<AppState>>) -> Config {
    match state.lock() {
        Ok(g) => g.cfg.clone(),
        Err(_) => Config::default(),
    }
}

#[tauri::command]
fn set_config(state: tauri::State<'_, Mutex<AppState>>, patch: serde_json::Value) -> Config {
    let mut guard = match state.lock() {
        Ok(g) => g,
        Err(_) => return Config::default(),
    };
    config::apply_patch(&mut guard.cfg, &patch);
    config::save(&guard.cfg);
    guard.cfg.clone()
}

/// 轻量轮询用：只取最近一轮消耗（任务结束音/消耗泡泡的 5 秒轮询）
#[tauri::command]
fn get_last_turn(state: tauri::State<'_, Mutex<AppState>>) -> Option<sources::LastTurn> {
    let mut guard = state.lock().ok()?;
    ensure_week_anchor(&mut guard);
    let AppState { cfg, orchestrator } = &mut *guard;
    orchestrator.fetch(cfg).last_turn
}

/// 用量历史（记录窗口）：按天×模型 + 全时段模型合计
#[tauri::command]
fn get_usage_history(state: tauri::State<'_, Mutex<AppState>>) -> sources::UsageHistory {
    match state.lock() {
        Ok(mut g) => g.orchestrator.history(),
        Err(_) => sources::UsageHistory::default(),
    }
}

/// 自定义角色图：返回已设置的图片路径（存在才返回），前端用 convertFileSrc 显示
#[tauri::command]
fn get_custom_image() -> Option<String> {
    let p = config::custom_image_path();
    if p.is_file() {
        Some(p.to_string_lossy().to_string())
    } else {
        None
    }
}

/// 设置自定义角色图：校验后复制到配置目录（源文件后续被移动/删除也不影响）
#[tauri::command]
fn set_custom_image(path: String) -> Result<String, String> {
    let src = std::path::PathBuf::from(&path);
    let meta = std::fs::metadata(&src).map_err(|_| "文件不存在或不可读".to_string())?;
    if meta.len() > 20 * 1024 * 1024 {
        return Err("图片超过 20MB，换一张小点的".to_string());
    }
    let dest = config::custom_image_path();
    if let Some(dir) = dest.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::copy(&src, &dest).map_err(|e| format!("复制失败：{e}"))?;
    Ok(dest.to_string_lossy().to_string())
}

/// 恢复默认角色图
#[tauri::command]
fn clear_custom_image() {
    let _ = std::fs::remove_file(config::custom_image_path());
}

// ---------- 资源库（角色图 / 音效片段 / 泡泡图，索引存 config.json，文件存库目录） ----------

/// 导入文件到库目录，返回 (id, name, file)
fn import_asset(src_path: &str, dir: &std::path::PathBuf, max_bytes: u64) -> Result<config::AssetEntry, String> {
    let src = std::path::PathBuf::from(src_path);
    let meta = std::fs::metadata(&src).map_err(|_| "文件不存在或不可读".to_string())?;
    if meta.len() > max_bytes {
        return Err(format!("文件超过 {}MB", max_bytes / 1024 / 1024));
    }
    let id = sources::now_millis().to_string();
    let ext = src
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let name = src
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| id.clone());
    let file = if ext.is_empty() { id.clone() } else { format!("{id}.{ext}") };
    let _ = std::fs::create_dir_all(dir);
    std::fs::copy(&src, dir.join(&file)).map_err(|e| format!("复制失败：{e}"))?;
    Ok(config::AssetEntry { id, name, file })
}

#[cfg(test)]
mod tests {
    /// 导入链路核心：文件复制进库目录 + 索引条目字段正确
    #[test]
    fn import_asset_copies_file_and_builds_entry() {
        let base = std::env::temp_dir().join(format!(
            "kimi-rabbit-import-test-{}-{}",
            std::process::id(),
            crate::sources::now_millis()
        ));
        let src_dir = base.join("src");
        let lib_dir = base.join("lib");
        std::fs::create_dir_all(&src_dir).unwrap();
        let src_file = src_dir.join("测试图.png");
        std::fs::write(&src_file, b"fake-png-bytes").unwrap();

        let entry = super::import_asset(src_file.to_str().unwrap(), &lib_dir, 1024).unwrap();
        assert_eq!(entry.name, "测试图");
        assert!(entry.file.ends_with(".png"));
        // 文件真的被复制进库目录（用户反馈「导入是虚假的」的回归保障）
        let copied = std::fs::read(lib_dir.join(&entry.file)).unwrap();
        assert_eq!(copied, b"fake-png-bytes");

        // 不存在的源文件必须报错而不是静默成功
        assert!(super::import_asset("C:/no/such/file.png", &lib_dir, 1024).is_err());
        // 超限必须报错
        assert!(super::import_asset(src_file.to_str().unwrap(), &lib_dir, 2).is_err());

        let _ = std::fs::remove_dir_all(&base);
    }
}

fn delete_asset_file(dir: &std::path::PathBuf, file: &str) {
    let _ = std::fs::remove_file(dir.join(file));
}

#[tauri::command]
fn list_roles(state: tauri::State<'_, Mutex<AppState>>) -> serde_json::Value {
    match state.lock() {
        Ok(g) => serde_json::json!({ "roles": g.cfg.roles, "active": g.cfg.active_role }),
        Err(_) => serde_json::json!({ "roles": [], "active": null }),
    }
}

#[tauri::command]
fn add_role(state: tauri::State<'_, Mutex<AppState>>, path: String) -> Result<config::AssetEntry, String> {
    let entry = import_asset(&path, &config::roles_dir(), 20 * 1024 * 1024)?;
    let mut g = state.lock().map_err(|_| "状态锁失败")?;
    g.cfg.roles.push(entry.clone());
    config::save(&g.cfg);
    Ok(entry)
}

#[tauri::command]
fn select_role(state: tauri::State<'_, Mutex<AppState>>, id: String) -> Result<String, String> {
    let mut g = state.lock().map_err(|_| "状态锁失败")?;
    let entry = g.cfg.roles.iter().find(|r| r.id == id).cloned()
        .ok_or_else(|| "角色图不存在".to_string())?;
    g.cfg.active_role = Some(id);
    config::save(&g.cfg);
    Ok(config::roles_dir().join(&entry.file).to_string_lossy().to_string())
}

#[tauri::command]
fn clear_role(state: tauri::State<'_, Mutex<AppState>>) {
    if let Ok(mut g) = state.lock() {
        g.cfg.active_role = None;
        config::save(&g.cfg);
    }
}

#[tauri::command]
fn delete_role(state: tauri::State<'_, Mutex<AppState>>, id: String) {
    if let Ok(mut g) = state.lock() {
        if let Some(pos) = g.cfg.roles.iter().position(|r| r.id == id) {
            let entry = g.cfg.roles.remove(pos);
            delete_asset_file(&config::roles_dir(), &entry.file);
        }
        if g.cfg.active_role.as_deref() == Some(id.as_str()) {
            g.cfg.active_role = None;
        }
        config::save(&g.cfg);
    }
}

#[tauri::command]
fn list_audio(state: tauri::State<'_, Mutex<AppState>>) -> Vec<config::AssetEntry> {
    match state.lock() {
        Ok(g) => g.cfg.audio_fragments.clone(),
        Err(_) => Vec::new(),
    }
}

#[tauri::command]
fn import_audio(state: tauri::State<'_, Mutex<AppState>>, path: String) -> Result<config::AssetEntry, String> {
    let entry = import_asset(&path, &config::audio_dir(), 10 * 1024 * 1024)?;
    let mut g = state.lock().map_err(|_| "状态锁失败")?;
    g.cfg.audio_fragments.push(entry.clone());
    config::save(&g.cfg);
    Ok(entry)
}

#[tauri::command]
fn delete_audio(state: tauri::State<'_, Mutex<AppState>>, id: String) {
    if let Ok(mut g) = state.lock() {
        if let Some(pos) = g.cfg.audio_fragments.iter().position(|a| a.id == id) {
            let entry = g.cfg.audio_fragments.remove(pos);
            delete_asset_file(&config::audio_dir(), &entry.file);
        }
        // 被引用的槽位一并清空
        if g.cfg.custom_press.as_deref() == Some(id.as_str()) {
            g.cfg.custom_press = None;
        }
        if g.cfg.custom_release.as_deref() == Some(id.as_str()) {
            g.cfg.custom_release = None;
        }
        config::save(&g.cfg);
    }
}

/// 音效片段完整路径（前端 convertFileSrc 播放用）
#[tauri::command]
fn audio_path(state: tauri::State<'_, Mutex<AppState>>, id: String) -> Option<String> {
    let g = state.lock().ok()?;
    let entry = g.cfg.audio_fragments.iter().find(|a| a.id == id)?;
    Some(config::audio_dir().join(&entry.file).to_string_lossy().to_string())
}

#[tauri::command]
fn list_bubble_imgs(state: tauri::State<'_, Mutex<AppState>>) -> Vec<config::AssetEntry> {
    match state.lock() {
        Ok(g) => g.cfg.bubble_images.clone(),
        Err(_) => Vec::new(),
    }
}

#[tauri::command]
fn import_bubble_img(state: tauri::State<'_, Mutex<AppState>>, path: String) -> Result<config::AssetEntry, String> {
    let entry = import_asset(&path, &config::bubble_imgs_dir(), 20 * 1024 * 1024)?;
    let mut g = state.lock().map_err(|_| "状态锁失败")?;
    g.cfg.bubble_images.push(entry.clone());
    config::save(&g.cfg);
    Ok(entry)
}

#[tauri::command]
fn delete_bubble_img(state: tauri::State<'_, Mutex<AppState>>, id: String) {
    if let Ok(mut g) = state.lock() {
        if let Some(pos) = g.cfg.bubble_images.iter().position(|a| a.id == id) {
            let entry = g.cfg.bubble_images.remove(pos);
            delete_asset_file(&config::bubble_imgs_dir(), &entry.file);
        }
        config::save(&g.cfg);
    }
}

/// 泡泡图完整路径
#[tauri::command]
fn bubble_img_path(state: tauri::State<'_, Mutex<AppState>>, id: String) -> Option<String> {
    let g = state.lock().ok()?;
    let entry = g.cfg.bubble_images.iter().find(|a| a.id == id)?;
    Some(config::bubble_imgs_dir().join(&entry.file).to_string_lossy().to_string())
}

// ---------- 视频库（开心笑/读书/小憩三个槽位可换） ----------

#[tauri::command]
fn list_videos(state: tauri::State<'_, Mutex<AppState>>) -> serde_json::Value {
    match state.lock() {
        Ok(g) => serde_json::json!({
            "videos": g.cfg.videos,
            "slots": {
                "happy": g.cfg.video_happy,
                "reading": g.cfg.video_reading,
                "nap": g.cfg.video_nap,
            }
        }),
        Err(_) => serde_json::json!({ "videos": [], "slots": {} }),
    }
}

#[tauri::command]
fn import_video(state: tauri::State<'_, Mutex<AppState>>, path: String) -> Result<config::AssetEntry, String> {
    let entry = import_asset(&path, &config::videos_dir(), 50 * 1024 * 1024)?;
    let mut g = state.lock().map_err(|_| "状态锁失败")?;
    g.cfg.videos.push(entry.clone());
    config::save(&g.cfg);
    Ok(entry)
}

/// 设置槽位视频：slot ∈ happy|reading|nap，id 为 null 时恢复该槽位默认视频
#[tauri::command]
fn set_video_slot(state: tauri::State<'_, Mutex<AppState>>, slot: String, id: Option<String>) -> Result<(), String> {
    let mut g = state.lock().map_err(|_| "状态锁失败")?;
    if let Some(ref vid) = id {
        if !g.cfg.videos.iter().any(|v| &v.id == vid) {
            return Err("视频不存在".to_string());
        }
    }
    match slot.as_str() {
        "happy" => g.cfg.video_happy = id,
        "reading" => g.cfg.video_reading = id,
        "nap" => g.cfg.video_nap = id,
        _ => return Err("未知槽位".to_string()),
    }
    config::save(&g.cfg);
    Ok(())
}

#[tauri::command]
fn delete_video(state: tauri::State<'_, Mutex<AppState>>, id: String) {
    if let Ok(mut g) = state.lock() {
        if let Some(pos) = g.cfg.videos.iter().position(|v| v.id == id) {
            let entry = g.cfg.videos.remove(pos);
            delete_asset_file(&config::videos_dir(), &entry.file);
        }
        // 引用该视频的槽位一并回退默认
        if g.cfg.video_happy.as_deref() == Some(id.as_str()) {
            g.cfg.video_happy = None;
        }
        if g.cfg.video_reading.as_deref() == Some(id.as_str()) {
            g.cfg.video_reading = None;
        }
        if g.cfg.video_nap.as_deref() == Some(id.as_str()) {
            g.cfg.video_nap = None;
        }
        config::save(&g.cfg);
    }
}

/// 视频完整路径（前端 convertFileSrc 播放/试用）
#[tauri::command]
fn video_path(state: tauri::State<'_, Mutex<AppState>>, id: String) -> Option<String> {
    let g = state.lock().ok()?;
    let entry = g.cfg.videos.iter().find(|v| v.id == id)?;
    Some(config::videos_dir().join(&entry.file).to_string_lossy().to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(Mutex::new(AppState::new()))
        .invoke_handler(tauri::generate_handler![
            get_usage,
            get_config,
            set_config,
            get_last_turn,
            get_usage_history,
            get_custom_image,
            set_custom_image,
            clear_custom_image,
            list_roles,
            add_role,
            select_role,
            clear_role,
            delete_role,
            list_audio,
            import_audio,
            delete_audio,
            audio_path,
            list_bubble_imgs,
            import_bubble_img,
            delete_bubble_img,
            bubble_img_path,
            list_videos,
            import_video,
            set_video_slot,
            delete_video,
            video_path
        ])
        .setup(|app| {
            // 系统托盘：无边框+跳过任务栏的窗口没有别的关闭入口，托盘是唯一的退出通道
            use tauri::menu::{MenuBuilder, MenuItemBuilder};
            use tauri::tray::TrayIconBuilder;
            let quit = MenuItemBuilder::with_id("quit", "退出月兔娘").build(app)?;
            let menu = MenuBuilder::new(app).item(&quit).build()?;
            let mut tray = TrayIconBuilder::with_id("main-tray").menu(&menu).tooltip("月兔娘 · Kimi 额度");
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.on_menu_event(|app, event| {
                if event.id().as_ref() == "quit" {
                    app.exit(0);
                }
            })
            .build(app)?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
