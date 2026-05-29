// 最小可运行壳子：加载 AppWindowPreview 灌好的示例数据，验证 slint 编译链路。
// 后续步骤再把 tbl-core 的真实数据接到 AppWindow 上。

slint::include_modules!();

fn main() -> anyhow::Result<()> {
    init_logger();

    let ui = AppWindowPreview::new()?;
    ui.run()?;
    Ok(())
}

fn init_logger() {
    use simplelog::*;
    let _ = TermLogger::init(
        LevelFilter::Info,
        Config::default(),
        TerminalMode::Mixed,
        ColorChoice::Auto,
    );
}
