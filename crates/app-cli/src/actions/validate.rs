//! 校验编排：跑 `revalidate_all` 后把校验结果整理成 [`ValidationSummary`]。
//!
//! 支持五级粒度过滤：project → group → node → col → cell。
//! 不 process::exit；CLI 调用方自己根据 `is_pass()` 决定 exit code。

use tablet_core::ops::{format_validation_log, ProjectEngine};

/// 一条校验错误。
///
/// `display` 与 GUI 日志同一套渲染（`tablet_core::ops::format_validation_log`），
/// 含「表头第 N 行 F 列」这类定位——表头层错误的 `row` 是 `SCHEMA_ROW` 哨兵值，
/// 直接打出来是 `18446744073709551615`，人没法看。
#[derive(Debug, serde::Serialize)]
pub struct ValidationIssue {
    pub project_id: String,
    pub group: String,
    pub node: String,
    /// 表头层错误为 `SCHEMA_ROW`（`usize::MAX`）哨兵；判断用 `display` 更可靠。
    pub row: usize,
    pub col: usize,
    pub message: String,
    /// 渲染好的定位 + 消息，CLI 直接打印这一行。
    pub display: String,
}

#[derive(Debug, Default, serde::Serialize)]
pub struct ValidationSummary {
    pub errors: Vec<ValidationIssue>,
}

impl ValidationSummary {
    pub fn is_pass(&self) -> bool { self.errors.is_empty() }
    pub fn error_count(&self) -> usize { self.errors.len() }
}

pub struct ValidateFilter {
    pub group: Option<String>,
    pub node: Option<String>,
    pub col: Option<u16>,
    pub row: Option<u32>,
}

pub fn run_validate(engine: &mut ProjectEngine) -> ValidationSummary {
    run_validate_filtered(engine, &ValidateFilter { group: None, node: None, col: None, row: None })
}

pub fn run_validate_filtered(engine: &mut ProjectEngine, filter: &ValidateFilter) -> ValidationSummary {
    // revalidate_all 仍要跑：它刷新引擎里的错误索引，供 GUI 的红框 / `!` 聚合用。
    engine.revalidate_all();
    let project_id = engine.active_project_id().unwrap_or_default().to_string();

    // 用 validate_errors 而不是错误索引——索引里只有位置，没有消息与表头行号
    let errors = engine.validate_errors().into_iter()
        .filter(|(_group, node, err)| {
            if let Some(ref fg) = filter.group {
                if _group != fg { return false; }
            }
            if let Some(ref fn_) = filter.node {
                if node != fn_ { return false; }
            }
            if let Some(fc) = filter.col {
                if err.col != fc as usize { return false; }
            }
            if let Some(fr) = filter.row {
                if err.row != fr as usize { return false; }
            }
            true
        })
        .map(|(group, node, err)| ValidationIssue {
            project_id: project_id.clone(),
            display: format_validation_log(&group, &node, &err),
            group,
            node,
            row: err.row,
            col: err.col,
            message: err.message,
        })
        .collect();

    ValidationSummary { errors }
}
