//! tbl-cli lib：把 CLI 命令处理拆成两层。
//!
//! - [`actions`]：业务编排层，无 println / 无 clap / 无 process::exit。
//!   GUI 想"按钮 = 跑一次完整导出"直接调这一层；签名干净，返 `Result<某种 Summary>`。
//! - [`cli`]：仅 CLI 二进制用：clap 派生类型、stdout 打印、exit code 翻译。
//!   GUI 不该引用本模块。
//!
//! 顶层 re-export `run_with_args`，方便 `tbl-slint.exe` 在 CLI 模式下一句调过去。
//!
//! 模块路径就是契约：`tbl_cli::actions::*` = 可复用，`tbl_cli::cli::*` 或带 `_cli`
//! 后缀的函数 = CLI 专用。

pub mod actions;
pub mod cli;

pub use cli::dispatcher::run_with_args;
