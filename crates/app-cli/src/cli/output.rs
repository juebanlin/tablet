//! CLI 屏幕输出：把 actions 层的 Summary 翻译成 stdout / stderr。
//!
//! 全部函数后缀 `_cli`，强提示"仅 CLI 二进制使用，GUI 别调"。

use crate::actions::export::{ExportSummary, FormatOutcome};
use crate::actions::excel::{ExcelExportSummary, ExcelTarget};
use crate::actions::list_templates::TemplateList;
use crate::actions::overrides::{OverrideOutcome, OverrideWarning};
use crate::actions::project_info::ProjectInfoSummary;
use crate::actions::validate::ValidationSummary;
use tablet_core::export::{ExportResult, FileStatus};
use tablet_core::project::ProjectListEntry;
use tablet_core::template::TemplateMeta;

pub fn print_export_summary_cli(summary: &ExportSummary) {
    for (label, outcome) in &summary.per_format {
        match outcome {
            FormatOutcome::Ok(r) => print_export_result_cli(label, r),
            FormatOutcome::Err(msg) => eprintln!("[{}] 错误: {}", label, msg),
        }
    }
}

fn print_export_result_cli(label: &str, result: &ExportResult) {
    println!("[{}] {} 新增, {} 修改, {} 删除, {} 不变",
        label, result.added(), result.modified(), result.deleted(), result.unchanged());
    for f in &result.files {
        match f.status {
            FileStatus::Added => println!("  [新增] {}", f.path),
            FileStatus::Modified => println!("  [修改] {}", f.path),
            FileStatus::Deleted => println!("  [删除] {}", f.path),
            FileStatus::Unchanged => {}
        }
    }
}

pub fn print_validate_summary_cli(summary: &ValidationSummary) {
    if summary.is_pass() {
        println!("验证通过，无错误");
    } else {
        println!("发现 {} 个验证错误:", summary.error_count());
        for (project_id, group, name, row, col) in &summary.errors {
            println!("  [{}] {}/{} [{},{}]", project_id, group, name, row, col);
        }
    }
}

pub fn print_template_list_cli(list: &TemplateList) {
    println!("== 内置模板 ==");
    for m in &list.builtin {
        print_template_meta_cli(m);
    }
    if !list.local.is_empty() {
        println!();
        println!("== 本地模板（{}）==", list.local_root.display());
        for m in &list.local {
            print_template_meta_cli(m);
        }
    }
}

fn print_template_meta_cli(m: &TemplateMeta) {
    let category = if m.category.is_empty() { "-" } else { m.category.as_str() };
    let version = if m.version.is_empty() { "-" } else { m.version.as_str() };
    println!("  {:<16} {:<24} category={:<8} version={}", m.id, m.name, category, version);
}

pub fn print_project_list_cli(projects: &[ProjectListEntry]) {
    if projects.is_empty() {
        println!("(无 Project；可用 `tablet-cli new-project --template full --id default --name 默认项目` 创建)");
        return;
    }
    for p in projects {
        println!("{:<24} {}", p.id, p.name);
    }
}

pub fn print_migrate_outcome_cli(migrated: bool) {
    if migrated {
        println!("已迁移老 config/ 到 projects/default/");
    } else {
        println!("无需迁移：projects/ 已存在或老 config/ 不存在");
    }
}

pub fn print_new_project_outcome_cli(project_root: &std::path::Path) {
    println!("已创建 Project: {}", project_root.display());
}

pub fn print_excel_export_summary_cli(s: &ExcelExportSummary) {
    let label = match &s.target {
        ExcelTarget::Group { name, include } if include.is_empty() => format!("分组 {}", name),
        ExcelTarget::Group { name, include } => {
            format!("分组 {}（{} 个子节点）", name, include.len())
        }
    };
    println!("[Excel] {} → {} ({} bytes)", label, s.output.display(), s.bytes);
}

pub fn print_override_warnings_cli(out: &OverrideOutcome) {
    for w in &out.warnings {
        match w {
            OverrideWarning::InvalidFormat(s) => {
                eprintln!("警告: 无效的覆盖参数 '{}', 格式应为 key=value", s);
            }
            OverrideWarning::Deprecated { hint, .. } => {
                eprintln!("警告: {}", hint);
            }
            OverrideWarning::Unknown(key) => {
                eprintln!("警告: 未知配置项 '{}'", key);
            }
        }
    }
}

pub fn print_project_info_cli(s: &ProjectInfoSummary) {
    println!("Project: {} ({})", s.id, s.name);
    println!("  Groups:    {}", s.groups);
    println!("  Tables:    {}", s.tables);
    println!("  Constants: {}", s.constants);
    println!("  Enums:     {}", s.enums);
    println!("  总数据行:  {}", s.total_rows);
    println!("  Dirty:     {}", if s.dirty { "是" } else { "否" });
}

pub fn print_sep_show_cli(s: &crate::actions::sep::SepShowSummary) {
    println!("分隔符配置（合并后生效值）:");
    for e in &s.entries {
        println!("  {:<22}= {:<10}[{}]", e.key, e.value, e.source);
    }
}

pub fn print_schema_show_cli(entries: &[crate::actions::schema::SchemaShowEntry]) {
    if entries.is_empty() {
        println!("(空项目，无 Group)");
        return;
    }
    for (gi, g) in entries.iter().enumerate() {
        println!("{}/", g.group);
        for (ni, n) in g.nodes.iter().enumerate() {
            let is_last = ni == g.nodes.len() - 1;
            let prefix = if is_last { "  └── " } else { "  ├── " };
            println!("{}{:<16}({}, {})", prefix, n.name, n.kind, n.detail);
        }
        if gi < entries.len() - 1 {
            println!();
        }
    }
}
