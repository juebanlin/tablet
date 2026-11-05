// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 juebanlin <juebanlin@gmail.com>

// Windows 清单 / 图标嵌入 + 前端资源收集。Tauri 的常规构建入口。
fn main() {
    // Univer 编辑器是 Vite 产物（crates/app-tauri/univer-editor），不随源码提交。
    // 这里只做探测，让后端能在资源缺失时给出可读错误，而不是弹一个空白窗口。
    let univer = std::path::Path::new("ui/univer/index.html");
    println!(
        "cargo:rustc-env=TABLET_UNIVER_ASSETS={}",
        if univer.is_file() { "1" } else { "0" }
    );
    println!("cargo:rerun-if-changed=ui/univer/index.html");

    tauri_build::build()
}
