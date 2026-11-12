// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 juebanlin <juebanlin@gmail.com>

//! GUI 壳子共用的启动范式（@docs/17 §4）。
//!
//! Tauri GUI 只应该提供「窗口 + 画界面」。启动分流、工作区锁、
//! 日志初始化、崩溃日志安装、控制台释放这些与 UI 无关的事统一放在这里，
//! 避免每个 `main.rs` 各养一份、慢慢分叉。

use std::path::{Path, PathBuf};

use log::info;
use simplelog::{CombinedLogger, LevelFilter, WriteLogger};

/// 三条启动路径（@03.1）：
/// 1. 零参数 → GUI（双击启动；workdir = cwd）
/// 2. `--gui [--workdir=...]` → GUI（开发期 / 便携重定位）
/// 3. 其它任何参数 → CLI（含 `--help` / `--version` / 子命令 / 全局参数）
pub enum Route {
    Gui { workdir: Option<PathBuf> },
    Cli,
}

pub fn classify(args: &[String]) -> Route {
    if args.len() <= 1 {
        return Route::Gui { workdir: None };
    }
    if !args.iter().skip(1).any(|a| a == "--gui") {
        return Route::Cli;
    }
    Route::Gui {
        workdir: parse_workdir_arg(args),
    }
}

/// 解析 `--workdir=foo` / `--workdir foo`。
pub fn parse_workdir_arg(args: &[String]) -> Option<PathBuf> {
    args.iter()
        .skip(1)
        .find_map(|a| a.strip_prefix("--workdir=").map(PathBuf::from))
        .or_else(|| {
            let pos = args.iter().position(|a| a == "--workdir")?;
            args.get(pos + 1).map(PathBuf::from)
        })
}

/// 默认 = 当前工作目录（cmd cwd / 双击启动时即 exe 所在目录）。
pub fn resolve_workdir(arg: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    match arg {
        Some(p) => Ok(std::fs::canonicalize(&p)?),
        None => Ok(std::env::current_dir()?),
    }
}

/// GUI 启动时持有的一整套「与界面无关」的资源。
///
/// 锁必须活到窗口结束（drop 时释放）；workdir 给引擎加载。日志路径 / 时区当前只用于
/// 启动期初始化，尚未被窗口读取，因此不放进结构体（避免 dead_code）。
pub struct Shell {
    pub workdir: PathBuf,
    _lock: tablet_core::workspace_lock::WorkspaceLock,
}

impl Shell {
    /// 取锁 → 初始化日志 → 安装崩溃日志 hook。
    ///
    /// `app` 是崩溃日志里的程序名（Tauri 单 GUI，固定为 `tablet`）。
    pub fn init(app: &'static str, workdir_arg: Option<PathBuf>) -> anyhow::Result<Self> {
        // 本地时区必须在早期单线程阶段取；之后多线程环境下 API 会返回 None。
        let time_offset = time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC);
        let workdir = resolve_workdir(workdir_arg)?;

        // 工作区进程锁：两套 GUI 与 CLI 互斥（@03.7.5）。持有到窗口结束。
        let lock = tablet_core::workspace_lock::acquire(
            &workdir,
            tablet_core::workspace_lock::ProcKind::Gui,
        )?;

        init_observability(app, &workdir, time_offset)?;

        info!(
            "{app} {} ({}) 启动，workdir={}",
            env!("CARGO_PKG_VERSION"),
            tablet_core::crash::BUILD_TARGET,
            workdir.display()
        );

        Ok(Self {
            workdir,
            _lock: lock,
        })
    }
}

/// 日志 + 崩溃日志。与工作区锁解耦：`--crash-selftest` 不取锁也能跑。
pub fn init_observability(
    app: &'static str,
    workdir: &Path,
    time_offset: time::UtcOffset,
) -> anyhow::Result<PathBuf> {
    let log_path = init_logging(workdir, time_offset)?;
    tablet_core::crash::CrashLog::new(app, workdir, &log_path, time_offset).install();
    Ok(log_path)
}

/// 日志系统初始化：`<workdir>/tablet.log`（每次启动截断，这是有意的）。
/// 返回日志路径 —— 崩溃日志会读它的尾巴作为「崩溃前最后 N 条」。
///
/// 必须早于引擎加载：配置加载期的 ICU4X 警告要先有过滤器。
fn init_logging(workdir: &Path, time_offset: time::UtcOffset) -> anyhow::Result<PathBuf> {
    let log_path = workdir.join(tablet_core::LOG_FILE);
    let log_file = std::fs::File::create(&log_path)?;

    // 先快速读取 log_level 配置（避免完整加载）
    let log_level = {
        let config_path = workdir.join(tablet_core::CONFIG_FILE);
        if config_path.exists() {
            std::fs::read_to_string(&config_path)
                .ok()
                .and_then(|text| {
                    // 简单正则匹配 log_level = "xxx"
                    text.lines()
                        .find(|line| line.trim().starts_with("log_level"))
                        .and_then(|line| {
                            line.split('=')
                                .nth(1)
                                .map(|s| s.trim().trim_matches('"').to_string())
                        })
                })
                .unwrap_or_else(|| "debug".to_string())
        } else {
            "debug".to_string()
        }
    };
    let file_level = match log_level.as_str() {
        "info" => LevelFilter::Info,
        "warn" => LevelFilter::Warn,
        "error" => LevelFilter::Error,
        _ => LevelFilter::Debug,
    };

    // 自定义 Config：Java 风格日志格式 + 过滤 ICU4X 日语分词警告
    let log_config = simplelog::ConfigBuilder::new()
        .set_time_format_custom(time::macros::format_description!(
            "[year]-[month]-[day] [hour]:[minute]:[second].[subsecond digits:3]"))
        .set_time_offset(time_offset)
        .set_thread_level(LevelFilter::Off)
        .set_target_level(LevelFilter::Off)
        .add_filter_ignore_str("icu")
        .add_filter_ignore_str("icu4x")
        .add_filter_ignore_str("icu_segmenter")
        .build();

    CombinedLogger::init(vec![WriteLogger::new(file_level, log_config, log_file)])?;
    Ok(log_path)
}

/// 统一入口：`main` 只留一行 `dispatch("tablet", run_gui)`。
///
/// CLI 分支在 `tauri::Builder` 之前就 `process::exit`，退出码原样透传
/// （Jenkins 依赖）；GUI 分支先释放 Windows 控制台再起窗口。
pub fn dispatch(app: &'static str, launch_gui: fn(Option<PathBuf>) -> anyhow::Result<()>) -> ! {
    let args: Vec<String> = std::env::args().collect();

    // 隐藏自检参数必须在 classify 之前拦截，否则会被当成 CLI 参数转给 tablet_cli。
    if args.iter().skip(1).any(|a| a == "--crash-selftest") {
        crash_selftest(app, &args);
    }

    match classify(&args) {
        Route::Gui { workdir } => {
            detach_console_for_gui();
            match launch_gui(workdir) {
                Ok(()) => std::process::exit(0),
                Err(e) => {
                    eprintln!("{:#}", e);
                    std::process::exit(1);
                }
            }
        }
        Route::Cli => run_cli(&args),
    }
}

/// CLI 分支：原样转发给 tablet_cli。
fn run_cli(args: &[String]) -> ! {
    // `--gui` 不属于 tablet_cli 的语法，转发前剔除。
    let forwarded: Vec<String> = args
        .iter()
        .filter(|a| a.as_str() != "--gui")
        .cloned()
        .collect();
    match tablet_cli::run_with_args(&forwarded) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            if let Some(clap_err) = e.downcast_ref::<clap::Error>() {
                clap_err.print().ok();
                std::process::exit(clap_err.exit_code());
            }
            eprintln!("{:#}", e);
            std::process::exit(1);
        }
    }
}

/// 隐藏自检参数 `--crash-selftest`（@docs/17 §7）：故意 panic 一次，验证崩溃日志链路。
/// 不取工作区锁：自检只写日志文件、不碰项目数据，别的实例开着也应当能跑。
fn crash_selftest(app: &'static str, args: &[String]) -> ! {
    let time_offset = time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC);
    let workdir = match resolve_workdir(parse_workdir_arg(args)) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{:#}", e);
            std::process::exit(1);
        }
    };
    if let Err(e) = init_observability(app, &workdir, time_offset) {
        eprintln!("{:#}", e);
        std::process::exit(1);
    }
    log::info!("崩溃日志自检开始（--crash-selftest）");
    panic!("崩溃日志自检：这是 --crash-selftest 故意触发的 panic，不是缺陷");
}

/// GUI 分支释放父 cmd 的 console，避免双击启动 / cmd 调用时黑窗驻留。
/// console subsystem 仍会在进程启动瞬间分配 console（PE 头决定，无法关闭），
/// 这里调 FreeConsole 让它尽快消失；初始的 < 50ms 黑闪客观存在。
#[cfg(windows)]
pub fn detach_console_for_gui() {
    use windows_sys::Win32::System::Console::FreeConsole;
    unsafe { FreeConsole() };
}

#[cfg(not(windows))]
pub fn detach_console_for_gui() {}
