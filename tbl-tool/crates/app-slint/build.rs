// slint UI 编译。
//
// style（widget 视觉风格）按"目标平台"选，而不是按编译机器。
// slint-build 默认会用编译机器的 OS 来推断 style（Windows→fluent / macOS→cupertino /
// Linux→cosmic），跨平台编译时会选错（在 Windows 编 Linux exe 还是 fluent）。
//
// 这里读 cargo 的 CARGO_CFG_TARGET_OS 显式映射到匹配的 style：
//   - Windows → fluent
//   - macOS   → cupertino
//   - 其它    → cosmic（Linux / 嵌入式）
//
// 用户也可以用环境变量 SLINT_STYLE 显式覆盖，跳过这段映射。
fn main() {
    let style = std::env::var("SLINT_STYLE").ok().unwrap_or_else(|| {
        match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
            Ok("windows") => "fluent".to_string(),
            Ok("macos") | Ok("ios") => "cupertino".to_string(),
            _ => "cosmic".to_string(),
        }
    });

    slint_build::compile_with_config(
        "ui/app.slint",
        slint_build::CompilerConfiguration::new().with_style(style),
    )
    .expect("Slint build failed");
}
