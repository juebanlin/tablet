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
mod convert;
mod refresh;
mod ui;
mod dialogs;
mod excel_bridge;

use std::cell::RefCell;
use std::path::PathBuf;
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
    // 默认 = 当前工作目录（cmd cwd / 双击启动时即 exe 所在目录）。
    // `--workdir` 仅作开发期便利覆盖（cargo run -p tablet-slint -- --gui --workdir=...）。
    let workdir = match workdir_arg {
        Some(p) => std::fs::canonicalize(&p)?,
        None => std::env::current_dir()?,
    };

    let lock_path = workdir.join(tablet_core::LOCK_FILE);
    if let Ok(content) = std::fs::read_to_string(&lock_path) {
        if let Ok(pid) = content.trim().parse::<u32>() {
            if is_process_alive(pid) {
                eprintln!("另一个 TBL Tool 实例正在运行 (PID: {})", pid);
                std::process::exit(1);
            }
        }
    }
    std::fs::write(&lock_path, std::process::id().to_string())?;

    let app_state = Rc::new(RefCell::new(AppState::load(&workdir)?));

    let log_level = {
        let st = app_state.borrow();
        st.engine.global_config.ui.as_ref()
            .and_then(|u| u.log_level.as_deref())
            .unwrap_or("debug")
            .to_string()
    };
    let file_level = match log_level.as_str() {
        "info" => LevelFilter::Info,
        "warn" => LevelFilter::Warn,
        "error" => LevelFilter::Error,
        _ => LevelFilter::Debug,
    };

    let log_path = workdir.join(tablet_core::LOG_FILE);
    let log_file = std::fs::File::create(&log_path)?;
    CombinedLogger::init(vec![WriteLogger::new(
        file_level,
        Config::default(),
        log_file,
    )])?;
    info!(
        "loaded {} opened / {} available, {} groups in active",
        app_state.borrow().engine.projects.len(),
        app_state.borrow().engine.available().len(),
        app_state.borrow().engine.active().map(|p| p.groups.len()).unwrap_or(0),
    );

    let app_window = AppWindow::new()?;
    app_window.set_app_version(env!("CARGO_PKG_VERSION").into());
    {
        let st = app_state.borrow();
        app_window.set_picker_trigger_header_single(st.picker_trigger_header_single);
        app_window.set_picker_trigger_data_single(st.picker_trigger_data_single);
        app_window.set_tree_sort_index(ui::tree::sort_to_index(&st.project_sort));
    }

    // 「关于」→「检查更新」：跳 GitHub 仓库
    {
        let s = app_state.clone();
        let weak = app_window.as_weak();
        app_window.on_check_update_clicked(move || {
            const REPO_URL: &str = "https://github.com/juebanlin/tablet";
            match open::that(REPO_URL) {
                Ok(_) => s.borrow_mut().engine.log(format!("[关于] 已打开 {}", REPO_URL)),
                Err(e) => s.borrow_mut().engine.log(format!("[关于] 打开 GitHub 失败: {}", e)),
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
            dialogs::global_settings::open(&s);
            if let Some(ui_h) = weak.upgrade() {
                dialogs::global_settings::push(&ui_h, &s);
            }
        });
    }

    // 初次 push（首屏数据 + 各对话框默认关闭态）
    refresh::initial(&app_window, &app_state);

    // 注册全部 callback（顺序无关，只要在 ui.run() 之前）
    ui::tree::wire(&app_window, &app_state);
    ui::grid::wire(&app_window, &app_state);
    ui::toolbar::wire(&app_window, &app_state);
    ui::focus::wire(&app_window, &app_state);
    ui::log_panel::wire(&app_window, &app_state);
    dialogs::context_menu::wire(&app_window, &app_state);
    dialogs::pending::wire(&app_window, &app_state);
    dialogs::type_selector::wire(&app_window, &app_state);
    dialogs::ref_picker::wire(&app_window, &app_state);
    dialogs::data_export::wire(&app_window, &app_state);
    dialogs::schema_io::wire(&app_window, &app_state);
    dialogs::create_project::wire(&app_window, &app_state);
    dialogs::clone_project::wire(&app_window, &app_state);
    dialogs::project_settings::wire(&app_window, &app_state);
    dialogs::global_settings::wire(&app_window, &app_state);
    excel_bridge::wire(&app_window, &app_state);

    // 启动时扫描 Excel 编辑残留（@plans §5.6）：上次崩溃 / kill 留下的 .tbl-cache/*.xlsx
    excel_bridge::scan_residuals_on_startup(&app_state);

    let result = app_window.run().map_err(|e| anyhow::anyhow!("{}", e));
    // 退出清理（@plans §5.7）：清掉所有 Project 的 .tbl-cache/*.xlsx
    excel_bridge::cleanup_all_caches_on_exit(&app_state);
    let _ = std::fs::remove_file(&lock_path);
    result
}

fn is_process_alive(pid: u32) -> bool {
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {}", pid), "/NH"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
            .unwrap_or(false)
    }
    #[cfg(not(target_os = "windows"))]
    {
        // Unix-like：kill 0 不发送信号，仅检查目标进程是否存在
        use std::process::Command;
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}
