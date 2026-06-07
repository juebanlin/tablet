//! CLI 专用模块：clap 派生类型 / stdout 打印 / exit code 翻译。
//!
//! 不应被 GUI 引用。GUI 想复用业务逻辑请直接调 [`crate::actions`]。

pub mod args;
pub mod dispatcher;
pub mod output;
