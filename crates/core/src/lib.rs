// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 juebanlin <juebanlin@gmail.com>

pub mod model;
pub mod config_leaf;
pub mod export_keys;
pub mod ui_keys;
pub mod types;
pub mod enums;
pub mod tbl;
pub mod tbl_str;
pub mod rules;
pub mod schema_model;
pub mod schema_grammar;
pub mod schema_projection;
pub mod tblschema;
pub mod manifest;
pub mod importer;
pub mod excel_mapping;
pub mod template;
pub mod project;
pub mod validate;
pub mod ops;
pub mod export;
pub mod excel;
pub mod excel_sync;
pub mod search;
pub mod test_util;
pub mod config_fix;
pub mod workspace_lock;
pub mod vcs;
pub mod local_state;
pub mod history;

pub use search::name_matches;

pub const CONFIG_FILE: &str = "tablet.toml";
pub const LOG_FILE: &str = "tablet.log";
/// 崩溃日志：与 LOG_FILE 分开，因为它必须是 append 且不被下次启动截断。
pub const CRASH_LOG_FILE: &str = "tablet-crash.log";

/// 旧版本写出过的工作区配置文件名。启动时如果 CONFIG_FILE 不存在但旧文件存在，自动 rename 一次性迁移。
pub const LEGACY_CONFIG_FILE: &str = "tbl-tool.toml";
