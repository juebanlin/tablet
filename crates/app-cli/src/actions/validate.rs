//! 校验编排：跑 `revalidate_all` 后把校验结果整理成 [`ValidationSummary`]。
//!
//! 支持五级粒度过滤：project → group → node → col → cell。
//! 不 process::exit；CLI 调用方自己根据 `is_pass()` 决定 exit code。

use tablet_core::model::SeverityAction;
use tablet_core::ops::{format_validation_log, ProjectEngine};

/// 过滤目标写错了必须吵。
///
/// `validate --group typoo` 以前会得到"验证通过，无错误" + exit 0 —— 空集恒真，
/// 而调用它的脚本无从分辨"这个组没问题"和"这个组根本不存在"。
/// `export data --group` 同理（导出 0 个文件，照样 exit 0）。
pub fn ensure_filter_targets(
    engine: &ProjectEngine,
    group: Option<&str>,
    node: Option<&str>,
) -> anyhow::Result<()> {
    let Some(project) = engine.active() else {
        anyhow::bail!("没有活跃的 Project");
    };
    let Some(g) = group else { return Ok(()) };
    let Some(grp) = project.groups.iter().find(|x| x.name == g) else {
        anyhow::bail!("Group 不存在: {}", g);
    };
    if let Some(n) = node {
        let ok = grp.tables.iter().any(|t| t.name == n)
            || grp.constants.iter().any(|c| c.name == n)
            || grp.enums.iter().any(|e| e.name == n);
        if !ok {
            anyhow::bail!("节点不存在: {}/{}", g, n);
        }
    }
    Ok(())
}

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
    /// `error` / `warning`（@01.10.7）。
    pub severity: String,
}

#[derive(Debug, Default, serde::Serialize)]
pub struct ValidationSummary {
    pub errors: Vec<ValidationIssue>,
    pub warnings: Vec<ValidationIssue>,
}

impl ValidationSummary {
    /// 通过 = 没有错误。警告不构成失败（`--strict` 才把警告也算上）。
    pub fn is_pass(&self) -> bool { self.errors.is_empty() }
    pub fn is_pass_strict(&self) -> bool {
        self.errors.is_empty() && self.warnings.is_empty()
    }
    pub fn error_count(&self) -> usize { self.errors.len() }
    pub fn warning_count(&self) -> usize { self.warnings.len() }
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
    // 级别 → 动作由项目级「验证设置」决定（@01.10.7）；Ignore 的级别已被引擎过滤掉。
    let policy = engine.validate_policy();

    // 用 validate_errors 而不是错误索引——索引里只有位置，没有消息与表头行号
    let issues = engine.validate_errors().into_iter()
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
        .map(|(group, node, err)| {
            let action = policy.action(err.severity);
            let issue = ValidationIssue {
                project_id: project_id.clone(),
                display: format_validation_log(&group, &node, &err),
                group,
                node,
                row: err.row,
                col: err.col,
                severity: match err.severity {
                    tablet_core::model::Severity::Error => "error",
                    tablet_core::model::Severity::Warning => "warning",
                    tablet_core::model::Severity::Info => "info",
                }.to_string(),
                message: err.message,
            };
            (action, issue)
        })
        .collect::<Vec<_>>();

    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    for (action, issue) in issues {
        match action {
            SeverityAction::Block => errors.push(issue),
            SeverityAction::Report => warnings.push(issue),
            SeverityAction::Ignore => {}
        }
    }
    ValidationSummary { errors, warnings }
}
