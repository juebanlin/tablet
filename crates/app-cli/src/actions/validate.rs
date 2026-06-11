//! 校验编排：跑 `revalidate_all` 后把 `validation_errors` 复制成 [`ValidationSummary`]。
//!
//! 支持五级粒度过滤：project → group → node → col → cell。
//! 不 process::exit；CLI 调用方自己根据 `is_pass()` 决定 exit code。

use tablet_core::ops::ProjectEngine;

/// 一条校验错误：(project_id, group, name, row, col)。
pub type ValidationError = (String, String, String, usize, usize);

#[derive(Debug, Default, serde::Serialize)]
pub struct ValidationSummary {
    pub errors: Vec<ValidationError>,
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
    engine.revalidate_all();
    let errors: Vec<ValidationError> = engine.validation_errors.iter()
        .filter(|(_pid, group, name, row, col)| {
            if let Some(ref fg) = filter.group {
                if group != fg { return false; }
            }
            if let Some(ref fn_) = filter.node {
                if name != fn_ { return false; }
            }
            if let Some(fc) = filter.col {
                if *col != fc as usize { return false; }
            }
            if let Some(fr) = filter.row {
                if *row != fr as usize { return false; }
            }
            true
        })
        .cloned()
        .collect();
    ValidationSummary { errors }
}
