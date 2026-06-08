//! 业务编排层：GUI / CLI 共用。
//!
//! 这里的函数：
//! - 不打印 stdout / stderr
//! - 不调 `clap` 派生类型
//! - 不调 `process::exit`
//! - 接收强类型参数，返 `Result<某种 Summary>`
//!
//! CLI 把 Summary 翻译成屏幕输出和 exit code（详见 [`crate::cli`]）；
//! GUI 拿 Summary 自行塞进 LogPanel / 弹窗。

pub mod export;
pub mod excel;
pub mod generate_test;
pub mod list_projects;
pub mod list_templates;
pub mod migrate;
pub mod new_project;
pub mod overrides;
pub mod validate;
