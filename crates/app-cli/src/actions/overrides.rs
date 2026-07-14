//! `--set key=value` 覆盖项目配置。
//!
//! 直接在 `ProjectEngine` 持有的 `Project.config` 上做就地修改。
//! 未知 / 废弃 key 通过 [`OverrideWarning`] 收集起来交给调用方决定怎么报告。

use std::str::FromStr;

use tablet_core::model::{
    ClientConfig, CppExport, CSharpExport, ExportConfig, GoExport, JavaExport, JsonExport,
    LuaExport, ProjectConfig, ServerExport, TypeScriptExport, XmlExport,
};
use tablet_core::ops::ProjectEngine;

#[derive(Debug)]
pub enum OverrideWarning {
    /// `key=value` 格式不对。
    InvalidFormat(String),
    /// 已废弃 key。
    Deprecated { key: String, hint: &'static str },
    /// 未知 key。
    Unknown(String),
    /// 无效值。
    Invalid(String),
}

#[derive(Debug, Default)]
pub struct OverrideOutcome {
    pub warnings: Vec<OverrideWarning>,
}

pub fn apply_overrides(engine: &mut ProjectEngine, overrides: &[String]) -> OverrideOutcome {
    let mut out = OverrideOutcome::default();
    let project = engine.project_mut();
    for item in overrides {
        let Some((key, value)) = item.split_once('=') else {
            out.warnings.push(OverrideWarning::InvalidFormat(item.clone()));
            continue;
        };
        match key.trim() {
            "project.config_dir" | "app.config_dir" => {
                out.warnings.push(OverrideWarning::Deprecated {
                    key: key.to_string(),
                    hint: "config_dir 已废弃，多项目模式下固定使用 <project_root>/config/",
                });
            }
            "project.cache_dir" | "app.cache_dir" => {
                out.warnings.push(OverrideWarning::Deprecated {
                    key: key.to_string(),
                    hint: "cache_dir 已废弃，多项目模式下固定使用 <project_root>/.tbl-cache/",
                });
            }
            "app.last_project" | "project.last_project" => {
                out.warnings.push(OverrideWarning::Deprecated {
                    key: key.to_string(),
                    hint: "请直接修改 tablet.toml 的 [project] last_project 字段",
                });
            }
            "export.encoding" => {
                ensure_export(&mut project.config);
                project.config.export.as_mut().unwrap().encoding = Some(value.parse().unwrap_or_default());
            }
            "export.line_ending" => {
                ensure_export(&mut project.config);
                project.config.export.as_mut().unwrap().line_ending = Some(value.parse().unwrap_or_default());
            }
            "export.json.empty_as" => {
                ensure_export_json(&mut project.config);
                project.config.export.as_mut().unwrap().json.as_mut().unwrap().empty_as = Some(value.parse().unwrap_or_default());
            }
            "export.xml.empty_as" => {
                ensure_export_xml(&mut project.config);
                project.config.export.as_mut().unwrap().xml.as_mut().unwrap().empty_as = Some(value.parse().unwrap_or_default());
            }
            "export.server.lang" => {
                out.warnings.push(OverrideWarning::Deprecated {
                    key: key.to_string(),
                    hint: "export.server.lang 已废弃，使用 export.server.java / export.server.go 子节点",
                });
            }
            "export.server.package" | "export.server.java.package" => {
                ensure_export_server_java(&mut project.config);
                project.config.export.as_mut().unwrap().server.as_mut().unwrap().java.as_mut().unwrap().package = Some(value.to_string());
            }
            "export.server.data_output" => {
                ensure_export_server(&mut project.config);
                project.config.export.as_mut().unwrap().server.as_mut().unwrap().data_output = Some(value.to_string());
            }
            "export.server.code_output" | "export.server.java.code_output" => {
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
            "export.server.cpp.namespace" => {
                ensure_export_server_cpp(&mut project.config);
                project.config.export.as_mut().unwrap().server.as_mut().unwrap().cpp.as_mut().unwrap().namespace = Some(value.to_string());
            }
            "export.server.cpp.code_output" => {
                ensure_export_server_cpp(&mut project.config);
                project.config.export.as_mut().unwrap().server.as_mut().unwrap().cpp.as_mut().unwrap().code_output = Some(value.to_string());
            }
            "export.server.cpp.json_lib" => {
                ensure_export_server_cpp(&mut project.config);
                project.config.export.as_mut().unwrap().server.as_mut().unwrap().cpp.as_mut().unwrap().json_lib = Some(value.parse().unwrap_or_default());
            }
            "export.server.csharp_dotnet.namespace" => {
                ensure_export_server_csharp_dotnet(&mut project.config);
                project.config.export.as_mut().unwrap().server.as_mut().unwrap().csharp_dotnet.as_mut().unwrap().namespace = Some(value.to_string());
            }
            "export.server.csharp_dotnet.code_output" => {
                ensure_export_server_csharp_dotnet(&mut project.config);
                project.config.export.as_mut().unwrap().server.as_mut().unwrap().csharp_dotnet.as_mut().unwrap().code_output = Some(value.to_string());
            }
            "export.client.csharp_unity.namespace" => {
                ensure_export_client_csharp_unity(&mut project.config);
                project.config.export.as_mut().unwrap().client.as_mut().unwrap().csharp_unity.as_mut().unwrap().namespace = Some(value.to_string());
            }
            "export.client.csharp_unity.code_output" => {
                ensure_export_client_csharp_unity(&mut project.config);
                project.config.export.as_mut().unwrap().client.as_mut().unwrap().csharp_unity.as_mut().unwrap().code_output = Some(value.to_string());
            }
            "export.client.csharp_godot.namespace" => {
                ensure_export_client_csharp_godot(&mut project.config);
                project.config.export.as_mut().unwrap().client.as_mut().unwrap().csharp_godot.as_mut().unwrap().namespace = Some(value.to_string());
            }
            "export.client.csharp_godot.code_output" => {
                ensure_export_client_csharp_godot(&mut project.config);
                project.config.export.as_mut().unwrap().client.as_mut().unwrap().csharp_godot.as_mut().unwrap().code_output = Some(value.to_string());
            }
            "export.client.lua.output" | "export.client.output" => {
                ensure_export_client_lua(&mut project.config);
                project.config.export.as_mut().unwrap().client.as_mut().unwrap().lua.as_mut().unwrap().output = Some(value.to_string());
            }
            "export.client.typescript.output" => {
                ensure_export_client_typescript(&mut project.config);
                project.config.export.as_mut().unwrap().client.as_mut().unwrap().typescript.as_mut().unwrap().output = Some(value.to_string());
            }
            "export.client.typescript.module_kind" => {
                ensure_export_client_typescript(&mut project.config);
                match tablet_core::enums::ModuleKind::from_str(value) {
                    Ok(kind) => {
                        project.config.export.as_mut().unwrap().client.as_mut().unwrap().typescript.as_mut().unwrap().module_kind = Some(kind);
                    }
                    Err(_) => {
                        out.warnings.push(OverrideWarning::Invalid(format!(
                            "export.client.typescript.module_kind: invalid value '{}', expected 'esm' or 'commonjs'",
                            value
                        )));
                    }
                }
            }
            "export.server.typescript.output" => {
                ensure_export_server_typescript(&mut project.config);
                project.config.export.as_mut().unwrap().server.as_mut().unwrap().typescript.as_mut().unwrap().output = Some(value.to_string());
            }
            "export.server.typescript.module_kind" => {
                ensure_export_server_typescript(&mut project.config);
                match tablet_core::enums::ModuleKind::from_str(value) {
                    Ok(kind) => {
                        project.config.export.as_mut().unwrap().server.as_mut().unwrap().typescript.as_mut().unwrap().module_kind = Some(kind);
                    }
                    Err(_) => {
                        out.warnings.push(OverrideWarning::Invalid(format!(
                            "export.server.typescript.module_kind: invalid value '{}', expected 'esm' or 'commonjs'",
                            value
                        )));
                    }
                }
            }
            other => out.warnings.push(OverrideWarning::Unknown(other.to_string())),
        }
    }
    out
}

fn ensure_export(config: &mut ProjectConfig) {
    if config.export.is_none() {
        config.export = Some(ExportConfig {
            json: None, xml: None, server: None, client: None, encoding: None, line_ending: None,
        });
    }
}

fn ensure_export_json(config: &mut ProjectConfig) {
    ensure_export(config);
    if config.export.as_ref().unwrap().json.is_none() {
        config.export.as_mut().unwrap().json = Some(JsonExport { empty_as: None });
    }
}

fn ensure_export_xml(config: &mut ProjectConfig) {
    ensure_export(config);
    if config.export.as_ref().unwrap().xml.is_none() {
        config.export.as_mut().unwrap().xml = Some(XmlExport { empty_as: None });
    }
}

fn ensure_export_server(config: &mut ProjectConfig) {
    ensure_export(config);
    if config.export.as_ref().unwrap().server.is_none() {
        config.export.as_mut().unwrap().server = Some(ServerExport {
            data_output: None, java: None, go: None, cpp: None, csharp_dotnet: None,
            typescript: None,
        });
    }
}

fn ensure_export_server_java(config: &mut ProjectConfig) {
    ensure_export_server(config);
    if config.export.as_ref().unwrap().server.as_ref().unwrap().java.is_none() {
        config.export.as_mut().unwrap().server.as_mut().unwrap().java = Some(JavaExport {
            package: None, code_output: None,
        });
    }
}

fn ensure_export_server_go(config: &mut ProjectConfig) {
    ensure_export_server(config);
    if config.export.as_ref().unwrap().server.as_ref().unwrap().go.is_none() {
        config.export.as_mut().unwrap().server.as_mut().unwrap().go = Some(GoExport {
            package: None, code_output: None,
        });
    }
}

fn ensure_export_server_cpp(config: &mut ProjectConfig) {
    ensure_export_server(config);
    if config.export.as_ref().unwrap().server.as_ref().unwrap().cpp.is_none() {
        config.export.as_mut().unwrap().server.as_mut().unwrap().cpp = Some(CppExport {
            namespace: None, code_output: None, json_lib: None,
        });
    }
}

fn ensure_export_server_csharp_dotnet(config: &mut ProjectConfig) {
    ensure_export_server(config);
    if config.export.as_ref().unwrap().server.as_ref().unwrap().csharp_dotnet.is_none() {
        config.export.as_mut().unwrap().server.as_mut().unwrap().csharp_dotnet = Some(CSharpExport {
            namespace: None, code_output: None,
        });
    }
}

fn ensure_export_client(config: &mut ProjectConfig) {
    ensure_export(config);
    if config.export.as_ref().unwrap().client.is_none() {
        config.export.as_mut().unwrap().client = Some(ClientConfig {
            lua: None, gdscript: None, typescript: None,
            csharp_unity: None, csharp_godot: None,
        });
    }
}

fn ensure_export_client_csharp_unity(config: &mut ProjectConfig) {
    ensure_export_client(config);
    if config.export.as_ref().unwrap().client.as_ref().unwrap().csharp_unity.is_none() {
        config.export.as_mut().unwrap().client.as_mut().unwrap().csharp_unity = Some(CSharpExport {
            namespace: None, code_output: None,
        });
    }
}

fn ensure_export_client_csharp_godot(config: &mut ProjectConfig) {
    ensure_export_client(config);
    if config.export.as_ref().unwrap().client.as_ref().unwrap().csharp_godot.is_none() {
        config.export.as_mut().unwrap().client.as_mut().unwrap().csharp_godot = Some(CSharpExport {
            namespace: None, code_output: None,
        });
    }
}

fn ensure_export_client_lua(config: &mut ProjectConfig) {
    ensure_export_client(config);
    if config.export.as_ref().unwrap().client.as_ref().unwrap().lua.is_none() {
        config.export.as_mut().unwrap().client.as_mut().unwrap().lua = Some(LuaExport {
            output: None,
        });
    }
}

fn ensure_export_client_typescript(config: &mut ProjectConfig) {
    ensure_export_client(config);
    if config.export.as_ref().unwrap().client.as_ref().unwrap().typescript.is_none() {
        config.export.as_mut().unwrap().client.as_mut().unwrap().typescript = Some(TypeScriptExport {
            output: None, module_kind: None,
        });
    }
}

fn ensure_export_server_typescript(config: &mut ProjectConfig) {
    ensure_export_server(config);
    if config.export.as_ref().unwrap().server.as_ref().unwrap().typescript.is_none() {
        config.export.as_mut().unwrap().server.as_mut().unwrap().typescript = Some(TypeScriptExport {
            output: None, module_kind: None,
        });
    }
}
