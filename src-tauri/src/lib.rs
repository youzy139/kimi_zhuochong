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
            clear_custom_image
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
