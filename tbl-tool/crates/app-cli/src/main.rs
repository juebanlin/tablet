use std::path::PathBuf;
use clap::{Parser, Subcommand};
use anyhow::Result;
use tbl_core::project::load_project;
use tbl_core::ops::ProjectEngine;

#[derive(Parser)]
#[command(name = "tbl-cli", version, about = "TBL 配置管理工具 - 命令行模式")]
struct Cli {
    /// 工作目录（默认当前目录）
    #[arg(short = 'w', long, default_value = ".")]
    workdir: PathBuf,

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
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let project = load_project(&cli.workdir)?;
    let mut engine = ProjectEngine::new(project);

    apply_overrides(&mut engine, &cli.overrides);

    match cli.command {
        Command::Export { json, xml, java, lua } => {
            let export_all = !json && !xml && !java && !lua;

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
                for (group, name, row, col) in &engine.validation_errors {
                    println!("  {}/{} [{},{}]", group, name, row, col);
                }
                std::process::exit(1);
            }
        }
        Command::GenerateTest { empty, schema, rows, seed, format } => {
            let config_dir = engine.project.workdir.join(&engine.project.config.project.config_dir);
            let opts = tbl_core::test_util::TestGenOptions {
                include_empty: empty,
                rows,
                seed,
            };

            if let Some(schema_path) = schema {
                let content = std::fs::read_to_string(&schema_path)
                    .unwrap_or_else(|e| { eprintln!("读取 schema 失败: {}", e); std::process::exit(1); });
                let parsed = tbl_core::tblschema::parse_tblschema(&content)
                    .unwrap_or_else(|e| { eprintln!("解析 schema 失败: {}", e); std::process::exit(1); });
                tbl_core::test_util::generate_from_schema(&config_dir, &parsed, &opts);

                let pkg = engine.project.config.export.as_ref()
                    .and_then(|e| e.server.as_ref())
                    .and_then(|s| s.java.as_ref())
                    .and_then(|j| j.package.as_deref())
                    .unwrap_or("com.game.config");
                tbl_core::test_util::generate_test_main_from_schema(&cli.workdir, &parsed, pkg, &format);
            } else {
                tbl_core::test_util::generate_test_config(&config_dir, &opts);

                let pkg = engine.project.config.export.as_ref()
                    .and_then(|e| e.server.as_ref())
                    .and_then(|s| s.java.as_ref())
                    .and_then(|j| j.package.as_deref())
                    .unwrap_or("com.game.config");
                tbl_core::test_util::generate_test_main(&cli.workdir, &opts, pkg, &format);
            }

            println!("已生成测试配置");
        }
    }

    Ok(())
}

// PLACEHOLDER_APPLY_OVERRIDES

fn apply_overrides(engine: &mut ProjectEngine, overrides: &[String]) {
    for item in overrides {
        let Some((key, value)) = item.split_once('=') else {
            eprintln!("警告: 无效的覆盖参数 '{}', 格式应为 key=value", item);
            continue;
        };
        match key.trim() {
            "project.name" => engine.project.config.project.name = value.to_string(),
            "project.config_dir" => engine.project.config.project.config_dir = value.to_string(),
            "project.cache_dir" => engine.project.config.project.cache_dir = value.to_string(),
            "export.encoding" => {
                ensure_export(&mut engine.project.config);
                engine.project.config.export.as_mut().unwrap().encoding = Some(value.to_string());
            }
            "export.line_ending" => {
                ensure_export(&mut engine.project.config);
                engine.project.config.export.as_mut().unwrap().line_ending = Some(value.to_string());
            }
            "export.json.empty_as" => {
                ensure_export_json(&mut engine.project.config);
                engine.project.config.export.as_mut().unwrap().json.as_mut().unwrap().empty_as = Some(value.to_string());
            }
            "export.xml.empty_as" => {
                ensure_export_xml(&mut engine.project.config);
                engine.project.config.export.as_mut().unwrap().xml.as_mut().unwrap().empty_as = Some(value.to_string());
            }
            "export.server.lang" => {
                eprintln!("警告: 'export.server.lang' 已废弃，使用 export.server.java / export.server.go 子节点");
            }
            "export.server.package" => {
                ensure_export_server_java(&mut engine.project.config);
                engine.project.config.export.as_mut().unwrap().server.as_mut().unwrap().java.as_mut().unwrap().package = Some(value.to_string());
            }
            "export.server.data_output" => {
                ensure_export_server(&mut engine.project.config);
                engine.project.config.export.as_mut().unwrap().server.as_mut().unwrap().data_output = Some(value.to_string());
            }
            "export.server.java.package" => {
                ensure_export_server_java(&mut engine.project.config);
                engine.project.config.export.as_mut().unwrap().server.as_mut().unwrap().java.as_mut().unwrap().package = Some(value.to_string());
            }
            "export.server.java.code_output" => {
                ensure_export_server_java(&mut engine.project.config);
                engine.project.config.export.as_mut().unwrap().server.as_mut().unwrap().java.as_mut().unwrap().code_output = Some(value.to_string());
            }
            "export.server.go.package" => {
                ensure_export_server_go(&mut engine.project.config);
                engine.project.config.export.as_mut().unwrap().server.as_mut().unwrap().go.as_mut().unwrap().package = Some(value.to_string());
            }
            "export.server.go.code_output" => {
                ensure_export_server_go(&mut engine.project.config);
                engine.project.config.export.as_mut().unwrap().server.as_mut().unwrap().go.as_mut().unwrap().code_output = Some(value.to_string());
            }
            "export.server.code_output" => {
                ensure_export_server_java(&mut engine.project.config);
                engine.project.config.export.as_mut().unwrap().server.as_mut().unwrap().java.as_mut().unwrap().code_output = Some(value.to_string());
            }
            "export.client.lua.output" => {
                ensure_export_client_lua(&mut engine.project.config);
                engine.project.config.export.as_mut().unwrap().client.as_mut().unwrap().lua.as_mut().unwrap().output = Some(value.to_string());
            }
            "export.client.output" => {
                ensure_export_client_lua(&mut engine.project.config);
                engine.project.config.export.as_mut().unwrap().client.as_mut().unwrap().lua.as_mut().unwrap().output = Some(value.to_string());
            }
            _ => eprintln!("警告: 未知配置项 '{}'", key),
        }
    }
}

fn ensure_export(config: &mut tbl_core::model::ProjectConfig) {
    if config.export.is_none() {
        config.export = Some(tbl_core::model::ExportConfig {
            json: None, xml: None, server: None, client: None, encoding: None, line_ending: None,
        });
    }
}

fn ensure_export_json(config: &mut tbl_core::model::ProjectConfig) {
    ensure_export(config);
    if config.export.as_ref().unwrap().json.is_none() {
        config.export.as_mut().unwrap().json = Some(tbl_core::model::JsonExport { empty_as: None, line_ending: None, encoding: None });
    }
}

fn ensure_export_xml(config: &mut tbl_core::model::ProjectConfig) {
    ensure_export(config);
    if config.export.as_ref().unwrap().xml.is_none() {
        config.export.as_mut().unwrap().xml = Some(tbl_core::model::XmlExport { empty_as: None, line_ending: None, encoding: None });
    }
}

fn ensure_export_server(config: &mut tbl_core::model::ProjectConfig) {
    ensure_export(config);
    if config.export.as_ref().unwrap().server.is_none() {
        config.export.as_mut().unwrap().server = Some(tbl_core::model::ServerExport {
            data_output: None, java: None, go: None, line_ending: None, encoding: None,
        });
    }
}

fn ensure_export_server_java(config: &mut tbl_core::model::ProjectConfig) {
    ensure_export_server(config);
    if config.export.as_ref().unwrap().server.as_ref().unwrap().java.is_none() {
        config.export.as_mut().unwrap().server.as_mut().unwrap().java = Some(tbl_core::model::JavaExport {
            package: None, code_output: None, line_ending: None, encoding: None,
        });
    }
}

fn ensure_export_server_go(config: &mut tbl_core::model::ProjectConfig) {
    ensure_export_server(config);
    if config.export.as_ref().unwrap().server.as_ref().unwrap().go.is_none() {
        config.export.as_mut().unwrap().server.as_mut().unwrap().go = Some(tbl_core::model::GoExport {
            package: None, code_output: None, line_ending: None, encoding: None,
        });
    }
}

fn ensure_export_client(config: &mut tbl_core::model::ProjectConfig) {
    ensure_export(config);
    if config.export.as_ref().unwrap().client.is_none() {
        config.export.as_mut().unwrap().client = Some(tbl_core::model::ClientConfig {
            lua: None, line_ending: None, encoding: None,
        });
    }
}

fn ensure_export_client_lua(config: &mut tbl_core::model::ProjectConfig) {
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
