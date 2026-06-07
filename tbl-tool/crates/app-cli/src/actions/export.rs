//! 导出编排：按 [`ExportFormats`] 选项依次调 `engine.export_*`，
//! 每种格式的成功 / 失败都装进 [`ExportSummary`] 返给调用方。
//!
//! 注意：单种格式失败不会停下其它格式（与原 CLI 行为一致——逐项 print，
//! 一项报错也继续下一项）。调用方按需聚合 / 提示。

use tbl_core::export::ExportResult;
use tbl_core::ops::ProjectEngine;

/// 选哪些格式。`all()` = 啥都没选时的默认（全跑）。
#[derive(Debug, Clone, Copy, Default)]
pub struct ExportFormats {
    pub json: bool,
    pub xml: bool,
    pub java: bool,
    pub go: bool,
    pub lua: bool,
    pub gdscript: bool,
    pub typescript: bool,
    pub cpp: bool,
    pub csharp: bool,
}

impl ExportFormats {
    pub fn all() -> Self {
        Self {
            json: true, xml: true, java: true, go: true,
            lua: true, gdscript: true, typescript: true, cpp: true,
            csharp: true,
        }
    }

    /// 任一 flag 为真 = 用户显式指定子集；全 false = 走 all() 兜底。
    pub fn any(&self) -> bool {
        self.json || self.xml || self.java || self.go
            || self.lua || self.gdscript || self.typescript || self.cpp
            || self.csharp
    }
}

/// 单个格式的导出结果（成功或失败）。
#[derive(Debug)]
pub enum FormatOutcome {
    Ok(ExportResult),
    Err(String),
}

/// 一次 export 的总结：每种实际跑过的格式各一条。
#[derive(Debug, Default)]
pub struct ExportSummary {
    pub per_format: Vec<(&'static str, FormatOutcome)>,
}

impl ExportSummary {
    pub fn has_error(&self) -> bool {
        self.per_format.iter().any(|(_, o)| matches!(o, FormatOutcome::Err(_)))
    }
}

/// 按 formats 的 flag 跑导出；空选集 = 全跑。
pub fn run_export(engine: &mut ProjectEngine, formats: ExportFormats) -> ExportSummary {
    let formats = if formats.any() { formats } else { ExportFormats::all() };
    let mut summary = ExportSummary::default();

    if formats.json {
        summary.per_format.push(("JSON", outcome(engine.export_json())));
    }
    if formats.xml {
        summary.per_format.push(("XML", outcome(engine.export_xml())));
    }
    if formats.java {
        summary.per_format.push(("Java", outcome(engine.export_java())));
    }
    if formats.go {
        summary.per_format.push(("Go", outcome(engine.export_go())));
    }
    if formats.lua {
        summary.per_format.push(("Lua", outcome(engine.export_lua())));
    }
    if formats.gdscript {
        summary.per_format.push(("GDScript", outcome(engine.export_gdscript())));
    }
    if formats.typescript {
        summary.per_format.push(("TypeScript", outcome(engine.export_typescript())));
    }
    if formats.cpp {
        summary.per_format.push(("C++", outcome(engine.export_cpp())));
    }
    if formats.csharp {
        summary.per_format.push(("C# (.NET)", outcome(engine.export_csharp_dotnet())));
        summary.per_format.push(("C# (Unity)", outcome(engine.export_csharp_unity())));
        summary.per_format.push(("C# (Godot)", outcome(engine.export_csharp_godot())));
    }
    summary
}

fn outcome(r: anyhow::Result<ExportResult>) -> FormatOutcome {
    match r {
        Ok(r) => FormatOutcome::Ok(r),
        Err(e) => FormatOutcome::Err(format!("{}", e)),
    }
}
