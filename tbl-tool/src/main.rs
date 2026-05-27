#![windows_subsystem = "windows"]

mod app;
mod model;
mod core;
mod ui;

use std::path::PathBuf;
use clap::Parser;
use log::info;
use simplelog::*;

pub const CONFIG_FILE: &str = "tbl-tool.toml";
pub const LOCK_FILE: &str = ".tbl-tool.lock";
pub const LOG_FILE: &str = "tbl-tool.log";

#[derive(Parser)]
#[command(name = "tbl-tool", version = "0.1.0")]
struct Cli {
    /// 工作目录（包含 tbl-tool.toml 的目录），默认为 exe 所在目录
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

    let lock_path = workdir.join(LOCK_FILE);
    if let Ok(content) = std::fs::read_to_string(&lock_path) {
        if let Ok(pid) = content.trim().parse::<u32>() {
            if is_process_alive(pid) {
                eprintln!("另一个 TBL Tool 实例正在运行 (PID: {})", pid);
                std::process::exit(1);
            }
        }
    }
    std::fs::write(&lock_path, std::process::id().to_string())?;

    let project = core::project::load_project(&workdir)?;

    let log_level = project.config.ui.as_ref()
        .and_then(|u| u.log_level.as_deref())
        .unwrap_or("debug");
    let file_level = match log_level {
        "info" => LevelFilter::Info,
        "warn" => LevelFilter::Warn,
        "error" => LevelFilter::Error,
        _ => LevelFilter::Debug,
    };

    let log_path = workdir.join(LOG_FILE);
    let log_file = std::fs::File::create(&log_path)?;
    CombinedLogger::init(vec![
        WriteLogger::new(file_level, Config::default(), log_file),
    ])?;
    info!("loaded {} groups", project.groups.len());

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([960.0, 640.0])
            .with_title("TBL Tool v0.1"),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    let result = eframe::run_native(
        "TBL Tool",
        options,
        Box::new(|cc| {
            setup_fonts(&cc.egui_ctx);
            Ok(Box::new(app::TblApp::new(project)))
        }),
    ).map_err(|e| anyhow::anyhow!("{}", e));

    let _ = std::fs::remove_file(&lock_path);
    result
}

fn is_process_alive(pid: u32) -> bool {
    use std::process::Command;
    Command::new("tasklist")
        .args(["/FI", &format!("PID eq {}", pid), "/NH"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
        .unwrap_or(false)
}

fn setup_fonts(ctx: &eframe::egui::Context) {
    use font_kit::family_name::FamilyName;
    use font_kit::properties::Properties;
    use font_kit::source::SystemSource;

    let mut fonts = eframe::egui::FontDefinitions::default();

    let candidates = [
        FamilyName::Title("Microsoft YaHei".to_string()),
        FamilyName::Title("SimHei".to_string()),
        FamilyName::Title("PingFang SC".to_string()),
        FamilyName::Title("Noto Sans CJK SC".to_string()),
        FamilyName::Title("WenQuanYi Micro Hei".to_string()),
        FamilyName::SansSerif,
    ];

    let source = SystemSource::new();
    if let Ok(handle) = source.select_best_match(&candidates, &Properties::new()) {
        if let Ok(font) = handle.load() {
            if let Some(data) = font.copy_font_data() {
                fonts.font_data.insert(
                    "system_cjk".to_owned(),
                    eframe::egui::FontData::from_owned((*data).clone()),
                );
                fonts.families
                    .entry(eframe::egui::FontFamily::Proportional)
                    .or_default()
                    .insert(0, "system_cjk".to_owned());
                fonts.families
                    .entry(eframe::egui::FontFamily::Monospace)
                    .or_default()
                    .insert(0, "system_cjk".to_owned());
            }
        }
    }

    ctx.set_fonts(fonts);
}
