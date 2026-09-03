//! 本地记账数据源（默认主线）：扫描 kimi-code 会话目录下的 wire.jsonl，
//! 聚合 usage.record 行得到本周用量 / 5 小时窗口 / 最近一轮消耗。
//!
//! 目录结构：`~/.kimi-code/sessions/<wd_*>/session_<uuid>/agents/<*>/wire.jsonl`
//! （KIMI_CODE_HOME 环境变量可替代 ~/.kimi-code）
//!
//! 性能：内存增量缓存，记录每个文件的 (已读偏移, mtime)，mtime 没变就跳过，
//! 变了从偏移续读；文件被截断（长度 < 偏移）则从头重读。

use super::{LastTurn, UsageSnapshot, UsageSource, WeekInfo, Window5h};
use crate::config::{kimi_code_home, Config};
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::time::SystemTime;

const WEEK_MS: i64 = 7 * 24 * 3600 * 1000;
const FIVE_H_MS: i64 = 5 * 3600 * 1000;

/// 单文件增量读取状态
#[derive(Debug, Clone)]
struct FileState {
    offset: u64,
    mtime: Option<SystemTime>,
}

/// 一条用量记录：(epoch 毫秒, 总 token)
type Record = (i64, u64);

pub struct LedgerSource {
    files: HashMap<PathBuf, FileState>,
    records: Vec<Record>,
}

impl LedgerSource {
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
            records: Vec::new(),
        }
    }

    /// 扫描 sessions 目录，增量读取所有 wire.jsonl 的新增内容
    fn scan(&mut self, sessions_root: &PathBuf) {
        let Ok(wds) = fs::read_dir(sessions_root) else {
            return; // 目录不存在：没有任何记录，不视为错误
        };
        for wd in wds.flatten() {
            let wd_path = wd.path();
            if !wd_path.is_dir() {
                continue;
            }
            let Ok(sessions) = fs::read_dir(&wd_path) else {
                continue;
            };
            for sess in sessions.flatten() {
                let agents = sess.path().join("agents");
                if !agents.is_dir() {
                    continue;
                }
                let Ok(agent_dirs) = fs::read_dir(&agents) else {
                    continue;
                };
                for agent in agent_dirs.flatten() {
                    let wire = agent.path().join("wire.jsonl");
                    if wire.is_file() {
                        self.read_incremental(&wire);
                    }
                }
            }
        }
    }

    /// 按 (mtime, offset) 增量读取单个文件
    fn read_incremental(&mut self, path: &PathBuf) {
        let Ok(meta) = fs::metadata(path) else {
            return;
        };
        let mtime = meta.modified().ok();
        let len = meta.len();

        let prev = self.files.get(path).cloned();
        if let Some(st) = &prev {
            // mtime 未变：跳过
            if st.mtime == mtime && st.offset <= len {
                return;
            }
        }
        // 文件被截断则从头读，否则续读
        let mut offset = match &prev {
            Some(st) if st.offset <= len => st.offset,
            _ => 0,
        };

        let Ok(mut file) = fs::File::open(path) else {
            return;
        };
        if file.seek(SeekFrom::Start(offset)).is_err() {
            return;
        }
        let mut buf = Vec::new();
        if file.read_to_end(&mut buf).is_err() {
            return;
        }
        let text = String::from_utf8_lossy(&buf);

        // 只解析完整行；末尾半截行留待下次续读
        for line in text.split_inclusive('\n') {
            if !line.ends_with('\n') {
                break;
            }
            offset += line.len() as u64;
            if let Some(rec) = parse_usage_line(line) {
                self.records.push(rec);
            }
        }
        // 文件没有换行结尾但恰好完整（极少见），尝试解析剩余部分
        if !text.ends_with('\n') {
            if let Some(last) = text.rsplit('\n').next() {
                if !last.is_empty() && last.contains("\"usage.record\"") {
                    if let Some(rec) = parse_usage_line(last) {
                        self.records.push(rec);
                        offset += last.len() as u64;
                    }
                }
            }
        }

        self.files.insert(path.clone(), FileState { offset, mtime });
    }

    /// 聚合当前记录为快照
    fn aggregate(&self, cfg: &Config, now: i64) -> UsageSnapshot {
        // ---- 周周期：anchor + 7 天 × ceil ----
        let anchor = cfg.week_anchor.unwrap_or(now);
        // 最小 k 使 anchor + k*7d > now
        let k = ((now - anchor).max(0) as f64 / WEEK_MS as f64).ceil() as i64;
        let reset_at = anchor + k.max(1) * WEEK_MS;
        let period_start = reset_at - WEEK_MS;

        let week_used: u64 = self
            .records
            .iter()
            .filter(|(t, _)| *t >= period_start && *t < reset_at)
            .map(|(_, n)| *n)
            .sum();
        let percent = cfg
            .weekly_quota_tokens
            .filter(|q| *q > 0)
            .map(|q| week_used as f64 / q as f64 * 100.0);

        // ---- 5 小时滚动窗口 ----
        let win_start = now - FIVE_H_MS;
        let win_used: u64 = self
            .records
            .iter()
            .filter(|(t, _)| *t >= win_start && *t <= now)
            .map(|(_, n)| *n)
            .sum();
        let status = match cfg.window5h_warn_tokens {
            Some(th) if win_used >= th => "warn",
            Some(_) => "ok",
            None => "unknown",
        };

        // ---- 最近一轮 ----
        let last_turn = self
            .records
            .iter()
            .max_by_key(|(t, _)| *t)
            .map(|(_, n)| LastTurn {
                tokens: *n,
                seq: self.records.len() as u64,
            });

        UsageSnapshot {
            source: "ledger".to_string(),
            week: WeekInfo {
                used_tokens: week_used,
                total_tokens: cfg.weekly_quota_tokens,
                percent,
                reset_at: Some(reset_at),
            },
            window5h: Window5h {
                used_tokens: win_used,
                warn_threshold: cfg.window5h_warn_tokens,
                status: status.to_string(),
            },
            fuel_pack: None,
            last_turn,
            updated_at: now,
        }
    }
}

impl Default for LedgerSource {
    fn default() -> Self {
        Self::new()
    }
}

impl UsageSource for LedgerSource {
    fn fetch(&mut self, cfg: &Config) -> Result<UsageSnapshot, String> {
        let root = kimi_code_home().join("sessions");
        self.scan(&root);
        Ok(self.aggregate(cfg, super::now_millis()))
    }
}

/// 解析一行 wire.jsonl；非 usage.record 或坏行返回 None
fn parse_usage_line(line: &str) -> Option<Record> {
    // 快速子串过滤，避免对每行做完整 JSON 解析
    if !line.contains("\"usage.record\"") {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    if v.get("type")?.as_str()? != "usage.record" {
        return None;
    }
    // 只收 turn 粒度记录：session 粒度是累计快照（实测 inputOther 可到 18 万+），
    // 计入会让周用量/5h 窗口虚高，last_turn 也会被污染成「累计消耗」
    if v.get("usageScope").and_then(|s| s.as_str()) != Some("turn") {
        return None;
    }
    let usage = v.get("usage")?;
    let get = |key: &str| usage.get(key).and_then(|x| x.as_u64()).unwrap_or(0);
    let total =
        get("inputOther") + get("output") + get("inputCacheRead") + get("inputCacheCreation");
    let time = v.get("time")?.as_i64()?;
    Some((time, total))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// 在临时目录造 sessions 结构，返回 (sessions_root, 记录时间基准)
    fn make_sessions_tree() -> (PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!(
            "kimi-rabbit-ledger-test-{}-{}",
            std::process::id(),
            super::super::now_millis()
        ));
        let agent_dir = base
            .join("sessions")
            .join("wd_test_001")
            .join("session_abc")
            .join("agents")
            .join("main");
        fs::create_dir_all(&agent_dir).unwrap();
        (base, agent_dir.join("wire.jsonl"))
    }

    fn usage_line(time: i64, total_parts: (u64, u64, u64, u64)) -> String {
        format!(
            "{{\"type\":\"usage.record\",\"model\":\"kimi-code/k3\",\"usage\":{{\"inputOther\":{},\"output\":{},\"inputCacheRead\":{},\"inputCacheCreation\":{}}},\"usageScope\":\"turn\",\"time\":{}}}\n",
            total_parts.0, total_parts.1, total_parts.2, total_parts.3, time
        )
    }

    #[test]
    fn parse_line_ok_and_skip_garbage() {
        let line = usage_line(1784685328447, (2183, 124, 19200, 0));
        let (t, n) = parse_usage_line(&line).unwrap();
        assert_eq!(t, 1784685328447);
        assert_eq!(n, 2183 + 124 + 19200);

        assert!(parse_usage_line("{\"type\":\"other\"}\n").is_none());
        assert!(parse_usage_line("not json at all\n").is_none());
        // 含关键字但 JSON 损坏
        assert!(parse_usage_line("{\"type\":\"usage.record\", broken\n").is_none());
    }

    #[test]
    fn session_scope_records_are_ignored() {
        // session 粒度是累计快照（真实样本），不能计入聚合或 last_turn
        let session_line = "{\"type\":\"usage.record\",\"model\":\"kimi-code/k3\",\"usage\":{\"inputOther\":188907,\"output\":1500,\"inputCacheRead\":19200,\"inputCacheCreation\":0},\"usageScope\":\"session\",\"time\":1787294558678}\n";
        assert!(parse_usage_line(session_line).is_none());
        // 缺 usageScope 字段的也不收（口径未知，宁可漏记不可虚高）
        let no_scope = "{\"type\":\"usage.record\",\"usage\":{\"inputOther\":1,\"output\":2,\"inputCacheRead\":3,\"inputCacheCreation\":0},\"time\":1787294558678}\n";
        assert!(parse_usage_line(no_scope).is_none());
    }

    #[test]
    fn aggregate_week_and_window() {
        let now = super::super::now_millis();
        let mut src = LedgerSource::new();
        // 本周内两条
        src.records.push((now - 3600_000, 1000));
        src.records.push((now - 60_000, 500));
        // 8 天前：上个周期，不计入本周
        src.records.push((now - 8 * 24 * 3600_000, 9999));

        let cfg = Config {
            week_anchor: Some(now - 24 * 3600_000), // 锚在昨天，本周内
            weekly_quota_tokens: Some(10000),
            window5h_warn_tokens: Some(1200),
            ..Default::default()
        };
        let snap = src.aggregate(&cfg, now);
        assert_eq!(snap.week.used_tokens, 1500);
        assert_eq!(snap.week.total_tokens, Some(10000));
        assert!((snap.week.percent.unwrap() - 15.0).abs() < 1e-9);
        assert!(snap.week.reset_at.unwrap() > now);
        // 5h 窗口内也是 1500，>= 阈值 → warn
        assert_eq!(snap.window5h.used_tokens, 1500);
        assert_eq!(snap.window5h.status, "warn");
        // last_turn 取时间最新的一条
        let lt = snap.last_turn.unwrap();
        assert_eq!(lt.tokens, 500);
        assert_eq!(lt.seq, 3);
    }

    #[test]
    fn aggregate_without_quota_gives_null_percent() {
        let now = super::super::now_millis();
        let mut src = LedgerSource::new();
        src.records.push((now - 1000, 100));
        let cfg = Config {
            week_anchor: Some(now),
            ..Default::default()
        };
        let snap = src.aggregate(&cfg, now);
        assert!(snap.week.percent.is_none());
        assert!(snap.week.total_tokens.is_none());
        assert_eq!(snap.window5h.status, "unknown");
    }

    #[test]
    fn incremental_scan_reads_appends_only() {
        let (base, wire) = make_sessions_tree();
        let now = super::super::now_millis();

        {
            let mut f = fs::File::create(&wire).unwrap();
            f.write_all(usage_line(now - 1000, (100, 0, 0, 0)).as_bytes())
                .unwrap();
        }
        let mut src = LedgerSource::new();
        src.scan(&base.join("sessions"));
        assert_eq!(src.records.len(), 1);

        // 追加一行 + 一行坏行 + 半截行
        {
            let mut f = fs::OpenOptions::new().append(true).open(&wire).unwrap();
            f.write_all(usage_line(now, (0, 50, 0, 0)).as_bytes()).unwrap();
            f.write_all(b"garbage line\n").unwrap();
            f.write_all(b"{\"type\":\"usage.record\",\"partial").unwrap();
        }
        // mtime 粒度在某些文件系统上较粗，强制等一个 tick 不可靠；
        // 直接扫描两次，第二次应只新增 1 条完整记录
        std::thread::sleep(std::time::Duration::from_millis(20));
        src.scan(&base.join("sessions"));
        assert_eq!(src.records.len(), 2);

        // 再扫一次（mtime 未变）不应重复计数
        src.scan(&base.join("sessions"));
        assert_eq!(src.records.len(), 2);

        let cfg = Config {
            week_anchor: Some(now - 24 * 3600_000), // 锚在昨天，两条记录都在本周期内
            ..Default::default()
        };
        let snap = src.aggregate(&cfg, now);
        assert_eq!(snap.week.used_tokens, 150);

        let _ = fs::remove_dir_all(&base);
    }
}
