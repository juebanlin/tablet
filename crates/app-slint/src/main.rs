// SPDX-License-Identifier: GPL-3.0-only
// Copyright 2026 juebanlin <juebanlin@gmail.com>

// 启动壳子（@05.12 跨平台启动约定）
// - 用 console subsystem（Windows / Linux / macOS 三端统一），CLI 模式下 cmd / 终端
//   同步等待 stdout，与普通 CLI 工具一致
// - GUI 分支在 Windows 下显式 FreeConsole() 释放控制台，避免双击启动后黑窗驻留
//   （仍会有 < 50ms 的初始黑闪，PE 头层无法消除；Linux/macOS 没有 subsystem 概念，零影响）
// - 同一个 exe 兼任 CLI：classify(args) 决定走 tablet_cli 还是 GUI（@03.1 三层架构）
// - workdir 默认 = cwd；`--workdir` 仅作开发期覆盖。lock 文件 / 文件日志 / 加载 project
// - AppState 持 ProjectEngine + UI 临时态，用 Rc<RefCell<...>> 在 callback 中共享
//
// 后端模块布局：
//   state.rs    数据 / 业务态
//   theme.rs    全 crate 共享视觉常量（图标 / marker / 颜色）
//   convert/    state → UI 数据派生（tree / grid / 工具）
//   refresh.rs  跨模块刷新 fan-out（callback 末尾按"刚做了什么"选 helper）
//   ui/         主窗口区域：tree / grid / grid_actions / toolbar / log_panel / focus
//   dialogs/    子窗口：context_menu / pending / type_selector / ref_picker /
//               data_export / schema_io / create_project（统一）/ clone_project
//
// 每个 ui/dialogs 模块统一对外暴露 `wire(ui, state)` + `push(ui, state)`。

mod state;
mod theme;
mod icons;
mod convert;
mod refresh;
mod ui;
mod dialogs;
mod excel_bridge;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use log::info;
use simplelog::*;

slint::include_modules!();

use state::AppState;

/// 三条启动路径：
/// 1. 零参数 → GUI（双击启动；workdir = cwd）
/// 2. `--gui [--workdir=...]` → GUI（开发期 / 便携重定位）
/// 3. 其它任何参数 → CLI（含 `--help` / `--version` / 子命令 / tablet_cli 的 global option）
enum Route {
    Gui { workdir: Option<PathBuf> },
    Cli,
}

fn classify(args: &[String]) -> Route {
    if args.len() <= 1 {
        return Route::Gui { workdir: None };
    }
    if !args.iter().skip(1).any(|a| a == "--gui") {
        return Route::Cli;
    }
    // --gui 模式下顺便解析 --workdir=foo / --workdir foo
    let workdir = args.iter().skip(1).find_map(|a| {
        a.strip_prefix("--workdir=").map(PathBuf::from)
    }).or_else(|| {
        let pos = args.iter().position(|a| a == "--workdir")?;
        args.get(pos + 1).map(PathBuf::from)
    });
    Route::Gui { workdir }
}

/// 安装 panic hook：把 panic（消息 + 位置 + 调用栈）追加到 workdir 下的崩溃日志。
///
/// 为什么必须落到文件：
///   - GUI 分支启动时已 `FreeConsole()`，双击启动时 stderr 无人接收 —— panic 会变成
///     完全无声的"闪退"；只有 stderr 是管道/文件（cargo run / IDE / 重定向）时才看得到。
///   - 单独一个文件而不是 tablet.log：那份日志每次启动 `File::create` 截断，崩溃记录
///     会被下次启动清掉。
/// 为什么不顺手 `log::error!`：hook 可能正是在持日志锁时触发，重入会死锁。
///
/// 调用栈要想有函数名，release 包必须保留符号表（见 `[profile.release] strip`）；
/// 注意 MSVC 目标的符号只能来自外挂 pdb（无法内嵌），Windows 包因此走 windows-gnu。
fn install_crash_hook(workdir: &Path, time_offset: time::UtcOffset) {
    let path = workdir.join(tablet_core::CRASH_LOG_FILE);
    let file = match std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        Ok(f) => f,
        Err(e) => {
            log::warn!("崩溃日志无法创建 {}: {}", path.display(), e);
            return;
        }
    };
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        use std::io::Write;
        let stamp = time::OffsetDateTime::now_utc()
            .to_offset(time_offset)
            .format(time::macros::format_description!(
                "[year]-[month]-[day] [hour]:[minute]:[second]"))
            .unwrap_or_default();
        let thread = std::thread::current();
        let mut out = &file;
        let _ = writeln!(out, "===== [{stamp}] panic in thread '{}' =====",
            thread.name().unwrap_or("<unnamed>"));
        let _ = writeln!(out, "{info}");
        let _ = writeln!(out, "{}", std::backtrace::Backtrace::force_capture());
        let _ = out.flush();
        // 照旧走默认 hook：开发期在终端 / IDE 里仍能看到
        default_hook(info);
    }));
    log::info!("崩溃日志: {}", path.display());
}

#[cfg(windows)]
fn detach_console_for_gui() {
    // GUI 分支释放父 cmd 的 console，避免双击启动 / cmd 调用时黑窗驻留。
    // console subsystem 仍会在进程启动瞬间分配 console（PE 头决定，无法关闭），
    // 这里调 FreeConsole 让它尽快消失；初始的 < 50ms 黑闪客观存在。
    use windows_sys::Win32::System::Console::FreeConsole;
    unsafe { FreeConsole() };
}

#[cfg(not(windows))]
fn detach_console_for_gui() {}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match classify(&args) {
        Route::Gui { workdir } => {
            detach_console_for_gui();
            run_gui(workdir)
        }
        Route::Cli => {
            // CLI 模式下 `--gui` 不属于 tablet_cli 的语法，剔除后再转发
            let forwarded: Vec<String> = args.iter()
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
    }
}

fn run_gui(workdir_arg: Option<PathBuf>) -> anyhow::Result<()> {
    // 获取本地时区偏移（必须在早期单线程时获取，之后多线程环境下不安全）
    let time_offset = time::UtcOffset::current_local_offset()
        .unwrap_or(time::UtcOffset::UTC);

    // 默认 = 当前工作目录（cmd cwd / 双击启动时即 exe 所在目录）。
    // `--workdir` 仅作开发期便利覆盖（cargo run -p tablet-slint -- --gui --workdir=...）。
    let workdir = match workdir_arg {
        Some(p) => std::fs::canonicalize(&p)?,
        None => std::env::current_dir()?,
    };

    // 工作区进程锁：GUI 与 CLI 互斥（@03.7.5）。持有到函数结束，drop 时释放。
    let _workspace_lock = match tablet_core::workspace_lock::acquire(
        &workdir,
        tablet_core::workspace_lock::ProcKind::Gui,
    ) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("{:#}", e);
            std::process::exit(1);
        }
    };

    // 提前初始化日志系统（在 load 之前），避免配置加载时的 ICU4X 警告无法被过滤
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
        .set_time_format_custom(time::macros::format_description!("[year]-[month]-[day] [hour]:[minute]:[second].[subsecond digits:3]"))
        .set_time_offset(time_offset)
        .set_thread_level(LevelFilter::Off)
        .set_target_level(LevelFilter::Off)
        .add_filter_ignore_str("icu")
        .add_filter_ignore_str("icu4x")
        .add_filter_ignore_str("icu_segmenter")
        .build();

    CombinedLogger::init(vec![WriteLogger::new(
        file_level,
        log_config,
        log_file,
    )])?;

    // 日志系统就绪后、加载项目之前挂上：这样加载期 panic 也能留下记录
    install_crash_hook(&workdir, time_offset);

    let (app_state_inner, config_fixed, open_failures) = AppState::load(&workdir)?;
    let app_state = Rc::new(RefCell::new(app_state_inner));

    // 延迟记录配置修复日志（在日志系统初始化后）
    if config_fixed {
        log::warn!("启动时检测到配置文件无效值，已自动修复并保存");
    }

    info!(
        "loaded {} opened / {} available, {} groups in active",
        app_state.borrow().engine.projects.len(),
        app_state.borrow().engine.available().len(),
        app_state.borrow().engine.active().map(|p| p.groups.len()).unwrap_or(0),
    );

    let app_window = AppWindow::new()?;
    {
        let st = app_state.borrow();
        app_window.set_picker_trigger_header_single(st.picker_trigger_header_single);
        app_window.set_picker_trigger_data_single(st.picker_trigger_data_single);
        app_window.set_tree_sort_index(ui::tree::sort_to_index(&st.project_sort));
    }

    // 启动时打不开的项目：**中断打开 + 弹框说明**，而不是兜底成空项目静静放过去。
    // 放在这里是因为弹框要有窗口，而加载期还没有（@02.26）。
    for f in &open_failures {
        dialogs::alert::raise(
            &app_state,
            "项目打不开",
            &format!("Project {} 已跳过：
{}", f.id, f.message),
        );
    }
    if !open_failures.is_empty() {
        dialogs::alert::push(&app_window, &app_state);
    }

    // 「关于」→「检查更新」：跳 GitHub 仓库
    {
        let s = app_state.clone();
        let weak = app_window.as_weak();
        app_window.on_check_update_clicked(move || {
            const REPO_URL: &str = "https://github.com/juebanlin/tablet";
            match open::that(REPO_URL) {
                Ok(_) => s.borrow_mut().engine.ui_log(format!("[关于] 已打开 {}", REPO_URL)),
                Err(e) => s.borrow_mut().engine.error_log(format!("[关于] 打开 GitHub 失败: {}", e)),
            }
            if let Some(ui_h) = weak.upgrade() {
                refresh::after_log(&ui_h, &s);
            }
        });
    }

    // 「设置」→「全局设置...」：打开全局设置对话框
    {
        let s = app_state.clone();
        let weak = app_window.as_weak();
        app_window.on_global_settings_clicked(move || {
            if let Some(ui_h) = weak.upgrade() {
                dialogs::global_settings::open(&s, &ui_h);
                dialogs::global_settings::push(&ui_h, &s);
            }
        });
    }

    // 初次 push（首屏数据 + 各对话框默认关闭态）
    refresh::initial(&app_window, &app_state);

    // 字形常量（src/icons.rs）推给 slint 的 global Icons，.slint 侧只引用 Icons.xxx
    icons::wire(&app_window);

    // 注册全部 callback（顺序无关，只要在 ui.run() 之前）
    ui::tree::wire(&app_window, &app_state);
    ui::grid::wire(&app_window, &app_state);
    ui::grid_layout::wire(&app_window, &app_state);
    ui::toolbar::wire(&app_window, &app_state);
    ui::focus::wire(&app_window, &app_state);
    ui::log_panel::wire(&app_window, &app_state);
    ui::bottom_panel::wire(&app_window, &app_state);
    ui::excel_sync::wire(&app_window, &app_state);
    dialogs::context_menu::wire(&app_window, &app_state);
    dialogs::pending::wire(&app_window, &app_state);
    dialogs::type_selector::wire(&app_window, &app_state);
    dialogs::ref_picker::wire(&app_window, &app_state);
    dialogs::rules_panel::wire(&app_window, &app_state);
    dialogs::data_export::wire(&app_window, &app_state);
    dialogs::schema_io::wire(&app_window, &app_state);
    dialogs::create_project::wire(&app_window, &app_state);
    dialogs::clone_project::wire(&app_window, &app_state);
    dialogs::project_settings::wire(&app_window, &app_state);
    dialogs::global_settings::wire(&app_window, &app_state);
    dialogs::alert::wire(&app_window, &app_state);
    excel_bridge::wire(&app_window, &app_state);

    // 启动时扫描 Excel 编辑残留（@plans §5.6）：上次崩溃 / kill 留下的 .tbl-cache/*.xlsx
    excel_bridge::scan_residuals_on_startup(&app_state);

    // xlsx 组的外部改动轮询（@02.26.8）：返回的 Timer 必须活到窗口结束。
    let _watch_timer = ui::watch::start(&app_window, &app_state);

    let result = app_window.run().map_err(|e| anyhow::anyhow!("{}", e));
    // 用户习惯数据兜底保存（正常在拖拽结束/勾选/重置时已写）。
    {
        let mut st = app_state.borrow_mut();
        ui::grid_layout::save_current(&mut st);
    }
    // 退出清理（@plans §5.7）：清掉所有 Project 的 .tbl-cache/*.xlsx
    excel_bridge::cleanup_all_caches_on_exit(&app_state);
    result
}


#[cfg(test)]
mod crash_hook_tests {
    use super::*;

    /// 崩溃日志必须真的落盘，且是 append（不截断上一次的崩溃）。
    #[test]
    fn panic_hook_appends_to_crash_log() {
        let dir = std::env::temp_dir().join(format!("tablet-crash-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        install_crash_hook(&dir, time::UtcOffset::UTC);

        // 第一次崩溃
        let _ = std::panic::catch_unwind(|| panic!("smoke-boom-{}", 1));
        // 第二次崩溃：必须追加，不能覆盖
        let _ = std::panic::catch_unwind(|| panic!("smoke-boom-{}", 2));

        let text = std::fs::read_to_string(dir.join(tablet_core::CRASH_LOG_FILE)).unwrap();
        assert!(text.contains("smoke-boom-1"), "第一次崩溃没写进日志:\n{text}");
        assert!(text.contains("smoke-boom-2"), "第二次崩溃没写进日志(append 失效?):\n{text}");
        assert!(text.contains("panic in thread"), "缺时间戳/线程头:\n{text}");
        assert!(text.contains("main.rs"), "缺 panic 位置(file:line):\n{text}");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
