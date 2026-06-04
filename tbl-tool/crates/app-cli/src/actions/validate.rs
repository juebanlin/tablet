//! 校验编排：跑 `revalidate_all` 后把 `validation_errors` 复制成 [`ValidationSummary`]。
//!
//! 不 process::exit；CLI 调用方自己根据 `is_pass()` 决定 exit code。

use tbl_core::ops::ProjectEngine;

/// 一条校验错误：(project_id, group, name, row, col)。
pub type ValidationError = (String, String, String, usize, usize);

#[derive(Debug, Default)]
pub struct ValidationSummary {
    pub errors: Vec<ValidationError>,
}

impl ValidationSummary {
    pub fn is_pass(&self) -> bool { self.errors.is_empty() }
    pub fn error_count(&self) -> usize { self.errors.len() }
}

pub fn run_validate(engine: &mut ProjectEngine) -> ValidationSummary {
    engine.revalidate_all();
    ValidationSummary {
        errors: engine.validation_errors.iter().cloned().collect(),
    }
}
