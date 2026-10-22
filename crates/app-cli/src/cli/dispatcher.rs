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
use tablet_core::model::GlobalConfig;
use tablet_core::project::{load_project, load_specific_project, load_workspace};

use super::args::*;
use super::output;
use crate::actions::{
    excel::{run_excel_export, ExcelTarget},
    export::{run_export, ExportFormats},
    list_projects::list_projects,
    list_templates::list_templates,
    new_project::{run_new_project, NewProjectSource},
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
    // GUI 与 CLI 互斥（@03.7.5）：两边共享同一个 `projects/`，同时写就是丢更新。
    // `util` 是纯文件工具（不碰工作区状态），`list-templates` 只读应用目录，不参与。
    let _workspace_lock = match &cli.command {
        Command::Util(_) | Command::ListTemplates => None,
        _ => Some(tablet_core::workspace_lock::acquire(
            &cli.workdir,
            tablet_core::workspace_lock::ProcKind::Cli,
        )?),
    };

    match &cli.command {
        // 不需要加载 Project 的命令
        Command::ListTemplates => {
            output::print_template_list_cli(&list_templates());
            return Ok(0);
        }
        Command::Project(pa) => return dispatch_project(&cli, pa),
        // sep show 只要给了显式来源（--defaults / --config / --schema）就与项目无关，
        // 不该被「先加载 Project」那步挡住——@03.8.8 明确写了「可带项目上下文也可独立使用」。
        // 只有光秃秃的 `sep show`（读当前项目生效值）才需要项目。
        Command::Sep(sa) => {
            // SepAction 目前只有 Show 一个变体，直接解构（不加 if let，避免 irrefutable 警告）
            let SepAction::Show { defaults, config, schema } = &sa.action;
            {
                if *defaults || config.is_some() || schema.is_some() {
                    let summary = crate::actions::sep::run_sep_show(
                        *defaults,
                        None, // 显式来源就不牵扯项目
                        config.as_deref(),
                        schema.as_deref(),
                    )?;
                    if cli.is_json() {
                        println!("{}", serde_json::to_string_pretty(&summary).unwrap_or_default());
                    } else {
                        output::print_sep_show_cli(&summary);
                    }
                    return Ok(0);
                }
            }
        }
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
                UtilAction::GenTest { lang, format, schema, output, package, code_output, lombok } => {
                    crate::actions::util::run_gen_test(lang, format, schema, output, package, code_output, *lombok)?;
                    println!("已生成测试代码: {}", output.display());
                    Ok(0)
                }
            };
        }
        _ => {}
    }

    // 以下命令需要先加载 Project
    let (project, global_config) = match cli.project.as_deref() {
        Some(pid) => {
            let config_path = cli.workdir.join("tablet.toml");
            let global_text = std::fs::read_to_string(&config_path)?;
            let global_config: GlobalConfig = toml::from_str(&global_text)?;
            let project = load_specific_project(&cli.workdir, pid)?;
            (project, global_config)
        }
        None => {
            let config_path = cli.workdir.join("tablet.toml");
            let global_text = std::fs::read_to_string(&config_path)?;
            let global_config: GlobalConfig = toml::from_str(&global_text)?;
            let project = load_project(&cli.workdir)?;
            (project, global_config)
        }
    };
    let mut engine = ProjectEngine::new(project, global_config);

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
                    // 逐格式失败不打断其它格式，但**退出码必须是失败**：
                    // `export all || exit 1` 这种写法在 CI 里全靠它。
                    Ok(if summary.has_error() { 1 } else { 0 })
                }
                ExportAction::Data { json, xml, group, node, output } => {
                    // 过滤目标不存在 = 用错命令，不是"没数据"。否则脚本会把
                    // "组名写错"当成"这个组导出成功且为空"。
                    crate::actions::validate::ensure_filter_targets(
                        &engine, group.as_deref(), node.as_deref(),
                    )?;
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
                    Ok(if summary.has_error() { 1 } else { 0 })
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
                    Ok(if summary.has_error() { 1 } else { 0 })
                }
            }
        }
        Command::Validate { ref group, ref node, col, row } => {
            crate::actions::validate::ensure_filter_targets(
                &engine, group.as_deref(), node.as_deref(),
            )?;
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
                SchemaAction::AddGroup { name, storage } => {
                    crate::actions::schema::run_schema_add_group(&mut engine, name, storage)?;
                    println!("已添加 Group: {}（{}）", name, storage);
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
                SchemaAction::AddEnum { group, name, entries } => {
                    crate::actions::schema::run_schema_add_enum(&mut engine, group, name, entries)?;
                    println!("已添加 Enum: {}/{}（{} 个条目）", group, name, entries.len());
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
                    // 走到这里说明没给显式来源 → 展示**当前项目**的生效值。
                    // 「当前项目」= `--project` 指定的，否则 last_project（跟其它命令同一套）。
                    let project_id = engine.active_project_id().unwrap_or_default().to_string();
                    let summary = crate::actions::sep::run_sep_show(
                        *defaults,
                        Some(crate::actions::sep::ProjectSep {
                            id: &project_id,
                            separators: &engine.project().schema.separators,
                        }),
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
        Command::ListTemplates | Command::Project(_) | Command::Util(_) => {
            unreachable!()
        }
    }
}

/// 加载工作区，并把「打不开的项目」当成命令失败。
///
/// CLI 主要由 AI / 脚本驱动：**静静少一个项目比报错更难查**（@02.26）。打不开的项目在
/// `load_workspace` 里已经被跳过了（不会进 engine），所以这里只负责吵 + 非零退出 ——
/// `bail!` 经 main.rs 翻成 exit 1。
fn load_workspace_checked(workdir: &std::path::Path) -> Result<(ProjectEngine, bool)> {
    let (engine, config_fixed, failed) = load_workspace(workdir)?;
    if !failed.is_empty() {
        for f in &failed {
            eprintln!("错误: Project {} 打不开: {}", f.id, f.message);
        }
        anyhow::bail!("有 {} 个 Project 打不开，已中止", failed.len());
    }
    Ok((engine, config_fixed))
}

fn dispatch_project(cli: &Cli, pa: &ProjectArgs) -> Result<i32> {
    match &pa.action {
        ProjectAction::List => {
            // 先触发配置加载（含自动修复），即使 list 命令不需要用到 engine
            let (_engine, config_fixed) = load_workspace_checked(&cli.workdir)?;
            if config_fixed {
                eprintln!("已自动修复 tablet.toml 中的无效配置值");
            }
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
        ProjectAction::New { template, from_file, id, name, switch_after } => {
            let source = match (template.as_deref(), from_file.as_deref()) {
                (Some(t), None) => NewProjectSource::Template(t),
                (None, Some(p)) => NewProjectSource::SchemaFile(p),
                _ => anyhow::bail!("需要恰好指定 --template 或 --from-file 之一"),
            };
            let outcome = run_new_project(&cli.workdir, source, id, name.as_deref(), *switch_after)?;
            output::print_new_project_outcome_cli(&outcome.project_root);
            Ok(0)
        }
        ProjectAction::Info { id } => {
            // 点名了项目就按 id 直接加载 —— `load_workspace` 只打开 opened_projects /
            // last_project 那几个，`set_active_by_id` 对没打开的项目返回 false（静默），
            // 于是 `--project X` 会报告**另一个**项目、`--id X` 会报"不存在"。
            let target = cli.project.as_deref().or(id.as_deref());
            let summary = match target {
                Some(pid) => {
                    let project = tablet_core::project::load_specific_project(&cli.workdir, pid)?;
                    crate::actions::project_info::summarize(&project, false)
                }
                None => {
                    let (engine, _config_fixed) = load_workspace_checked(&cli.workdir)?;
                    run_project_info(&engine, None)?
                }
            };
            if cli.is_json() {
                println!("{}", serde_json::to_string_pretty(&summary).unwrap_or_default());
            } else {
                output::print_project_info_cli(&summary);
            }
            Ok(0)
        }
        ProjectAction::Rename { id, new_id, new_name } => {
            let (mut engine, _config_fixed) = load_workspace_checked(&cli.workdir)?;
            run_project_rename(&mut engine, id, new_id.as_deref(), new_name.as_deref())?;
            println!("已重命名 Project: {}", new_id.as_deref().unwrap_or(id));
            Ok(0)
        }
        ProjectAction::Delete { id, confirm } => {
            if !confirm {
                eprintln!("错误: 删除操作不可逆，请添加 --confirm 确认");
                return Ok(1);
            }
            let (mut engine, _config_fixed) = load_workspace_checked(&cli.workdir)?;
            run_project_delete(&mut engine, id)?;
            println!("已删除 Project: {}", id);
            Ok(0)
        }
        ProjectAction::Clone { source, id, name } => {
            let (mut engine, _config_fixed) = load_workspace_checked(&cli.workdir)?;
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
