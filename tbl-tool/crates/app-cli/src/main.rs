use std::path::PathBuf;
use clap::{Parser, Subcommand};
use anyhow::{Context, Result};
use tbl_core::project::{
    list_projects, load_project, load_specific_project, migrate_legacy_to_default,
    write_project_meta, PROJECTS_DIR,
};
use tbl_core::ops::ProjectEngine;
use tbl_core::template::{instantiate_template, BuiltinTemplates, LocalTemplates, TemplateSource};

#[derive(Parser)]
#[command(name = "tbl-cli", version, about = "TBL 配置管理工具 - 命令行模式")]
struct Cli {
    /// 工作目录（默认当前目录）
    #[arg(short = 'w', long, default_value = ".")]
    workdir: PathBuf,

    /// 显式指定 Project id（覆盖 [app] last_project）
    #[arg(long)]
    project: Option<String>,

    /// 覆盖配置项（格式: key=value，可多次使用）
    #[arg(short = 's', long = "set", value_name = "KEY=VALUE")]
    overrides: Vec<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// 导出数据文件和代码
    Export {
        /// 只导出 JSON 数据文件
        #[arg(long)]
        json: bool,
        /// 只导出 XML 数据文件
        #[arg(long)]
        xml: bool,
        /// 只导出 Java 模板类
        #[arg(long)]
        java: bool,
        /// 只导出 Go 模板代码
        #[arg(long)]
        go: bool,
        /// 只导出 Lua 前端文件
        #[arg(long)]
        lua: bool,
    },
    /// 验证所有 .tbl 文件
    Validate,
    /// 生成测试配置数据
    GenerateTest {
        /// 使用空值测试 schema
        #[arg(long)]
        empty: bool,
        /// 指定外部 .tblschema 文件
        #[arg(long)]
        schema: Option<PathBuf>,
        /// 数据行数（0 表示使用默认固定数据）
        #[arg(long, default_value_t = 0)]
        rows: usize,
        /// 随机种子（0 表示使用固定数据，非 0 启用随机生成）
        #[arg(long, default_value_t = 0)]
        seed: u64,
        /// 数据格式（json 或 xml），影响 TestMain.java 的初始化方式
        #[arg(long, default_value = "json")]
        format: String,
        /// 测试语言（java / none），none 时不生成 TestMain.java
        #[arg(long, default_value = "java")]
        lang: String,
    },
    /// 列出所有 Project（@02 Project）
    ListProjects,
    /// 列出可用模板（@02 项目模板）
    ListTemplates,
    /// 把根目录 config/ 迁移到 projects/default/
    MigrateLegacy,
    /// 用模板新建 Project
    NewProject {
        /// 模板 id（来自 list-templates）
        #[arg(long)]
        template: String,
        /// Project id（[a-z0-9_-]{1,32}）
        #[arg(long)]
        id: String,
        /// Project 显示名（默认 = id）
        #[arg(long)]
        name: Option<String>,
        /// 创建后切换为 last_project（默认开启）
        #[arg(long, default_value_t = true)]
        switch_after: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // 这些子命令不需要先加载 Project
    match &cli.command {
        Command::ListTemplates => return run_list_templates(),
        Command::MigrateLegacy => return run_migrate(&cli.workdir),
        Command::ListProjects => return run_list_projects(&cli.workdir),
        Command::NewProject { template, id, name, switch_after } => {
            return run_new_project(&cli.workdir, template, id, name.as_deref(), *switch_after);
        }
        _ => {}
    }

    let project = match cli.project.as_deref() {
        Some(pid) => load_specific_project(&cli.workdir, pid)?,
        None => load_project(&cli.workdir)?,
    };
    let mut engine = ProjectEngine::new(project);

    apply_overrides(&mut engine, &cli.overrides);

    match cli.command {
        Command::Export { json, xml, java, go, lua } => {
            let export_all = !json && !xml && !java && !go && !lua;

            if export_all || json {
                match engine.export_json() {
                    Ok(r) => print_export_result("JSON", &r),
                    Err(e) => eprintln!("[JSON] 错误: {}", e),
                }
            }

            if export_all || xml {
                match engine.export_xml() {
                    Ok(r) => print_export_result("XML", &r),
                    Err(e) => eprintln!("[XML] 错误: {}", e),
                }
            }

            if export_all || java {
                match engine.export_java() {
                    Ok(r) => print_export_result("Java", &r),
                    Err(e) => eprintln!("[Java] 错误: {}", e),
                }
            }

            if export_all || go {
                match engine.export_go() {
                    Ok(r) => print_export_result("Go", &r),
                    Err(e) => eprintln!("[Go] 错误: {}", e),
                }
            }

            if export_all || lua {
                match engine.export_lua() {
                    Ok(r) => print_export_result("Lua", &r),
                    Err(e) => eprintln!("[Lua] 错误: {}", e),
                }
            }
        }
        Command::Validate => {
            engine.revalidate_all();
            if engine.validation_errors.is_empty() {
                println!("验证通过，无错误");
            } else {
                println!("发现 {} 个验证错误:", engine.validation_errors.len());
                for (project_id, group, name, row, col) in &engine.validation_errors {
                    println!("  [{}] {}/{} [{},{}]", project_id, group, name, row, col);
                }
                std::process::exit(1);
            }
        }
        Command::GenerateTest { empty, schema, rows, seed, format, lang } => {
            let config_dir = engine.project().data_dir();
            let opts = tbl_core::test_util::TestGenOptions {
                include_empty: empty,
                rows,
                seed,
            };

            let server = engine.project().config.export.as_ref().and_then(|e| e.server.as_ref());
            let java_pkg = server.and_then(|s| s.java.as_ref())
                .and_then(|j| j.package.as_deref())
                .unwrap_or("com.game.config")
                .to_string();
            let go_pkg = server.and_then(|s| s.go.as_ref())
                .and_then(|g| g.package.as_deref())
                .unwrap_or("config")
                .to_string();
            let go_code_output = server.and_then(|s| s.go.as_ref())
                .and_then(|g| g.code_output.as_deref())
                .unwrap_or("gen/server/go")
                .to_string();

            if let Some(schema_path) = schema {
                let content = std::fs::read_to_string(&schema_path)
                    .unwrap_or_else(|e| { eprintln!("读取 schema 失败: {}", e); std::process::exit(1); });
                let parsed = tbl_core::tblschema::parse_tblschema(&content)
                    .unwrap_or_else(|e| { eprintln!("解析 schema 失败: {}", e); std::process::exit(1); });
                tbl_core::test_util::generate_from_schema(&config_dir, &parsed, &opts);

                match lang.as_str() {
                    "java" => tbl_core::test_util::generate_test_main_from_schema(&cli.workdir, &parsed, &java_pkg, &format),
                    "go" => tbl_core::test_util::generate_test_main_go_from_schema(&cli.workdir, &parsed, &go_pkg, &go_code_output, &format),
                    "none" => {}
                    other => eprintln!("未知 --lang: {}", other),
                }
            } else {
                tbl_core::test_util::generate_test_config(&config_dir, &opts);

                match lang.as_str() {
                    "java" => tbl_core::test_util::generate_test_main(&cli.workdir, &opts, &java_pkg, &format),
                    "go" => tbl_core::test_util::generate_test_main_go(&cli.workdir, &opts, &go_pkg, &go_code_output, &format),
                    "none" => {}
                    other => eprintln!("未知 --lang: {}", other),
                }
            }

            println!("已生成测试配置");
        }
        // 这些分支已在 main 顶部处理（不需要加载 Project），加 unreachable 兜底
        Command::ListProjects | Command::ListTemplates | Command::MigrateLegacy | Command::NewProject { .. } => {
            unreachable!("已在 main 顶部 dispatch")
        }
    }

    Ok(())
}

// PLACEHOLDER_APPLY_OVERRIDES

fn run_list_templates() -> Result<()> {
    let builtin = BuiltinTemplates::new();
    println!("== 内置模板 ==");
    for m in builtin.list() {
        let category = if m.category.is_empty() { "-" } else { m.category.as_str() };
        let version = if m.version.is_empty() { "-" } else { m.version.as_str() };
        println!("  {:<16} {:<24} category={:<8} version={}", m.id, m.name, category, version);
    }

    let local = LocalTemplates::new(tbl_core::template::default_local_dir());
    let local_list = local.list();
    if !local_list.is_empty() {
        println!();
        println!("== 本地模板（{}）==", local.root.display());
        for m in local_list {
            let category = if m.category.is_empty() { "-" } else { m.category.as_str() };
            let version = if m.version.is_empty() { "-" } else { m.version.as_str() };
            println!("  {:<16} {:<24} category={:<8} version={}", m.id, m.name, category, version);
        }
    }
    Ok(())
}

fn run_list_projects(workdir: &PathBuf) -> Result<()> {
    let projects = list_projects(workdir);
    if projects.is_empty() {
        println!("(无 Project；可用 `tbl-cli new-project --template full --id default --name 默认项目` 创建)");
        return Ok(());
    }
    for p in projects {
        println!("{:<24} {}", p.id, p.name);
    }
    Ok(())
}

fn run_migrate(workdir: &PathBuf) -> Result<()> {
    let migrated = migrate_legacy_to_default(workdir)?;
    if migrated {
        println!("已迁移老 config/ 到 projects/default/");
    } else {
        println!("无需迁移：projects/ 已存在或老 config/ 不存在");
    }
    Ok(())
}

fn run_new_project(
    workdir: &PathBuf,
    template_id: &str,
    project_id: &str,
    name: Option<&str>,
    switch_after: bool,
) -> Result<()> {
    use tbl_core::tblschema::is_valid_metadata_id;
    if !is_valid_metadata_id(project_id) {
        anyhow::bail!("Project id 不合法（要求 [a-z0-9_-]{{1,32}}）：{}", project_id);
    }

    // 找模板：先内置后本地
    let builtin = BuiltinTemplates::new();
    let local = LocalTemplates::new(tbl_core::template::default_local_dir());
    let content = builtin.load_by_id(template_id)
        .or_else(|| local.load_by_id(template_id))
        .with_context(|| format!("找不到模板: {}", template_id))?;

    // 实例化到 projects/<id>/
    let projects_dir = workdir.join(PROJECTS_DIR);
    std::fs::create_dir_all(&projects_dir)?;
    let project_root = projects_dir.join(project_id);
    if project_root.exists() {
        anyhow::bail!("Project 已存在: {}", project_root.display());
    }

    instantiate_template(&content.schema, &project_root)?;

    // 写 project.toml
    let display_name = name.unwrap_or(project_id).to_string();
    let meta = tbl_core::model::ProjectInstanceMeta {
        id: project_id.to_string(),
        name: display_name,
        created_at: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        source_template: content.meta.id.clone(),
        source_template_version: content.meta.version.clone(),
    };
    write_project_meta(&project_root, &meta)?;

    // 更新 last_project（如启用）
    if switch_after {
        let config_path = workdir.join(tbl_core::CONFIG_FILE);
        // 确保 tbl-tool.toml 存在（不存在则会被 load_project 写出默认）
        if !config_path.exists() {
            let _ = load_project(workdir);
        }
        let original = std::fs::read_to_string(&config_path)?;
        let mut project_cfg = tbl_core::model::ProjectConfig {
            name: "my-game".to_string(),
            last_project: project_id.to_string(),
            opened_projects: Vec::new(),
            project_sort: String::new(),
            project_order: Vec::new(),
            config_dir: "config".to_string(),
            cache_dir: ".tbl-cache".to_string(),
        };
        // 保留原有 [project].name（如有）
        if let Ok(parsed) = toml::from_str::<tbl_core::model::WorkspaceConfig>(&original) {
            project_cfg.name = parsed.project.name;
        }
        let updated = tbl_core::project::upsert_project_config_section(&original, &project_cfg);
        std::fs::write(&config_path, updated)?;
    }

    println!("已创建 Project: {}", project_root.display());
    Ok(())
}

fn apply_overrides(engine: &mut ProjectEngine, overrides: &[String]) {
    let project = engine.project_mut();
    for item in overrides {
        let Some((key, value)) = item.split_once('=') else {
            eprintln!("警告: 无效的覆盖参数 '{}', 格式应为 key=value", item);
            continue;
        };
        match key.trim() {
            // 历史 key（兼容老脚本/test fixture）：app.* 与 project.* 等价
            "project.name" | "app.name" => project.config.project.name = value.to_string(),
            "project.config_dir" | "app.config_dir" => project.config.project.config_dir = value.to_string(),
            "project.cache_dir" | "app.cache_dir" => project.config.project.cache_dir = value.to_string(),
            "app.last_project" | "project.last_project" => project.config.project.last_project = value.to_string(),
            "export.encoding" => {
                ensure_export(&mut project.config);
                project.config.export.as_mut().unwrap().encoding = Some(value.to_string());
            }
            "export.line_ending" => {
                ensure_export(&mut project.config);
                project.config.export.as_mut().unwrap().line_ending = Some(value.to_string());
            }
            "export.json.empty_as" => {
                ensure_export_json(&mut project.config);
                project.config.export.as_mut().unwrap().json.as_mut().unwrap().empty_as = Some(value.to_string());
            }
            "export.xml.empty_as" => {
                ensure_export_xml(&mut project.config);
                project.config.export.as_mut().unwrap().xml.as_mut().unwrap().empty_as = Some(value.to_string());
            }
            "export.server.lang" => {
                eprintln!("警告: 'export.server.lang' 已废弃，使用 export.server.java / export.server.go 子节点");
            }
            "export.server.package" => {
                ensure_export_server_java(&mut project.config);
                project.config.export.as_mut().unwrap().server.as_mut().unwrap().java.as_mut().unwrap().package = Some(value.to_string());
            }
            "export.server.data_output" => {
                ensure_export_server(&mut project.config);
                project.config.export.as_mut().unwrap().server.as_mut().unwrap().data_output = Some(value.to_string());
            }
            "export.server.java.package" => {
                ensure_export_server_java(&mut project.config);
                project.config.export.as_mut().unwrap().server.as_mut().unwrap().java.as_mut().unwrap().package = Some(value.to_string());
            }
            "export.server.java.code_output" => {
                ensure_export_server_java(&mut project.config);
                project.config.export.as_mut().unwrap().server.as_mut().unwrap().java.as_mut().unwrap().code_output = Some(value.to_string());
            }
            "export.server.go.package" => {
                ensure_export_server_go(&mut project.config);
                project.config.export.as_mut().unwrap().server.as_mut().unwrap().go.as_mut().unwrap().package = Some(value.to_string());
            }
            "export.server.go.code_output" => {
                ensure_export_server_go(&mut project.config);
                project.config.export.as_mut().unwrap().server.as_mut().unwrap().go.as_mut().unwrap().code_output = Some(value.to_string());
            }
            "export.server.code_output" => {
                ensure_export_server_java(&mut project.config);
                project.config.export.as_mut().unwrap().server.as_mut().unwrap().java.as_mut().unwrap().code_output = Some(value.to_string());
            }
            "export.client.lua.output" => {
                ensure_export_client_lua(&mut project.config);
                project.config.export.as_mut().unwrap().client.as_mut().unwrap().lua.as_mut().unwrap().output = Some(value.to_string());
            }
            "export.client.output" => {
                ensure_export_client_lua(&mut project.config);
                project.config.export.as_mut().unwrap().client.as_mut().unwrap().lua.as_mut().unwrap().output = Some(value.to_string());
            }
            _ => eprintln!("警告: 未知配置项 '{}'", key),
        }
    }
}

fn ensure_export(config: &mut tbl_core::model::WorkspaceConfig) {
    if config.export.is_none() {
        config.export = Some(tbl_core::model::ExportConfig {
            json: None, xml: None, server: None, client: None, encoding: None, line_ending: None,
        });
    }
}

fn ensure_export_json(config: &mut tbl_core::model::WorkspaceConfig) {
    ensure_export(config);
    if config.export.as_ref().unwrap().json.is_none() {
        config.export.as_mut().unwrap().json = Some(tbl_core::model::JsonExport { empty_as: None, line_ending: None, encoding: None });
    }
}

fn ensure_export_xml(config: &mut tbl_core::model::WorkspaceConfig) {
    ensure_export(config);
    if config.export.as_ref().unwrap().xml.is_none() {
        config.export.as_mut().unwrap().xml = Some(tbl_core::model::XmlExport { empty_as: None, line_ending: None, encoding: None });
    }
}

fn ensure_export_server(config: &mut tbl_core::model::WorkspaceConfig) {
    ensure_export(config);
    if config.export.as_ref().unwrap().server.is_none() {
        config.export.as_mut().unwrap().server = Some(tbl_core::model::ServerExport {
            data_output: None, java: None, go: None, line_ending: None, encoding: None,
        });
    }
}

fn ensure_export_server_java(config: &mut tbl_core::model::WorkspaceConfig) {
    ensure_export_server(config);
    if config.export.as_ref().unwrap().server.as_ref().unwrap().java.is_none() {
        config.export.as_mut().unwrap().server.as_mut().unwrap().java = Some(tbl_core::model::JavaExport {
            package: None, code_output: None, line_ending: None, encoding: None,
        });
    }
}

fn ensure_export_server_go(config: &mut tbl_core::model::WorkspaceConfig) {
    ensure_export_server(config);
    if config.export.as_ref().unwrap().server.as_ref().unwrap().go.is_none() {
        config.export.as_mut().unwrap().server.as_mut().unwrap().go = Some(tbl_core::model::GoExport {
            package: None, code_output: None, line_ending: None, encoding: None,
        });
    }
}

fn ensure_export_client(config: &mut tbl_core::model::WorkspaceConfig) {
    ensure_export(config);
    if config.export.as_ref().unwrap().client.is_none() {
        config.export.as_mut().unwrap().client = Some(tbl_core::model::ClientConfig {
            lua: None, line_ending: None, encoding: None,
        });
    }
}

fn ensure_export_client_lua(config: &mut tbl_core::model::WorkspaceConfig) {
    ensure_export_client(config);
    if config.export.as_ref().unwrap().client.as_ref().unwrap().lua.is_none() {
        config.export.as_mut().unwrap().client.as_mut().unwrap().lua = Some(tbl_core::model::LuaExport {
            output: None, line_ending: None, encoding: None,
        });
    }
}

fn print_export_result(label: &str, result: &tbl_core::export::ExportResult) {
    use tbl_core::export::FileStatus;
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
