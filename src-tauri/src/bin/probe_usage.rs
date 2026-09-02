//! 一次性探针：抓取 `kimi` TUI 中 `/usage` 的真实输出，
//! 原始（含 ANSI）与剥离后的文本都写入当前目录的 probe_usage_output.txt。
//! 用法：cargo run --bin probe_usage

use kimi_rabbit_widget::sources::cli_usage::{capture_usage_raw, strip_ansi};
use std::time::Duration;

fn main() {
    println!("启动 kimi 抓取 /usage，最长 20 秒……");
    match capture_usage_raw(Duration::from_secs(20)) {
        Ok(raw) => {
            let stripped = strip_ansi(&raw);
            let content =
                format!("===== RAW (含 ANSI) =====\n{raw}\n\n===== STRIPPED =====\n{stripped}\n");
            if let Err(e) = std::fs::write("probe_usage_output.txt", &content) {
                eprintln!("写文件失败: {e}");
                std::process::exit(1);
            }
            println!("已写入 probe_usage_output.txt（{} 字节）", content.len());
            println!("---- 剥离后预览 ----");
            println!("{stripped}");
        }
        Err(e) => {
            eprintln!("抓取失败: {e}");
            std::process::exit(1);
        }
    }
}
