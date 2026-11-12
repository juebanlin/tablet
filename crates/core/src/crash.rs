// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 juebanlin <juebanlin@gmail.com>

//! 崩溃日志：panic 时把「位置 + 现场」追加到 `<workdir>/tablet-crash.log`。
//!
//! 为什么必须落文件：GUI 分支启动时已 `FreeConsole()`，双击启动时 stderr 无人接收，
//! panic 会变成完全无声的闪退。为什么不复用 `tablet.log`：那份日志每次启动截断，
//! 上一次崩溃的记录会被下次启动清掉；崩溃日志只 append（@docs/17 §5）。
//!
//! 为什么不需要符号文件：`PanicHookInfo` 的 `Display` / `location()` 里的
//! `file:line:col` 是编译期常量，与 pdb / strip / LTO 全部无关；backtrace 只在真能
//! 解析出符号时才写，用户机上不产生十几行 `<unknown>` 噪音（@docs/17 §5.5）。
//!
//! hook 里禁止用 `log::` 宏：可能正持有日志锁，重入即死锁。读日志尾巴直接读文件。

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::panic::PanicHookInfo;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::CRASH_LOG_FILE;

/// 崩溃前附带的日志行数 / 字节上限；单行超过 `MAX_LINE_CHARS` 字符会被截断。
/// 目的是保留「用户怎么走到这」的线索，而不是把整个 schema 灌进崩溃日志。
const TAIL_LINES: usize = 20;
const TAIL_BYTES: usize = 8 * 1024;
const MAX_LINE_CHARS: usize = 512;

/// 构建期注入的目标三元组（`build.rs`），两份 GUI 也用它做「关于 / 状态栏」展示。
pub const BUILD_TARGET: &str = env!("BUILD_TARGET");
/// 构建时间（ISO8601 UTC）：同一版本号有多份构建时用来核对。
pub const BUILD_TIME: &str = env!("BUILD_TIME");

/// 崩溃日志安装器。Tauri GUI 与 CLI 共用同一实现，`app` 用来标识程序名。
pub struct CrashLog {
    app: &'static str,
    workdir: PathBuf,
    /// `tablet.log` 的路径：崩溃时读它的尾巴，回答「崩之前用户在做什么」。
    log_path: PathBuf,
    time_offset: time::UtcOffset,
}

impl CrashLog {
    pub fn new(
        app: &'static str,
        workdir: &Path,
        log_path: &Path,
        time_offset: time::UtcOffset,
    ) -> Self {
        Self {
            app,
            workdir: workdir.to_path_buf(),
            log_path: log_path.to_path_buf(),
            time_offset,
        }
    }

    /// 安装 panic hook（替换当前 hook，并链回它）。
    ///
    /// 打不开崩溃日志时只 warn 一次就返回：安装 hook 这一步本身绝不能 panic。
    pub fn install(self) {
        let crash_path = self.workdir.join(CRASH_LOG_FILE);
        let file = match OpenOptions::new()
            .create(true)
            .append(true)
            .open(&crash_path)
        {
            Ok(f) => f,
            Err(e) => {
                log::warn!("崩溃日志无法创建 {}: {}", crash_path.display(), e);
                return;
            }
        };

        // Mutex：多个线程同时 panic 时避免两条记录交错。hook 内部不会 panic，
        // 因此不存在「持锁时再 panic」的死锁路径；中毒也照样取出文件继续写。
        let sink = Mutex::new(file);
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            {
                let mut file = sink.lock().unwrap_or_else(|e| e.into_inner());
                let _ = write_report(&mut file, &self, info);
                let _ = file.flush();
            }
            // 照旧走默认 hook：开发期在终端 / IDE 里仍能看到完整 panic 输出。
            default_hook(info);
        }));
        log::info!("崩溃日志: {}", crash_path.display());
    }
}

fn write_report(out: &mut File, cfg: &CrashLog, info: &PanicHookInfo<'_>) -> std::io::Result<()> {
    let stamp = time::OffsetDateTime::now_utc()
        .to_offset(cfg.time_offset)
        .format(time::macros::format_description!(
            "[year]-[month]-[day] [hour]:[minute]:[second]"))
        .unwrap_or_default();

    writeln!(out, "===== [{stamp}] panic =====")?;
    writeln!(out, "app      : {}", cfg.app)?;
    writeln!(out, "version  : {}", env!("CARGO_PKG_VERSION"))?;
    writeln!(out, "target   : {BUILD_TARGET}")?;
    writeln!(out, "built    : {BUILD_TIME}")?;
    writeln!(out, "os       : {}", os_info::get())?;
    let thread = std::thread::current();
    writeln!(out, "thread   : {}", thread.name().unwrap_or("<unnamed>"))?;
    // 核心字段：位置是编译期常量，不依赖符号文件。
    match info.location() {
        Some(loc) => writeln!(out, "位置     : {}:{}:{}", loc.file(), loc.line(), loc.column())?,
        None => writeln!(out, "位置     : <无位置信息>")?,
    }
    writeln!(out, "消息     : {}", payload_message(info.payload()))?;
    writeln!(out, "--- 崩溃前最后 {TAIL_LINES} 条日志 ---")?;
    write!(out, "{}", tail(&cfg.log_path))?;
    if let Some(bt) = symbolized_backtrace() {
        writeln!(out, "--- backtrace ---")?;
        writeln!(out, "{bt}")?;
    }
    writeln!(out)?;
    Ok(())
}

/// 消息统一压成单行：多行 panic 消息会把「字段: 值」的版式撑坏，反而不易扫读。
fn payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    let raw = if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<非字符串 payload>".to_string()
    };
    raw.replace(['\n', '\r'], " ")
}

/// 读日志文件尾部：整读 + 截断（本项目日志量小，不值得按块倒读）。
fn tail(path: &Path) -> String {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        // 日志还没创建（极早崩溃）时不写错误，留空即可。
        Err(_) => return String::new(),
    };
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(TAIL_LINES);
    let mut out = String::new();
    for line in &lines[start..] {
        let line: String = line.chars().take(MAX_LINE_CHARS).collect();
        if out.len() + line.len() + 1 > TAIL_BYTES {
            break;
        }
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// 单文件发布包没有符号：`force_capture` 只会给十几行 `<unknown>`，纯噪音。
/// 有符号（dev profile / 打包时保留符号表）才写这一段。
fn symbolized_backtrace() -> Option<String> {
    let text = std::backtrace::Backtrace::force_capture().to_string();
    if text.trim().is_empty() || text.contains("<unknown>") {
        None
    } else {
        Some(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// panic hook 是进程级全局状态：用例必须串行，否则互相顶掉对方的 hook。
    static HOOK_LOCK: Mutex<()> = Mutex::new(());

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tablet-crash-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 装好 hook 跑一段闭包，结束后恢复调用方原本的 hook。
    fn with_hook<T>(dir: &Path, f: impl FnOnce() -> T) -> T {
        let prev = std::panic::take_hook();
        // 测试期把「链回的默认 hook」换成静音版，避免 panic 输出刷屏。
        std::panic::set_hook(Box::new(|_| {}));
        CrashLog::new("test-app", dir, &dir.join(crate::LOG_FILE), time::UtcOffset::UTC).install();
        let out = f();
        std::panic::set_hook(prev);
        out
    }

    /// 验收第一条：append 语义 —— 连续两次 panic，两条都在文件里。
    #[test]
    fn crash_log_appends_and_keeps_both_panics() {
        let _guard = HOOK_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = temp_dir("append");

        with_hook(&dir, || {
            let _ = std::panic::catch_unwind(|| panic!("first-crash"));
            let _ = std::panic::catch_unwind(|| panic!("second-crash"));
        });

        let text = std::fs::read_to_string(dir.join(CRASH_LOG_FILE)).unwrap();
        assert!(text.contains("first-crash"), "第一次崩溃没写进日志:\n{text}");
        assert!(text.contains("second-crash"), "append 失效，第二次覆盖了第一次:\n{text}");
        assert_eq!(text.matches("===== [").count(), 2, "应当正好两条记录:\n{text}");
        assert!(text.contains("app      : test-app"), "缺 app 字段:\n{text}");
        assert!(text.contains("version  : "), "缺 version 字段:\n{text}");
        assert!(text.contains("target   : "), "缺 target 字段:\n{text}");
        assert!(text.contains("--- 崩溃前最后 20 条日志 ---"), "缺日志尾巴标题:\n{text}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 验收第二条：位置字段必须是 `<file>:<line>:<col>`，不依赖任何符号文件。
    #[test]
    fn crash_log_records_source_location() {
        let _guard = HOOK_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = temp_dir("location");

        with_hook(&dir, || {
            let _ = std::panic::catch_unwind(|| panic!("location-probe"));
        });

        let text = std::fs::read_to_string(dir.join(CRASH_LOG_FILE)).unwrap();
        assert!(text.contains("位置     : "), "缺位置字段:\n{text}");
        assert!(text.contains("crash.rs:"), "位置里没有文件名:\n{text}");
        // 形如 crates/core/src/crash.rs:233:17 —— 三段都要有。
        let loc_line = text
            .lines()
            .find(|l| l.starts_with("位置"))
            .expect("没有位置行");
        let loc = loc_line.split(':').skip(1).collect::<Vec<_>>().join(":");
        let parts: Vec<&str> = loc.trim().rsplitn(3, ':').collect();
        assert_eq!(parts.len(), 3, "位置不是 file:line:col 形态: {loc_line}");
        assert!(parts[0].parse::<u32>().is_ok(), "列号不是数字: {loc_line}");
        assert!(parts[1].parse::<u32>().is_ok(), "行号不是数字: {loc_line}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 日志尾巴只保留最后 20 行，且单行超长会被截断——避免崩溃日志被整份 schema 撑爆。
    #[test]
    fn crash_log_tail_is_bounded() {
        let dir = temp_dir("tail");
        let log = dir.join(crate::LOG_FILE);
        let mut body = String::new();
        for i in 0..50 {
            body.push_str(&format!("line-{i}\n"));
        }
        body.push_str(&"x".repeat(4096));
        body.push('\n');
        std::fs::write(&log, body).unwrap();

        let text = tail(&log);
        assert!(!text.contains("line-29"), "保留了超过 20 行:\n{text}");
        assert!(text.contains("line-49"), "丢了最后一行:\n{text}");
        assert!(text.lines().count() <= TAIL_LINES, "行数超上限");
        assert!(text.len() <= TAIL_BYTES, "字节数超上限");
        assert!(!text.contains(&"x".repeat(MAX_LINE_CHARS + 1)), "单行没截断");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
