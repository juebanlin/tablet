//! `--set key=value` 覆盖项目配置。
//!
//! 直接在 `ProjectEngine` 持有的 `Project.config` 上做就地修改。
//! 未知 / 废弃 key 通过 [`OverrideWarning`] 收集起来交给调用方决定怎么报告。

use tbl_core::model::{
    ClientConfig, ExportConfig, GoExport, JavaExport, JsonExport, LuaExport, ServerExport,
    WorkspaceConfig, XmlExport,
};
use tbl_core::ops::ProjectEngine;

#[derive(Debug)]
pub enum OverrideWarning {
    /// `key=value` 格式不对。
    InvalidFormat(String),
    /// 已废弃 key。
    Deprecated { key: String, hint: &'static str },
    /// 未知 key。
    Unknown(String),
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
            "export.client.lua.output" | "export.client.output" => {
                ensure_export_client_lua(&mut project.config);
                project.config.export.as_mut().unwrap().client.as_mut().unwrap().lua.as_mut().unwrap().output = Some(value.to_string());
            }
            other => out.warnings.push(OverrideWarning::Unknown(other.to_string())),
        }
    }
    out
}

fn ensure_export(config: &mut WorkspaceConfig) {
    if config.export.is_none() {
        config.export = Some(ExportConfig {
            json: None, xml: None, server: None, client: None, encoding: None, line_ending: None,
        });
    }
}

fn ensure_export_json(config: &mut WorkspaceConfig) {
    ensure_export(config);
    if config.export.as_ref().unwrap().json.is_none() {
        config.export.as_mut().unwrap().json = Some(JsonExport { empty_as: None, line_ending: None, encoding: None });
    }
}

fn ensure_export_xml(config: &mut WorkspaceConfig) {
    ensure_export(config);
    if config.export.as_ref().unwrap().xml.is_none() {
        config.export.as_mut().unwrap().xml = Some(XmlExport { empty_as: None, line_ending: None, encoding: None });
    }
}

fn ensure_export_server(config: &mut WorkspaceConfig) {
    ensure_export(config);
    if config.export.as_ref().unwrap().server.is_none() {
        config.export.as_mut().unwrap().server = Some(ServerExport {
            data_output: None, java: None, go: None, line_ending: None, encoding: None,
        });
    }
}

fn ensure_export_server_java(config: &mut WorkspaceConfig) {
    ensure_export_server(config);
    if config.export.as_ref().unwrap().server.as_ref().unwrap().java.is_none() {
        config.export.as_mut().unwrap().server.as_mut().unwrap().java = Some(JavaExport {
            package: None, code_output: None, line_ending: None, encoding: None,
        });
    }
}

fn ensure_export_server_go(config: &mut WorkspaceConfig) {
    ensure_export_server(config);
    if config.export.as_ref().unwrap().server.as_ref().unwrap().go.is_none() {
        config.export.as_mut().unwrap().server.as_mut().unwrap().go = Some(GoExport {
            package: None, code_output: None, line_ending: None, encoding: None,
        });
    }
}

fn ensure_export_client(config: &mut WorkspaceConfig) {
    ensure_export(config);
    if config.export.as_ref().unwrap().client.is_none() {
        config.export.as_mut().unwrap().client = Some(ClientConfig {
            lua: None, line_ending: None, encoding: None,
        });
    }
}

fn ensure_export_client_lua(config: &mut WorkspaceConfig) {
    ensure_export_client(config);
    if config.export.as_ref().unwrap().client.as_ref().unwrap().lua.is_none() {
        config.export.as_mut().unwrap().client.as_mut().unwrap().lua = Some(LuaExport {
            output: None, line_ending: None, encoding: None,
        });
    }
}
