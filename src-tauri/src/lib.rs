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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(Mutex::new(AppState::new()))
        .invoke_handler(tauri::generate_handler![get_usage, get_config, set_config])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
