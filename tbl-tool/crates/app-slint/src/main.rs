// 启动壳子：与 egui 端策略对齐
// - Windows 下隐藏控制台（避免双击/run 带 console；同时抑制 vm3dgl 等 OpenGL 启动日志）
//   仅 Windows 平台需要这个属性，Linux/macOS 不存在 subsystem 概念
// - CLI workdir / lock 文件 / 文件日志 / 加载 project
// - AppState 持 ProjectEngine + UI 临时态，用 Rc<RefCell<...>> 在 callback 中共享
//
// 后端模块布局：
//   state.rs    数据 / 业务态
//   theme.rs    全 crate 共享视觉常量（图标 / marker / 颜色）
//   convert/    state → UI 数据派生（tree / grid / 工具）
//   refresh.rs  跨模块刷新 fan-out（callback 末尾按"刚做了什么"选 helper）
//   ui/         主窗口区域：tree / grid / grid_actions / toolbar / log_panel / focus
//   dialogs/    子窗口：context_menu / pending / type_selector / ref_picker /
//               data_export / schema_io / template_library / new_project
//
// 每个 ui/dialogs 模块统一对外暴露 `wire(ui, state)` + `push(ui, state)`。

#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod state;
mod theme;
mod convert;
mod refresh;
mod ui;
mod dialogs;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use clap::Parser;
use log::info;
use simplelog::*;

slint::include_modules!();

use state::AppState;

#[derive(Parser)]
#[command(name = "tbl-tool", version = "0.1.0")]
struct Cli {
    #[arg(long)]
    workdir: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let workdir = match cli.workdir {
        Some(p) => std::fs::canonicalize(&p)?,
        None => {
            let exe = std::env::current_exe()?;
            exe.parent().unwrap_or(exe.as_path()).to_path_buf()
        }
    };

    let lock_path = workdir.join(tbl_core::LOCK_FILE);
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
        st.engine.active()
            .or_else(|| st.engine.projects.first())
            .and_then(|p| p.config.ui.as_ref())
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

    let log_path = workdir.join(tbl_core::LOG_FILE);
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
    {
        let st = app_state.borrow();
        app_window.set_picker_trigger_header_single(st.picker_trigger_header_single);
        app_window.set_picker_trigger_data_single(st.picker_trigger_data_single);
        app_window.set_tree_sort_index(ui::tree::sort_to_index(&st.project_sort));
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
    dialogs::template_library::wire(&app_window, &app_state);
    dialogs::new_project::wire(&app_window, &app_state);

    let result = app_window.run().map_err(|e| anyhow::anyhow!("{}", e));
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
