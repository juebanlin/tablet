//! CLI 顶层 dispatcher：解析 argv → 调 [`crate::actions`] → [`crate::cli::output`] 打印
//! → 翻译 exit code。
//!
//! [`run_with_args`] 也被 `tablet-slint.exe` 在"有 CLI 子命令"时直接调，
//! 等价于跑了一次 `tablet-cli.exe` 子进程。
//!
//! exit code 约定：
//! - 验证不通过 → 1
//! - 其它一切正常 → 0
//! - I/O / 解析 / 加载等真错误 → 抛 `Err`，由 main.rs 翻成 exit 1。

use anyhow::Result;
use clap::Parser;
use tablet_core::ops::ProjectEngine;
use tablet_core::project::{load_project, load_specific_project, load_workspace};

use super::args::*;
use super::output;
use crate::actions::{
    excel::{run_excel_export, ExcelTarget},
    export::{run_export, ExportFormats},
    list_projects::list_projects,
    list_templates::list_templates,
    migrate::run_migrate,
    new_project::run_new_project,
    overrides::apply_overrides,
    project_clone::run_project_clone,
    project_delete::run_project_delete,
    project_info::run_project_info,
    project_rename::run_project_rename,
    validate::run_validate_filtered,
};

/// 入口：拿一份 argv（含 program name），解析并执行；返 exit code。
pub fn run_with_args(args: &[String]) -> Result<i32> {
    let cli = Cli::try_parse_from(args)?;
    run(cli)
}

fn run(cli: Cli) -> Result<i32> {
    match &cli.command {
        // 不需要加载 Project 的命令
        Command::ListTemplates => {
            output::print_template_list_cli(&list_templates());
            return Ok(0);
        }
        Command::MigrateLegacy => {
            let migrated = run_migrate(&cli.workdir)?;
            output::print_migrate_outcome_cli(migrated);
            return Ok(0);
        }
        Command::Project(pa) => return dispatch_project(&cli, pa),
        Command::Util(ua) => {
            return match &ua.action {
                UtilAction::ParseTbl { file } => {
                    let out = crate::actions::util::run_parse_tbl(file)?;
                    println!("{}", out);
                    Ok(0)
                }
                UtilAction::ParseSchema { file } => {
                    let out = crate::actions::util::run_parse_schema(file)?;
                    println!("{}", out);
                    Ok(0)
                }
                UtilAction::MergeSchema { files } => {
                    let out = crate::actions::util::run_merge_schema(files)?;
                    println!("{}", out);
                    Ok(0)
                }
                UtilAction::ValidateTbl { file, sep_opts } => {
                    let errors = crate::actions::util::run_validate_tbl(
                        file,
                        sep_opts.config.as_deref(),
                        sep_opts.schema.as_deref(),
                        &sep_opts.sep_overrides,
                    )?;
                    if errors.is_empty() {
                        println!("验证通过");
                        Ok(0)
                    } else {
                        for e in &errors {
                            println!("  {}", e);
                        }
                        println!("发现 {} 个错误", errors.len());
                        Ok(1)
                    }
                }
                UtilAction::ValidateType { r#type, value, sep_opts } => {
                    let result = crate::actions::util::run_validate_type(
                        r#type,
                        value,
                        sep_opts.config.as_deref(),
                        sep_opts.schema.as_deref(),
                        &sep_opts.sep_overrides,
                    )?;
                    match result {
                        None => Ok(0),
                        Some(msg) => {
                            eprintln!("错误: {}", msg);
                            Ok(1)
                        }
                    }
                }
                UtilAction::TblToXlsx { file, output } => {
                    crate::actions::util::run_tbl_to_xlsx(file, output)?;
                    println!("已转换: {} → {}", file.display(), output.display());
                    Ok(0)
                }
                UtilAction::XlsxToTbl { file, schema, output } => {
                    let count = crate::actions::util::run_xlsx_to_tbl(file, schema, output)?;
                    println!("已转换: {} → {} ({} 个文件)", file.display(), output.display(), count);
                    Ok(0)
                }
                UtilAction::Scaffold { file, output } => {
                    crate::actions::util::run_scaffold(file, output)?;
                    println!("已生成骨架: {}", output.display());
                    Ok(0)
                }
                UtilAction::Diff { a, b } => {
                    let out = crate::actions::util::run_diff(a, b)?;
                    print!("{}", out);
                    Ok(0)
                }
                UtilAction::Fmt { path, in_place } => {
                    match crate::actions::util::run_fmt(path, *in_place)? {
                        Some(content) => print!("{}", content),
                        None => println!("已格式化: {}", path.display()),
                    }
                    Ok(0)
                }
                UtilAction::Stat { path } => {
                    let out = crate::actions::util::run_stat(path)?;
                    print!("{}", out);
                    Ok(0)
                }
                UtilAction::GenTest { lang, format, schema, output, package, code_output } => {
                    crate::actions::util::run_gen_test(lang, format, schema, output, package, code_output)?;
                    println!("已生成测试代码: {}", output.display());
                    Ok(0)
                }
            };
        }
        _ => {}
    }

    // 以下命令需要先加载 Project
    let project = match cli.project.as_deref() {
        Some(pid) => load_specific_project(&cli.workdir, pid)?,
        None => load_project(&cli.workdir)?,
    };
    let mut engine = ProjectEngine::new(project);

    let warns = apply_overrides(&mut engine, &cli.overrides);
    output::print_override_warnings_cli(&warns);

    match cli.command {
        Command::Export(ref ea) => {
            match &ea.action {
                ExportAction::All { output } => {
                    if let Some(o) = output {
                        apply_output_root(&mut engine, o);
                    }
                    let summary = run_export(&mut engine, ExportFormats::all());
                    if cli.is_json() {
                        println!("{}", summary.to_json_value());
                    } else {
                        output::print_export_summary_cli(&summary);
                    }
                    Ok(0)
                }
                ExportAction::Data { json, xml, group, node, output } => {
                    if let Some(o) = output {
                        apply_data_output(&mut engine, o);
                    }
                    let summary = crate::actions::export::run_export_data(
                        &mut engine,
                        *json,
                        *xml,
                        group.as_deref(),
                        node.as_deref(),
                    );
                    if cli.is_json() {
                        println!("{}", summary.to_json_value());
                    } else {
                        output::print_export_summary_cli(&summary);
                    }
                    Ok(0)
                }
                ExportAction::Code {
                    java, go, lua, gdscript, typescript, cpp, csharp, all,
                    package, namespace, output,
                } => {
                    if let Some(pkg) = package {
                        apply_package_override(&mut engine, pkg);
                    }
                    if let Some(ns) = namespace {
                        apply_namespace_override(&mut engine, ns);
                    }
                    if let Some(o) = output {
                        apply_code_output(&mut engine, o);
                    }
                    let mut formats = ExportFormats::default();
                    if *all {
                        formats = ExportFormats::all();
                        formats.json = false;
                        formats.xml = false;
                    } else {
                        if *java { formats.java = true; }
                        if *go { formats.go = true; }
                        if *lua { formats.lua = true; }
                        if *gdscript { formats.gdscript = true; }
                        if *typescript { formats.typescript = true; }
                        if *cpp { formats.cpp = true; }
                        if *csharp { formats.csharp = true; }
                    }
                    if !formats.any() {
                        eprintln!("错误: 请指定至少一种语言或使用 --all");
                        return Ok(1);
                    }
                    let summary = run_export(&mut engine, formats);
                    if cli.is_json() {
                        println!("{}", summary.to_json_value());
                    } else {
                        output::print_export_summary_cli(&summary);
                    }
                    Ok(0)
                }
            }
        }
        Command::Validate { ref group, ref node, col, row } => {
            let filter = crate::actions::validate::ValidateFilter {
                group: group.clone(), node: node.clone(), col, row,
            };
            let summary = run_validate_filtered(&mut engine, &filter);
            if cli.is_json() {
                println!("{}", serde_json::to_string_pretty(&summary).unwrap_or_default());
            } else {
                output::print_validate_summary_cli(&summary);
            }
            Ok(if summary.is_pass() { 0 } else { 1 })
        }
        Command::Excel(ea) => match ea.action {
            ExcelAction::Export { group, include, output: out } => {
                let target = ExcelTarget::Group { name: group, include };
                let summary = run_excel_export(&engine, target, out.as_deref())?;
                output::print_excel_export_summary_cli(&summary);
                Ok(0)
            }
            ExcelAction::Import { group, file } => {
                let summary = crate::actions::excel::run_excel_import(&mut engine, &group, &file)?;
                println!("[Excel] 已导入 {} ({} 个节点更新)", summary.group, summary.nodes_patched);
                Ok(0)
            }
        },
        Command::Schema(ref sa) => {
            match &sa.action {
                SchemaAction::Show => {
                    let entries = crate::actions::schema::run_schema_show(&engine)?;
                    if cli.is_json() {
                        println!("{}", serde_json::to_string_pretty(&entries).unwrap_or_default());
                    } else {
                        output::print_schema_show_cli(&entries);
                    }
                    Ok(0)
                }
                SchemaAction::AddGroup { name } => {
                    crate::actions::schema::run_schema_add_group(&mut engine, name)?;
                    println!("已添加 Group: {}", name);
                    Ok(0)
                }
                SchemaAction::AddTable { group, name } => {
                    crate::actions::schema::run_schema_add_table(&mut engine, group, name)?;
                    println!("已添加 Table: {}/{}", group, name);
                    Ok(0)
                }
                SchemaAction::AddConstant { group, name } => {
                    crate::actions::schema::run_schema_add_constant(&mut engine, group, name)?;
                    println!("已添加 Constant: {}/{}", group, name);
                    Ok(0)
                }
                SchemaAction::AddEnum { group, name } => {
                    crate::actions::schema::run_schema_add_enum(&mut engine, group, name)?;
                    println!("已添加 Enum: {}/{}", group, name);
                    Ok(0)
                }
                SchemaAction::RenameGroup { old, new } => {
                    crate::actions::schema::run_schema_rename_group(&mut engine, old, new)?;
                    println!("已重命名 Group: {} → {}", old, new);
                    Ok(0)
                }
                SchemaAction::RenameNode { group, old, new } => {
                    crate::actions::schema::run_schema_rename_node(&mut engine, group, old, new)?;
                    println!("已重命名: {}/{} → {}/{}", group, old, group, new);
                    Ok(0)
                }
                SchemaAction::DeleteGroup { name } => {
                    crate::actions::schema::run_schema_delete_group(&mut engine, name)?;
                    println!("已删除 Group: {}", name);
                    Ok(0)
                }
                SchemaAction::DeleteNode { group, name } => {
                    crate::actions::schema::run_schema_delete_node(&mut engine, group, name)?;
                    println!("已删除: {}/{}", group, name);
                    Ok(0)
                }
            }
        }
        Command::Workspace(wa) => {
            match &wa.action {
                WorkspaceAction::Save => {
                    crate::actions::workspace::run_workspace_save(&mut engine)?;
                    println!("已保存");
                    Ok(0)
                }
                WorkspaceAction::Reload => {
                    crate::actions::workspace::run_workspace_reload(&mut engine)?;
                    println!("已重新加载");
                    Ok(0)
                }
                WorkspaceAction::Clear { confirm } => {
                    if !confirm {
                        eprintln!("错误: 此操作将删除所有数据文件，请添加 --confirm 确认");
                        return Ok(1);
                    }
                    crate::actions::workspace::run_workspace_clear(&mut engine)?;
                    println!("已清空所有数据文件");
                    Ok(0)
                }
            }
        }
        Command::Sep(ref sa) => {
            match &sa.action {
                SepAction::Show { defaults, config, schema } => {
                    let summary = crate::actions::sep::run_sep_show(
                        *defaults,
                        config.as_deref(),
                        schema.as_deref(),
                    )?;
                    if cli.is_json() {
                        println!("{}", serde_json::to_string_pretty(&summary).unwrap_or_default());
                    } else {
                        output::print_sep_show_cli(&summary);
                    }
                    Ok(0)
                }
            }
        }
        // 已在上方 dispatch
        Command::ListTemplates | Command::MigrateLegacy
        | Command::Project(_) | Command::Util(_) => {
            unreachable!()
        }
    }
}

fn dispatch_project(cli: &Cli, pa: &ProjectArgs) -> Result<i32> {
    match &pa.action {
        ProjectAction::List => {
            let projects = list_projects(&cli.workdir);
            if cli.is_json() {
                let val: Vec<serde_json::Value> = projects.iter().map(|p| {
                    serde_json::json!({"id": p.id, "name": p.name})
                }).collect();
                println!("{}", serde_json::to_string_pretty(&val).unwrap_or_default());
            } else {
                output::print_project_list_cli(&projects);
            }
            Ok(0)
        }
        ProjectAction::New { template, id, name, switch_after } => {
            let outcome = run_new_project(
                &cli.workdir, template, id, name.as_deref(), *switch_after,
            )?;
            output::print_new_project_outcome_cli(&outcome.project_root);
            Ok(0)
        }
        ProjectAction::Info { id } => {
            let mut engine = load_workspace(&cli.workdir)?;
            if let Some(pid) = cli.project.as_deref().or(id.as_deref()) {
                engine.set_active_by_id(pid);
            }
            let summary = run_project_info(&engine, id.as_deref())?;
            if cli.is_json() {
                println!("{}", serde_json::to_string_pretty(&summary).unwrap_or_default());
            } else {
                output::print_project_info_cli(&summary);
            }
            Ok(0)
        }
        ProjectAction::Rename { id, new_id, new_name } => {
            let mut engine = load_workspace(&cli.workdir)?;
            run_project_rename(&mut engine, id, new_id.as_deref(), new_name.as_deref())?;
            println!("已重命名 Project: {}", new_id.as_deref().unwrap_or(id));
            Ok(0)
        }
        ProjectAction::Delete { id, confirm } => {
            if !confirm {
                eprintln!("错误: 删除操作不可逆，请添加 --confirm 确认");
                return Ok(1);
            }
            let mut engine = load_workspace(&cli.workdir)?;
            run_project_delete(&mut engine, id)?;
            println!("已删除 Project: {}", id);
            Ok(0)
        }
        ProjectAction::Clone { source, id, name } => {
            let mut engine = load_workspace(&cli.workdir)?;
            run_project_clone(&mut engine, source, id, name.as_deref())?;
            println!("已克隆 Project: {} → {}", source, id);
            Ok(0)
        }
    }
}

fn apply_output_root(engine: &mut ProjectEngine, path: &std::path::Path) {
    let p = path.to_string_lossy().to_string();
    let overrides = vec![
        format!("export.server.data_output={}/server/data", p),
        format!("export.server.java.code_output={}/server/java", p),
        format!("export.server.go.code_output={}/server/go", p),
        format!("export.server.cpp.code_output={}/server/cpp", p),
        format!("export.server.csharp_dotnet.code_output={}/server/csharp", p),
        format!("export.server.typescript.output={}/server/typescript", p),
        format!("export.client.lua.output={}/client/lua", p),
        format!("export.client.gdscript.output={}/client/gdscript", p),
        format!("export.client.typescript.output={}/client/typescript", p),
        format!("export.client.csharp_unity.code_output={}/client/csharp_unity", p),
        format!("export.client.csharp_godot.code_output={}/client/csharp_godot", p),
    ];
    crate::actions::overrides::apply_overrides(engine, &overrides);
}

fn apply_data_output(engine: &mut ProjectEngine, path: &std::path::Path) {
    let overrides = vec![
        format!("export.server.data_output={}", path.to_string_lossy()),
    ];
    crate::actions::overrides::apply_overrides(engine, &overrides);
}

fn apply_code_output(engine: &mut ProjectEngine, path: &std::path::Path) {
    let p = path.to_string_lossy().to_string();
    let overrides = vec![
        format!("export.server.java.code_output={}", p),
        format!("export.server.go.code_output={}", p),
        format!("export.server.cpp.code_output={}", p),
        format!("export.server.csharp_dotnet.code_output={}", p),
        format!("export.server.typescript.output={}", p),
        format!("export.client.lua.output={}", p),
        format!("export.client.gdscript.output={}", p),
        format!("export.client.typescript.output={}", p),
        format!("export.client.csharp_unity.code_output={}", p),
        format!("export.client.csharp_godot.code_output={}", p),
    ];
    crate::actions::overrides::apply_overrides(engine, &overrides);
}

fn apply_package_override(engine: &mut ProjectEngine, pkg: &str) {
    let overrides = vec![
        format!("export.server.java.package={}", pkg),
        format!("export.server.go.package={}", pkg),
    ];
    crate::actions::overrides::apply_overrides(engine, &overrides);
}

fn apply_namespace_override(engine: &mut ProjectEngine, ns: &str) {
    let overrides = vec![
        format!("export.server.cpp.namespace={}", ns),
        format!("export.server.csharp_dotnet.namespace={}", ns),
        format!("export.client.csharp_unity.namespace={}", ns),
        format!("export.client.csharp_godot.namespace={}", ns),
    ];
    crate::actions::overrides::apply_overrides(engine, &overrides);
}
