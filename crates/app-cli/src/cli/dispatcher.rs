//! CLI 顶层 dispatcher：解析 argv → 调 [`crate::actions`] → [`crate::cli::output`] 打印
//! → 翻译 exit code。
//!
//! [`run_with_args`] 也被 `tablet-slint.exe` 在"有 CLI 子命令"时直接调，
//! 等价于跑了一次 `tablet-cli.exe` 子进程。
//!
//! exit code 约定（与改造前完全一致，保 Jenkins 脚本兼容）：
//! - 验证不通过 → 1
//! - 其它一切正常 → 0
//! - I/O / 解析 / 加载等真错误 → 抛 `Err`，由 main.rs 翻成 exit 1。

use anyhow::Result;
use clap::Parser;
use tablet_core::ops::ProjectEngine;
use tablet_core::project::{load_project, load_specific_project};

use super::args::{Cli, Command, ExcelAction};
use super::output;
use crate::actions::{
    excel::{run_excel_export, ExcelTarget},
    export::{run_export, ExportFormats, ExportSummary},
    generate_test::{run_generate_test, GenerateTestOptions},
    list_projects::list_projects,
    list_templates::list_templates,
    migrate::run_migrate,
    new_project::run_new_project,
    overrides::apply_overrides,
    validate::run_validate,
};

/// 入口：拿一份 argv（含 program name），解析并执行；返 exit code。
pub fn run_with_args(args: &[String]) -> Result<i32> {
    let cli = Cli::try_parse_from(args)?;
    run(cli)
}

fn run(cli: Cli) -> Result<i32> {
    // 这些子命令不需要先加载 Project
    match &cli.command {
        Command::ListTemplates => {
            output::print_template_list_cli(&list_templates());
            return Ok(0);
        }
        Command::MigrateLegacy => {
            let migrated = run_migrate(&cli.workdir)?;
            output::print_migrate_outcome_cli(migrated);
            return Ok(0);
        }
        Command::ListProjects => {
            let projects = list_projects(&cli.workdir);
            output::print_project_list_cli(&projects);
            return Ok(0);
        }
        Command::NewProject { template, id, name, switch_after } => {
            let outcome = run_new_project(
                &cli.workdir, template, id, name.as_deref(), *switch_after,
            )?;
            output::print_new_project_outcome_cli(&outcome.project_root);
            return Ok(0);
        }
        _ => {}
    }

    let project = match cli.project.as_deref() {
        Some(pid) => load_specific_project(&cli.workdir, pid)?,
        None => load_project(&cli.workdir)?,
    };
    let mut engine = ProjectEngine::new(project);

    let warns = apply_overrides(&mut engine, &cli.overrides);
    output::print_override_warnings_cli(&warns);

    match cli.command {
        Command::Export { json, xml, java, go, lua, gdscript, typescript, cpp, csharp } => {
            let summary: ExportSummary = run_export(
                &mut engine,
                ExportFormats { json, xml, java, go, lua, gdscript, typescript, cpp, csharp },
            );
            output::print_export_summary_cli(&summary);
            // 与原行为一致：单格式失败仅 eprintln，整体仍返 0
            Ok(0)
        }
        Command::Validate => {
            let summary = run_validate(&mut engine);
            output::print_validate_summary_cli(&summary);
            Ok(if summary.is_pass() { 0 } else { 1 })
        }
        Command::GenerateTest { empty, schema, rows, seed, format, lang } => {
            run_generate_test(
                &mut engine,
                &cli.workdir,
                GenerateTestOptions {
                    include_empty: empty,
                    schema, rows, seed, format, lang,
                },
            )?;
            output::print_generate_test_done_cli();
            Ok(0)
        }
        Command::Excel(ea) => match ea.action {
            ExcelAction::Export { group, include, output: out } => {
                let target = ExcelTarget::Group { name: group, include };
                let summary = run_excel_export(&engine, target, out.as_deref())?;
                output::print_excel_export_summary_cli(&summary);
                Ok(0)
            }
        },
        // 顶部已 dispatch
        Command::ListProjects | Command::ListTemplates
        | Command::MigrateLegacy | Command::NewProject { .. } => {
            unreachable!("已在 dispatcher 顶部 dispatch")
        }
    }
}
