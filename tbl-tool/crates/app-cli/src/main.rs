//! tbl-cli 二进制入口：仅做 argv 转发 + exit code 翻译。
//!
//! 全部命令处理在 [`tbl_cli::cli::dispatcher`]；GUI 复用业务逻辑请直接调
//! `tbl_cli::actions::*`，不要走二进制子进程。

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match tbl_cli::run_with_args(&args) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            // clap 的 parse 错误已经把 usage 打到 stderr；这里只把通用 anyhow 错链印出来
            if let Some(clap_err) = e.downcast_ref::<clap::Error>() {
                clap_err.print().ok();
                std::process::exit(clap_err.exit_code());
            }
            eprintln!("{:#}", e);
            std::process::exit(1);
        }
    }
}
