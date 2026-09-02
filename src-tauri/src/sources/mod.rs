//! 数据源适配层：统一 `UsageSource` trait + auto 降级编排。

pub mod cli_usage;
pub mod ledger;

use crate::config::Config;
use serde::Serialize;
use std::time::{Duration, Instant};

/// 周额度信息
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct WeekInfo {
    pub used_tokens: u64,
    pub total_tokens: Option<u64>,
    pub percent: Option<f64>,
    pub reset_at: Option<i64>,
}

/// 5 小时滚动窗口状态
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct Window5h {
    pub used_tokens: u64,
    pub warn_threshold: Option<u64>,
    /// "ok" | "warn" | "unknown"
    pub status: String,
}

/// 最近一轮对话消耗
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct LastTurn {
    pub tokens: u64,
    pub seq: u64,
}

/// 额度快照（IPC 契约，serde snake_case）
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct UsageSnapshot {
    /// "ledger" | "cli" | "none"
    pub source: String,
    pub week: WeekInfo,
    pub window5h: Window5h,
    /// 加油包余额：无可用数据源，恒为 null
    pub fuel_pack: Option<u64>,
    pub last_turn: Option<LastTurn>,
    pub updated_at: i64,
}

impl UsageSnapshot {
    /// 保底快照：任何路径失败时返回，绝不 panic
    pub fn empty(source: &str) -> Self {
        Self {
            source: source.to_string(),
            window5h: Window5h {
                status: "unknown".to_string(),
                ..Default::default()
            },
            updated_at: now_millis(),
            ..Default::default()
        }
    }

    /// 用 fallback 填充本快照中缺失（None）的字段
    fn merge_missing(&mut self, fallback: &UsageSnapshot) {
        if self.week.total_tokens.is_none() {
            self.week.total_tokens = fallback.week.total_tokens;
        }
        if self.week.percent.is_none() {
            self.week.percent = fallback.week.percent;
        }
        if self.week.reset_at.is_none() {
            self.week.reset_at = fallback.week.reset_at;
        }
        if self.week.used_tokens == 0 {
            self.week.used_tokens = fallback.week.used_tokens;
        }
        if self.window5h.warn_threshold.is_none() {
            self.window5h.warn_threshold = fallback.window5h.warn_threshold;
        }
        if self.window5h.status.is_empty() || self.window5h.status == "unknown" {
            if !fallback.window5h.status.is_empty() {
                self.window5h.status = fallback.window5h.status.clone();
            }
        }
        if self.window5h.used_tokens == 0 {
            self.window5h.used_tokens = fallback.window5h.used_tokens;
        }
        if self.last_turn.is_none() {
            self.last_turn = fallback.last_turn.clone();
        }
    }
}

/// 当前 epoch 毫秒
pub fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 数据源 trait：返回 Err 即触发上层降级，不允许 panic
pub trait UsageSource {
    fn fetch(&mut self, cfg: &Config) -> Result<UsageSnapshot, String>;
}

/// CLI 结果缓存时长
const CLI_CACHE_TTL: Duration = Duration::from_secs(10 * 60);

/// 编排器：按 data_source 配置选择数据源，失败自动降级 ledger
pub struct Orchestrator {
    ledger: ledger::LedgerSource,
    cli: cli_usage::CliUsageSource,
    cli_cache: Option<(Instant, UsageSnapshot)>,
}

impl Default for Orchestrator {
    fn default() -> Self {
        Self::new()
    }
}

impl Orchestrator {
    pub fn new() -> Self {
        Self {
            ledger: ledger::LedgerSource::new(),
            cli: cli_usage::CliUsageSource::new(),
            cli_cache: None,
        }
    }

    /// 取快照。任何情况下都不 panic，最差返回空快照。
    pub fn fetch(&mut self, cfg: &Config) -> UsageSnapshot {
        match cfg.data_source.as_str() {
            // 显式 ledger：直接用
            "ledger" => self.ledger_or_empty(cfg),
            // 显式 cli 或 auto：先试 cli（auto 走 10 分钟缓存），失败降级 ledger
            _ => match self.try_cli(cfg) {
                Ok(mut snap) => {
                    // cli 数据可能不全，用 ledger 补缺
                    let ledger_snap = self.ledger_or_empty(cfg);
                    snap.merge_missing(&ledger_snap);
                    snap
                }
                Err(_) => self.ledger_or_empty(cfg),
            },
        }
    }

    fn ledger_or_empty(&mut self, cfg: &Config) -> UsageSnapshot {
        self.ledger
            .fetch(cfg)
            .unwrap_or_else(|_| UsageSnapshot::empty("ledger"))
    }

    fn try_cli(&mut self, cfg: &Config) -> Result<UsageSnapshot, String> {
        // 命中缓存直接返回
        if let Some((at, snap)) = &self.cli_cache {
            if at.elapsed() < CLI_CACHE_TTL {
                return Ok(snap.clone());
            }
        }
        let snap = self.cli.fetch(cfg)?;
        self.cli_cache = Some((Instant::now(), snap.clone()));
        Ok(snap)
    }
}
