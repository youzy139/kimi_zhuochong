//! 配置读写：存于 `~/.kimi-rabbit-widget/config.json`，不落任何敏感信息。

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// 窗口位置记忆
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Pos {
    pub x: i32,
    pub y: i32,
}

/// 挂件配置，JSON 字段一律 snake_case（前端契约，一字不能差）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case", default)]
pub struct Config {
    pub scale: f64,
    pub volume: f64,
    pub sound_on: bool,
    /// "duck"（小黄鸭 Ya1/Ya2）| "fx1"（音效1 D1/D2）
    pub sound_set: String,
    pub bubble_on: bool,
    pub turn_cost_on: bool,
    /// 每轮消耗泡泡自动关闭毫秒数，0 = 不自动关闭
    pub turn_cost_close_ms: u64,
    /// "auto" | "ledger" | "cli"
    pub data_source: String,
    pub weekly_quota_tokens: Option<u64>,
    pub window5h_warn_tokens: Option<u64>,
    pub pos: Option<Pos>,
    /// 内部字段：周计费周期锚点（epoch ms）。首次使用时自动写入当前时间。
    pub week_anchor: Option<i64>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            scale: 1.5,
            volume: 0.9,
            sound_on: true,
            sound_set: "duck".to_string(),
            bubble_on: true,
            turn_cost_on: true,
            turn_cost_close_ms: 5000,
            data_source: "auto".to_string(),
            weekly_quota_tokens: None,
            window5h_warn_tokens: None,
            pos: None,
            week_anchor: None,
        }
    }
}

/// 用户 home 目录：Windows 优先 USERPROFILE，其次 HOME
pub fn home_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("USERPROFILE") {
        return PathBuf::from(p);
    }
    if let Some(p) = std::env::var_os("HOME") {
        return PathBuf::from(p);
    }
    PathBuf::from(".")
}

/// kimi-code 数据目录：KIMI_CODE_HOME 优先，否则 ~/.kimi-code
pub fn kimi_code_home() -> PathBuf {
    if let Some(p) = std::env::var_os("KIMI_CODE_HOME") {
        return PathBuf::from(p);
    }
    home_dir().join(".kimi-code")
}

/// 配置文件路径
pub fn config_path() -> PathBuf {
    home_dir().join(".kimi-rabbit-widget").join("config.json")
}

/// 读取配置；文件缺失或损坏时回退默认值
pub fn load() -> Config {
    let path = config_path();
    let Ok(text) = fs::read_to_string(&path) else {
        return Config::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// 写回配置（尽力而为，失败不 panic）
pub fn save(cfg: &Config) {
    let path = config_path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string_pretty(cfg) {
        let _ = fs::write(&path, text);
    }
}

/// 部分字段合并：把 patch 里出现的键覆盖到当前配置上，再整体反序列化
pub fn apply_patch(cfg: &mut Config, patch: &serde_json::Value) {
    let Some(obj) = patch.as_object() else {
        return;
    };
    let Ok(mut cur) = serde_json::to_value(&*cfg) else {
        return;
    };
    if let Some(cur_obj) = cur.as_object_mut() {
        for (k, v) in obj {
            cur_obj.insert(k.clone(), v.clone());
        }
    }
    if let Ok(merged) = serde_json::from_value::<Config>(cur) {
        *cfg = merged;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_values() {
        let c = Config::default();
        assert_eq!(c.scale, 1.5);
        assert_eq!(c.volume, 0.9);
        assert!(c.sound_on);
        assert_eq!(c.data_source, "auto");
        assert!(c.weekly_quota_tokens.is_none());
    }

    #[test]
    fn patch_merges_partial_fields() {
        let mut c = Config::default();
        apply_patch(&mut c, &serde_json::json!({"scale": 2.0, "data_source": "ledger"}));
        assert_eq!(c.scale, 2.0);
        assert_eq!(c.data_source, "ledger");
        // 未提及的字段保持默认
        assert_eq!(c.volume, 0.9);
        assert!(c.bubble_on);
    }

    #[test]
    fn patch_can_set_and_clear_nullable() {
        let mut c = Config::default();
        apply_patch(&mut c, &serde_json::json!({"weekly_quota_tokens": 100000}));
        assert_eq!(c.weekly_quota_tokens, Some(100000));
        apply_patch(&mut c, &serde_json::json!({"weekly_quota_tokens": null}));
        assert_eq!(c.weekly_quota_tokens, None);
    }

    #[test]
    fn patch_ignores_garbage() {
        let mut c = Config::default();
        apply_patch(&mut c, &serde_json::json!(42));
        assert_eq!(c, Config::default());
        apply_patch(&mut c, &serde_json::json!({"unknown_key": 1}));
        assert_eq!(c, Config::default());
    }

    #[test]
    fn json_field_names_are_snake_case() {
        let c = Config::default();
        let v = serde_json::to_value(&c).unwrap();
        let obj = v.as_object().unwrap();
        for key in [
            "scale", "volume", "sound_on", "sound_set", "bubble_on", "turn_cost_on",
            "turn_cost_close_ms", "data_source", "weekly_quota_tokens", "window5h_warn_tokens", "pos",
        ] {
            assert!(obj.contains_key(key), "缺少字段 {key}");
        }
    }
}
