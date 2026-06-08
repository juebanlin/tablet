// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 juebanlin <juebanlin@gmail.com>

pub mod model;
pub mod types;
pub mod tbl;
pub mod tblschema;
pub mod template;
pub mod project;
pub mod validate;
pub mod ops;
pub mod export;
pub mod excel;
pub mod search;
pub mod test_util;

pub use search::name_matches;

pub const CONFIG_FILE: &str = "tablet.toml";
pub const LOCK_FILE: &str = ".tablet.lock";
pub const LOG_FILE: &str = "tablet.log";

/// 旧版本写出过的工作区配置文件名。启动时如果 CONFIG_FILE 不存在但旧文件存在，自动 rename 一次性迁移。
pub const LEGACY_CONFIG_FILE: &str = "tbl-tool.toml";
