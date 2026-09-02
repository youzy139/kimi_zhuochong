//! CLI /usage 抓取数据源（增强源，尽力而为）：
//! 用 portable-pty 在隐藏 pty 里跑 `kimi`，发送 `/usage\r`，收集输出、
//! 剥离 ANSI 转义后解析额度信息。任何一步失败都 Err，由编排层降级 ledger。
//!
//! probe 结论（kimi 0.28.1 实测，完整输出见项目根 probe_usage_output.txt）：
//! /usage 弹出一个 Usage 面板，关键行形如：
//!   │   Weekly limit  ██░░░░░░░░░░░░░░░░░░  10% used  resets in 1h 23m │
//!   │   5h limit      █████░░░░░░░░░░░░░░░  24% used  resets in 23m    │
//! 上方还有 Context window 段落（"░░░░  0%  (0 / 1M)"），其百分数
//! 与额度无关，解析时必须排除。首次 probe 没有遇到引导流程阻塞，
//! TUI 直接就绪；代码里仍保留了「输出过少时补发回车」的兜底。

use super::{UsageSnapshot, UsageSource, WeekInfo, Window5h};
use crate::config::Config;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::Read;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// 整体超时
const TOTAL_TIMEOUT: Duration = Duration::from_secs(15);

pub struct CliUsageSource;

impl CliUsageSource {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CliUsageSource {
    fn default() -> Self {
        Self::new()
    }
}

impl UsageSource for CliUsageSource {
    fn fetch(&mut self, cfg: &Config) -> Result<UsageSnapshot, String> {
        let raw = capture_usage_raw(TOTAL_TIMEOUT)?;
        let text = strip_ansi(&raw);
        let parsed = parse_usage_text(&text);
        if parsed.week_percent.is_none() && parsed.window_percent.is_none() {
            return Err("/usage 输出中未解析到任何额度信息".to_string());
        }

        let now = super::now_millis();
        // 映射到快照；缺失字段留 None/0，由编排层用 ledger 补齐
        let mut week = WeekInfo::default();
        if let Some(p) = parsed.week_percent {
            week.percent = Some(p);
            if let Some(q) = cfg.weekly_quota_tokens {
                week.total_tokens = Some(q);
                week.used_tokens = (p / 100.0 * q as f64).round() as u64;
            }
        }
        if let Some(reset_in) = parsed.week_reset_in_ms {
            week.reset_at = Some(now + reset_in);
        }
        let mut window5h = Window5h::default();
        if let Some(p) = parsed.window_percent {
            // 窗口没有 token 绝对值，用百分比粗估状态
            window5h.status = if p >= 80.0 { "warn" } else { "ok" }.to_string();
            if let Some(th) = cfg.window5h_warn_tokens {
                window5h.warn_threshold = Some(th);
                window5h.used_tokens = (p / 100.0 * th as f64).round() as u64;
            }
        }

        Ok(UsageSnapshot {
            source: "cli".to_string(),
            week,
            window5h,
            fuel_pack: None,
            last_turn: None,
            updated_at: now,
        })
    }
}

/// /usage 文本解析结果
#[derive(Debug, Default, Clone)]
pub struct UsageTextParse {
    pub week_percent: Option<f64>,
    pub window_percent: Option<f64>,
    /// 「resets in 1h 23m」换算成的毫秒数
    pub week_reset_in_ms: Option<i64>,
}

/// 解析剥离 ANSI 后的 /usage 输出（真实格式见文件头注释）。
/// 规则：
/// 1. 含 "context" 的行跳过（Context window 百分比与额度无关）；
/// 2. 含 "weekly"/"week"/"周" 的行 → 周额度，同时解析 "resets in X"；
/// 3. 含 "5h"/"5 小时" 的行 → 5 小时窗口；
/// 4. 兜底：周额度仍为空时，取第一个非 context 的百分数。
pub fn parse_usage_text(text: &str) -> UsageTextParse {
    let mut out = UsageTextParse::default();
    let mut first_free_percent: Option<f64> = None;
    for line in text.lines() {
        let lower = line.to_lowercase();
        if lower.contains("context") {
            continue;
        }
        let Some(pct) = find_percent(line) else {
            continue;
        };
        let is_week = lower.contains("weekly") || lower.contains("week") || line.contains("周");
        let is_window = lower.contains("5h")
            || lower.contains("5 h")
            || line.contains("5 小时")
            || line.contains("5小时");
        if is_week && out.week_percent.is_none() {
            out.week_percent = Some(pct);
            out.week_reset_in_ms = find_reset_in_ms(&lower);
        } else if is_window && out.window_percent.is_none() {
            out.window_percent = Some(pct);
        } else if first_free_percent.is_none() {
            first_free_percent = Some(pct);
        }
    }
    if out.week_percent.is_none() {
        out.week_percent = first_free_percent;
    }
    out
}

/// 解析 "resets in 1h 23m" 这类时长，返回毫秒。
/// 支持 d/h/m/s 单位组合，遇到无法识别的词就停止。
fn find_reset_in_ms(lower_line: &str) -> Option<i64> {
    let idx = lower_line.find("resets in")?;
    let rest = lower_line[idx + "resets in".len()..].trim_start();
    let mut total_ms: i64 = 0;
    let mut matched = false;
    for tok in rest.split_whitespace() {
        // 取最后一个字符作单位（注意 tok 可能是多字节字符如 │，不能用字节下标切）
        let mut chars = tok.chars();
        let Some(unit) = chars.next_back() else {
            break;
        };
        let Ok(n) = chars.as_str().parse::<i64>() else {
            break;
        };
        let factor = match unit {
            'd' => 24 * 3600 * 1000,
            'h' => 3600 * 1000,
            'm' => 60 * 1000,
            's' => 1000,
            _ => break,
        };
        total_ms += n * factor;
        matched = true;
    }
    matched.then_some(total_ms)
}

/// 取一行中第一个百分数（如 "42.5%"）
fn find_percent(line: &str) -> Option<f64> {
    let bytes = line.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'%' {
            // 向前收集数字与小数点
            let mut start = i;
            while start > 0 && (bytes[start - 1].is_ascii_digit() || bytes[start - 1] == b'.') {
                start -= 1;
            }
            if start < i {
                if let Ok(v) = line[start..i].parse::<f64>() {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// 在 pty 中运行 `kimi` 并抓取 /usage 原始输出（含 ANSI）。
/// probe 与正式抓取共用此函数。
pub fn capture_usage_raw(timeout: Duration) -> Result<String, String> {
    let start = Instant::now();
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("openpty 失败: {e}"))?;

    // 用临时目录起新会话，避免污染用户工作区
    let tmp = std::env::temp_dir().join(format!("kimi-rabbit-usage-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp);

    let mut cmd = CommandBuilder::new("kimi");
    cmd.cwd(&tmp);
    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("启动 kimi 失败: {e}"))?;
    // slave 句柄尽快释放，避免阻塞
    drop(pair.slave);

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("克隆 pty reader 失败: {e}"))?;
    let mut writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("获取 pty writer 失败: {e}"))?;

    // 后台线程持续把 pty 输出推进 channel
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });

    let mut out: Vec<u8> = Vec::new();
    let mut last_chunk_at = Instant::now();
    let mut sent_usage = false;
    let mut sent_enter = false;
    let deadline = start + timeout;

    // 轮询：先等 TUI 启动（首个输出后静默 1.5s 视为就绪），发 /usage，
    // 再等输出静默 2s 视为结果收集完成。首会话若有引导流程，补发回车尝试跳过。
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(150)) {
            Ok(chunk) => {
                out.extend_from_slice(&chunk);
                last_chunk_at = Instant::now();
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        let idle = last_chunk_at.elapsed();
        if !sent_usage {
            if !out.is_empty() && idle > Duration::from_millis(1500) {
                use std::io::Write;
                if !sent_enter && out.len() < 4096 {
                    // 输出很少，可能卡在引导页：先补一个回车试试
                    let _ = writer.write_all(b"\r");
                    let _ = writer.flush();
                    sent_enter = true;
                    last_chunk_at = Instant::now();
                    continue;
                }
                let _ = writer.write_all(b"/usage\r");
                let _ = writer.flush();
                sent_usage = true;
                last_chunk_at = Instant::now();
            }
        } else if idle > Duration::from_secs(2) {
            break; // 结果已静默 2 秒，收集完成
        }
    }

    let _ = child.kill();
    let _ = child.wait();

    if out.is_empty() {
        return Err("kimi 进程没有任何输出".to_string());
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// 剥离 ANSI 转义序列（CSI / OSC / 单字符序列），并把 \r\n 规整为 \n
pub fn strip_ansi(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == 0x1b {
            // ESC
            i += 1;
            if i >= bytes.len() {
                break;
            }
            match bytes[i] {
                b'[' => {
                    // CSI：跳到 final byte 0x40..=0x7E
                    i += 1;
                    while i < bytes.len() {
                        let c = bytes[i];
                        i += 1;
                        if (0x40..=0x7e).contains(&c) {
                            break;
                        }
                    }
                }
                b']' => {
                    // OSC：跳到 BEL 或 ESC \
                    i += 1;
                    while i < bytes.len() {
                        if bytes[i] == 0x07 {
                            i += 1;
                            break;
                        }
                        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                            i += 2;
                            break;
                        }
                        i += 1;
                    }
                }
                _ => {
                    // 其他单字符转义（如 ESC ( X 的后续字符也一并跳过）
                    i += 1;
                }
            }
            continue;
        }
        if b == b'\r' {
            i += 1;
            continue; // 丢掉回车
        }
        // 普通字节（含 UTF-8 多字节）：按 UTF-8 边界拷贝
        let len = utf8_len(b);
        let end = (i + len).min(bytes.len());
        out.push_str(&String::from_utf8_lossy(&bytes[i..end]));
        i = end;
    }
    out
}

fn utf8_len(first: u8) -> usize {
    if first < 0x80 {
        1
    } else if first >> 5 == 0b110 {
        2
    } else if first >> 4 == 0b1110 {
        3
    } else if first >> 3 == 0b11110 {
        4
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_ansi_removes_csi_and_osc() {
        let raw = "\x1b[1;31m红字\x1b[0m 普通 \x1b]0;标题\x07内容\x1b[2K行";
        assert_eq!(strip_ansi(raw), "红字 普通 内容行");
    }

    #[test]
    fn strip_ansi_normalizes_crlf() {
        assert_eq!(strip_ansi("a\r\nb\rc"), "a\nbc");
    }

    #[test]
    fn strip_ansi_keeps_utf8() {
        assert_eq!(strip_ansi("月兔娘\x1b[36m额度\x1b[0m"), "月兔娘额度");
    }

    #[test]
    fn find_percent_works() {
        assert_eq!(find_percent("已用 42.5%"), Some(42.5));
        assert_eq!(find_percent("100%"), Some(100.0));
        assert_eq!(find_percent("没有百分号"), None);
    }

    #[test]
    fn parse_classifies_week_and_window() {
        let text = "Weekly quota: 12.3% used\n5h rate limit window: 40% used";
        let p = parse_usage_text(text);
        assert_eq!(p.week_percent, Some(12.3));
        assert_eq!(p.window_percent, Some(40.0));
    }

    #[test]
    fn parse_chinese_keywords() {
        let text = "本周额度已用 25%\n5 小时窗口已用 60%";
        let p = parse_usage_text(text);
        assert_eq!(p.week_percent, Some(25.0));
        assert_eq!(p.window_percent, Some(60.0));
    }

    /// kimi 0.28.1 实测 /usage 面板格式（剥离 ANSI 后）
    #[test]
    fn parse_real_kimi_028_format() {
        let text = " │ Session usage                                                    │\n \
                     │   No token usage recorded yet.                                   │\n \
                     │ Context window                                                   │\n \
                     │   ░░░░░░░░░░░░░░░░░░░░      0%  (0 / 1M)                         │\n \
                     │ Plan usage                                                       │\n \
                     │   Weekly limit  ██░░░░░░░░░░░░░░░░░░  10% used  resets in 1h 23m │\n \
                     │   5h limit      █████░░░░░░░░░░░░░░░  24% used  resets in 23m    │";
        let p = parse_usage_text(text);
        assert_eq!(p.week_percent, Some(10.0));
        assert_eq!(p.window_percent, Some(24.0));
        // 1h 23m = 4980 秒
        assert_eq!(p.week_reset_in_ms, Some(4980 * 1000));
    }

    #[test]
    fn context_window_percent_is_not_misread() {
        // Context window 在周额度之前出现，其 0% 不得被当作周额度
        let text = "Context window\n  ░░░░  0%  (0 / 1M)\nWeekly limit  ██  42% used  resets in 2d 3h";
        let p = parse_usage_text(text);
        assert_eq!(p.week_percent, Some(42.0));
        assert_eq!(p.week_reset_in_ms, Some((2 * 24 * 3600 + 3 * 3600) * 1000));
    }

    #[test]
    fn find_reset_in_ms_units() {
        assert_eq!(find_reset_in_ms("resets in 23m"), Some(23 * 60 * 1000));
        assert_eq!(find_reset_in_ms("resets in 1h 23m │"), Some(4980 * 1000));
        assert_eq!(find_reset_in_ms("resets in 45s"), Some(45 * 1000));
        assert_eq!(find_reset_in_ms("没有这句"), None);
    }
}
