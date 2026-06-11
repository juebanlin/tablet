fn main() {
    println!("cargo:rerun-if-changed=../../res/icon.ico");

    // Windows: 嵌入 exe 图标
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../res/icon.ico");
        res.compile().expect("winresource failed");
    }
}
