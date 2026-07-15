// 项目设置对话框：身份（id/name/category/version）+ 分隔符（25 leaves）2 tab。
//
// 所有改动只更新内存，标记 schema_dirty=true，等"保存项目"才写盘。

use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;
use tablet_core::tblschema::is_valid_metadata_id;
use tablet_core::types::{SepKey, SeparatorsSection};

use crate::state::AppState;
use crate::{refresh, AppWindow};

/// 由项目右键「项目设置...」入口调用：把当前 project meta + separators + export 拷进 buf，开 dialog。
pub fn open_for(state: &Rc<RefCell<AppState>>, project_id: &str) {
    let mut st = state.borrow_mut();
    let Some(p) = st.engine.find_project(project_id) else {
        st.engine.ui_log(format!("[项目设置] 项目不存在: {}", project_id));
        return;
    };

    // 先提取所有需要的数据，避免借用冲突
    let id = p.schema.meta.id.clone();
    let name = p.schema.meta.name.clone();
    let category = p.schema.meta.category.clone();
    let version = p.schema.meta.version.clone();
    let created_at = p.schema.meta.created_at.clone();
    let source_template = p.schema.meta.source_template.clone();
    let source_template_version = p.schema.meta.source_template_version.clone();
    let has_preset = p.schema.meta.has_preset;
    let sep = p.schema.separators.clone();
    let export = p.config.export.clone();
    let id_editable = p.state.is_pending();

    let ps = &mut st.project_settings;
    ps.open = true;
    ps.tab = 0;
    ps.project_id = id.clone();

    // apply buffer
    ps.id_buf = id.clone();
    ps.name_buf = name.clone();
    ps.category_buf = category.clone();
    ps.version_buf = version.clone();
    ps.id_error.clear();
    ps.id_editable = id_editable;
    ps.sep = sep.clone();
    ps.export = export.clone();

    // current snapshots
    ps.current_meta = Some(tablet_core::tblschema::SchemaMetadata {
        id: id.clone(),
        name: name.clone(),
        category: category.clone(),
        version: version.clone(),
        created_at,
        source_template,
        source_template_version,
        has_preset,
    });
    ps.current_sep = Some(sep.clone());
    ps.current_export = Some(export.clone());

    // reset modified flags
    ps.identity_tab_modified = false;
    ps.sep_tab_modified = false;
    ps.export_tab_modified = false;
}

/// 计算 id 校验消息。空 = 合法。
fn validate_id(st: &AppState, new_id: &str, old_id: &str) -> String {
    if new_id == old_id {
        return String::new();
    }
    if new_id.is_empty() {
        return "Project ID 不能为空".to_string();
    }
    if !is_valid_metadata_id(new_id) {
        return "ID 仅允许小写字母 / 数字 / _ / -，长度 1..=32".to_string();
    }
    let in_opened = st.engine.projects.iter().any(|p| p.schema.meta.id == new_id);
    let in_available = st.engine.available_projects.iter().any(|a| a.id == new_id);
    if in_opened || in_available {
        return format!("ID 已存在: {}", new_id);
    }
    String::new()
}

/// 检测身份 tab 是否修改
fn check_identity_modified(ps: &crate::state::ProjectSettingsState) -> bool {
    let Some(ref current) = ps.current_meta else { return false };
    current.id != ps.id_buf
        || current.name != ps.name_buf
        || current.category != ps.category_buf
        || current.version != ps.version_buf
}

/// 检测分隔符 tab 是否修改
fn check_sep_modified(ps: &crate::state::ProjectSettingsState) -> bool {
    let Some(ref current) = ps.current_sep else { return false };
    *current != ps.sep
}

/// 检测导出配置 tab 是否修改
fn check_export_modified(ps: &crate::state::ProjectSettingsState) -> bool {
    let Some(ref current) = ps.current_export else { return false };
    *current != ps.export
}

pub fn push(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let mut st = state.borrow_mut();
    // 实时校验 id（可能因 buf 改动）
    let id_err = validate_id(&st, &st.project_settings.id_buf.clone(), &st.project_settings.project_id.clone());
    st.project_settings.id_error = id_err.clone();

    // 计算修改标记
    st.project_settings.identity_tab_modified = check_identity_modified(&st.project_settings);
    st.project_settings.sep_tab_modified = check_sep_modified(&st.project_settings);
    st.project_settings.export_tab_modified = check_export_modified(&st.project_settings);

    let ps = &st.project_settings;

    ui_h.set_dlg_ps_open(ps.open);
    if !ps.open {
        return;
    }
    ui_h.set_ps_tab(ps.tab);

    // 推送修改标记
    ui_h.set_ps_identity_tab_modified(ps.identity_tab_modified);
    ui_h.set_ps_sep_tab_modified(ps.sep_tab_modified);
    ui_h.set_ps_export_tab_modified(ps.export_tab_modified);

    // 身份字段
    ui_h.set_ps_id_buf(ps.id_buf.clone().into());
    ui_h.set_ps_name_buf(ps.name_buf.clone().into());
    ui_h.set_ps_category_buf(ps.category_buf.clone().into());
    ui_h.set_ps_version_buf(ps.version_buf.clone().into());
    ui_h.set_ps_id_error(id_err.clone().into());
    ui_h.set_ps_id_editable(ps.id_editable);
    let can_confirm = id_err.is_empty();
    ui_h.set_ps_can_confirm(can_confirm);

    let s = &ps.sep;
    ui_h.set_ps_sep_tuple2(s.tuple2.clone().into());
    ui_h.set_ps_sep_tuple3(s.tuple3.clone().into());
    ui_h.set_ps_sep_tuple4(s.tuple4.clone().into());
    ui_h.set_ps_sep_list(s.list.clone().into());
    ui_h.set_ps_sep_set(s.set.clone().into());
    ui_h.set_ps_sep_map_kv(s.map.kv.clone().into());
    ui_h.set_ps_sep_map_entry(s.map.entry.clone().into());
    ui_h.set_ps_sep_lt2_tuple(s.list_tuple2.tuple.clone().into());
    ui_h.set_ps_sep_lt2_list(s.list_tuple2.list.clone().into());
    ui_h.set_ps_sep_lt3_tuple(s.list_tuple3.tuple.clone().into());
    ui_h.set_ps_sep_lt3_list(s.list_tuple3.list.clone().into());
    ui_h.set_ps_sep_lt4_tuple(s.list_tuple4.tuple.clone().into());
    ui_h.set_ps_sep_lt4_list(s.list_tuple4.list.clone().into());
    ui_h.set_ps_sep_mt2_kv(s.map_tuple2.kv.clone().into());
    ui_h.set_ps_sep_mt2_tuple(s.map_tuple2.tuple.clone().into());
    ui_h.set_ps_sep_mt2_entry(s.map_tuple2.entry.clone().into());
    ui_h.set_ps_sep_mt3_kv(s.map_tuple3.kv.clone().into());
    ui_h.set_ps_sep_mt3_tuple(s.map_tuple3.tuple.clone().into());
    ui_h.set_ps_sep_mt3_entry(s.map_tuple3.entry.clone().into());
    ui_h.set_ps_sep_mt4_kv(s.map_tuple4.kv.clone().into());
    ui_h.set_ps_sep_mt4_tuple(s.map_tuple4.tuple.clone().into());
    ui_h.set_ps_sep_mt4_entry(s.map_tuple4.entry.clone().into());
    ui_h.set_ps_sep_ml_kv(s.map_list.kv.clone().into());
    ui_h.set_ps_sep_ml_item(s.map_list.item.clone().into());
    ui_h.set_ps_sep_ml_entry(s.map_list.entry.clone().into());

    // 导出设置：程序启动时已修正配置，项目配置字段不应为 None，None 时使用 default 兜底即可
    use tablet_core::enums::*;

    let export = &ps.export;

    // Encoding
    let encoding = export.encoding.unwrap_or(Encoding::default());
    let encoding_idx = Encoding::all().iter().position(|&e| e == encoding).unwrap_or(0) as i32;
    ui_h.set_ps_export_encoding_index(encoding_idx);
    let encoding_opts: Vec<slint::SharedString> = Encoding::all_str().iter().map(|s| (*s).into()).collect();
    ui_h.set_ps_encoding_options(slint::ModelRc::new(slint::VecModel::from(encoding_opts)));

    // LineEnding
    let line_ending = export.line_ending.unwrap_or(LineEnding::default());
    let line_ending_idx = LineEnding::all().iter().position(|&l| l == line_ending).unwrap_or(0) as i32;
    ui_h.set_ps_export_line_ending_index(line_ending_idx);
    let line_ending_opts: Vec<slint::SharedString> = LineEnding::all_str().iter().map(|s| (*s).into()).collect();
    ui_h.set_ps_line_ending_options(slint::ModelRc::new(slint::VecModel::from(line_ending_opts)));

    // JsonEmptyAs
    let json_empty_as = export.json.as_ref().and_then(|j| j.empty_as).unwrap_or(JsonEmptyAs::default());
    let json_empty_as_idx = JsonEmptyAs::all().iter().position(|&e| e == json_empty_as).unwrap_or(0) as i32;
    ui_h.set_ps_export_json_empty_as_index(json_empty_as_idx);
    let json_empty_as_opts: Vec<slint::SharedString> = JsonEmptyAs::all_str().iter().map(|s| (*s).into()).collect();
    ui_h.set_ps_json_empty_as_options(slint::ModelRc::new(slint::VecModel::from(json_empty_as_opts)));

    // XmlEmptyAs
    let xml_empty_as = export.xml.as_ref().and_then(|x| x.empty_as).unwrap_or(XmlEmptyAs::default());
    let xml_empty_as_idx = XmlEmptyAs::all().iter().position(|&e| e == xml_empty_as).unwrap_or(0) as i32;
    ui_h.set_ps_export_xml_empty_as_index(xml_empty_as_idx);
    let xml_empty_as_opts: Vec<slint::SharedString> = XmlEmptyAs::all_str().iter().map(|s| (*s).into()).collect();
    ui_h.set_ps_xml_empty_as_options(slint::ModelRc::new(slint::VecModel::from(xml_empty_as_opts)));

    // CppJsonLib
    let cpp_json_lib = export.server.as_ref()
        .and_then(|s| s.cpp.as_ref())
        .and_then(|c| c.json_lib)
        .unwrap_or(CppJsonLib::default());
    let cpp_json_lib_idx = CppJsonLib::all().iter().position(|&l| l == cpp_json_lib).unwrap_or(0) as i32;
    ui_h.set_ps_export_cpp_json_lib_index(cpp_json_lib_idx);
    let cpp_json_lib_opts: Vec<slint::SharedString> = CppJsonLib::all_str().iter().map(|s| (*s).into()).collect();
    ui_h.set_ps_cpp_json_lib_options(slint::ModelRc::new(slint::VecModel::from(cpp_json_lib_opts)));

    // 字符串字段推送
    ui_h.set_ps_export_server_data_output(
        export.server.as_ref().and_then(|s| s.data_output.as_ref()).cloned().unwrap_or_default().into()
    );
    ui_h.set_ps_export_java_package(
        export.server.as_ref().and_then(|s| s.java.as_ref()).and_then(|j| j.package.as_ref()).cloned().unwrap_or_default().into()
    );
    ui_h.set_ps_export_java_code_output(
        export.server.as_ref().and_then(|s| s.java.as_ref()).and_then(|j| j.code_output.as_ref()).cloned().unwrap_or_default().into()
    );
    ui_h.set_ps_export_go_package(
        export.server.as_ref().and_then(|s| s.go.as_ref()).and_then(|g| g.package.as_ref()).cloned().unwrap_or_default().into()
    );
    ui_h.set_ps_export_go_code_output(
        export.server.as_ref().and_then(|s| s.go.as_ref()).and_then(|g| g.code_output.as_ref()).cloned().unwrap_or_default().into()
    );
    ui_h.set_ps_export_cpp_namespace(
        export.server.as_ref().and_then(|s| s.cpp.as_ref()).and_then(|c| c.namespace.as_ref()).cloned().unwrap_or_default().into()
    );
    ui_h.set_ps_export_cpp_code_output(
        export.server.as_ref().and_then(|s| s.cpp.as_ref()).and_then(|c| c.code_output.as_ref()).cloned().unwrap_or_default().into()
    );
    ui_h.set_ps_export_csharp_dotnet_namespace(
        export.server.as_ref().and_then(|s| s.csharp_dotnet.as_ref()).and_then(|c| c.namespace.as_ref()).cloned().unwrap_or_default().into()
    );
    ui_h.set_ps_export_csharp_dotnet_code_output(
        export.server.as_ref().and_then(|s| s.csharp_dotnet.as_ref()).and_then(|c| c.code_output.as_ref()).cloned().unwrap_or_default().into()
    );
    ui_h.set_ps_export_typescript_server_code_output(
        export.server.as_ref().and_then(|s| s.typescript.as_ref()).and_then(|t| t.output.as_ref()).cloned().unwrap_or_default().into()
    );
    ui_h.set_ps_export_lua_output(
        export.client.as_ref().and_then(|c| c.lua.as_ref()).and_then(|l| l.output.as_ref()).cloned().unwrap_or_default().into()
    );
    ui_h.set_ps_export_gdscript_output(
        export.client.as_ref().and_then(|c| c.gdscript.as_ref()).and_then(|g| g.output.as_ref()).cloned().unwrap_or_default().into()
    );
    ui_h.set_ps_export_typescript_client_output(
        export.client.as_ref().and_then(|c| c.typescript.as_ref()).and_then(|t| t.output.as_ref()).cloned().unwrap_or_default().into()
    );
    ui_h.set_ps_export_csharp_unity_namespace(
        export.client.as_ref().and_then(|c| c.csharp_unity.as_ref()).and_then(|u| u.namespace.as_ref()).cloned().unwrap_or_default().into()
    );
    ui_h.set_ps_export_csharp_unity_code_output(
        export.client.as_ref().and_then(|c| c.csharp_unity.as_ref()).and_then(|u| u.code_output.as_ref()).cloned().unwrap_or_default().into()
    );
    ui_h.set_ps_export_csharp_godot_namespace(
        export.client.as_ref().and_then(|c| c.csharp_godot.as_ref()).and_then(|g| g.namespace.as_ref()).cloned().unwrap_or_default().into()
    );
    ui_h.set_ps_export_csharp_godot_code_output(
        export.client.as_ref().and_then(|c| c.csharp_godot.as_ref()).and_then(|g| g.code_output.as_ref()).cloned().unwrap_or_default().into()
    );
}

/// `# @sep` 行 key → 写到 SeparatorsSection 的对应字段。
fn apply_sep_kv(sep: &mut SeparatorsSection, key: &str, value: &str) {
    if let Some(k) = SepKey::from_directive_key(key) {
        k.set(sep, value.to_string());
    }
}

/// 导出配置字段编辑：从字符串值更新到 ExportConfig 结构
fn apply_export_field(export: &mut tablet_core::model::ExportConfig, key: &str, value: &str) {
    use std::str::FromStr;
    use tablet_core::enums::*;

    match key {
        "encoding" => {
            if let Ok(e) = Encoding::from_str(value) {
                export.encoding = Some(e);
            }
        }
        "line_ending" => {
            if let Ok(l) = LineEnding::from_str(value) {
                export.line_ending = Some(l);
            }
        }
        "json_empty_as" => {
            if export.json.is_none() {
                export.json = Some(Default::default());
            }
            if let Some(ref mut json) = export.json {
                if let Ok(e) = JsonEmptyAs::from_str(value) {
                    json.empty_as = Some(e);
                }
            }
        }
        "xml_empty_as" => {
            if export.xml.is_none() {
                export.xml = Some(Default::default());
            }
            if let Some(ref mut xml) = export.xml {
                if let Ok(e) = XmlEmptyAs::from_str(value) {
                    xml.empty_as = Some(e);
                }
            }
        }
        "cpp_json_lib" => {
            if export.server.is_none() {
                export.server = Some(Default::default());
            }
            if let Some(ref mut server) = export.server {
                if server.cpp.is_none() {
                    server.cpp = Some(Default::default());
                }
                if let Some(ref mut cpp) = server.cpp {
                    if let Ok(lib) = CppJsonLib::from_str(value) {
                        cpp.json_lib = Some(lib);
                    }
                }
            }
        }
        // 字符串字段
        "server_data_output" => {
            if export.server.is_none() {
                export.server = Some(Default::default());
            }
            if let Some(ref mut server) = export.server {
                server.data_output = if value.is_empty() { None } else { Some(value.to_string()) };
            }
        }
        "java_package" => {
            if export.server.is_none() {
                export.server = Some(Default::default());
            }
            if let Some(ref mut server) = export.server {
                if server.java.is_none() {
                    server.java = Some(Default::default());
                }
                if let Some(ref mut java) = server.java {
                    java.package = if value.is_empty() { None } else { Some(value.to_string()) };
                }
            }
        }
        "java_code_output" => {
            if export.server.is_none() {
                export.server = Some(Default::default());
            }
            if let Some(ref mut server) = export.server {
                if server.java.is_none() {
                    server.java = Some(Default::default());
                }
                if let Some(ref mut java) = server.java {
                    java.code_output = if value.is_empty() { None } else { Some(value.to_string()) };
                }
            }
        }
        "go_package" => {
            if export.server.is_none() {
                export.server = Some(Default::default());
            }
            if let Some(ref mut server) = export.server {
                if server.go.is_none() {
                    server.go = Some(Default::default());
                }
                if let Some(ref mut go) = server.go {
                    go.package = if value.is_empty() { None } else { Some(value.to_string()) };
                }
            }
        }
        "go_code_output" => {
            if export.server.is_none() {
                export.server = Some(Default::default());
            }
            if let Some(ref mut server) = export.server {
                if server.go.is_none() {
                    server.go = Some(Default::default());
                }
                if let Some(ref mut go) = server.go {
                    go.code_output = if value.is_empty() { None } else { Some(value.to_string()) };
                }
            }
        }
        "cpp_namespace" => {
            if export.server.is_none() {
                export.server = Some(Default::default());
            }
            if let Some(ref mut server) = export.server {
                if server.cpp.is_none() {
                    server.cpp = Some(Default::default());
                }
                if let Some(ref mut cpp) = server.cpp {
                    cpp.namespace = if value.is_empty() { None } else { Some(value.to_string()) };
                }
            }
        }
        "cpp_code_output" => {
            if export.server.is_none() {
                export.server = Some(Default::default());
            }
            if let Some(ref mut server) = export.server {
                if server.cpp.is_none() {
                    server.cpp = Some(Default::default());
                }
                if let Some(ref mut cpp) = server.cpp {
                    cpp.code_output = if value.is_empty() { None } else { Some(value.to_string()) };
                }
            }
        }
        "csharp_dotnet_namespace" => {
            if export.server.is_none() {
                export.server = Some(Default::default());
            }
            if let Some(ref mut server) = export.server {
                if server.csharp_dotnet.is_none() {
                    server.csharp_dotnet = Some(Default::default());
                }
                if let Some(ref mut cs) = server.csharp_dotnet {
                    cs.namespace = if value.is_empty() { None } else { Some(value.to_string()) };
                }
            }
        }
        "csharp_dotnet_code_output" => {
            if export.server.is_none() {
                export.server = Some(Default::default());
            }
            if let Some(ref mut server) = export.server {
                if server.csharp_dotnet.is_none() {
                    server.csharp_dotnet = Some(Default::default());
                }
                if let Some(ref mut cs) = server.csharp_dotnet {
                    cs.code_output = if value.is_empty() { None } else { Some(value.to_string()) };
                }
            }
        }
        "typescript_server_code_output" => {
            if export.server.is_none() {
                export.server = Some(Default::default());
            }
            if let Some(ref mut server) = export.server {
                if server.typescript.is_none() {
                    server.typescript = Some(Default::default());
                }
                if let Some(ref mut ts) = server.typescript {
                    ts.output = if value.is_empty() { None } else { Some(value.to_string()) };
                }
            }
        }
        "lua_output" => {
            if export.client.is_none() {
                export.client = Some(Default::default());
            }
            if let Some(ref mut client) = export.client {
                if client.lua.is_none() {
                    client.lua = Some(Default::default());
                }
                if let Some(ref mut lua) = client.lua {
                    lua.output = if value.is_empty() { None } else { Some(value.to_string()) };
                }
            }
        }
        "gdscript_output" => {
            if export.client.is_none() {
                export.client = Some(Default::default());
            }
            if let Some(ref mut client) = export.client {
                if client.gdscript.is_none() {
                    client.gdscript = Some(Default::default());
                }
                if let Some(ref mut gd) = client.gdscript {
                    gd.output = if value.is_empty() { None } else { Some(value.to_string()) };
                }
            }
        }
        "typescript_client_output" => {
            if export.client.is_none() {
                export.client = Some(Default::default());
            }
            if let Some(ref mut client) = export.client {
                if client.typescript.is_none() {
                    client.typescript = Some(Default::default());
                }
                if let Some(ref mut ts) = client.typescript {
                    ts.output = if value.is_empty() { None } else { Some(value.to_string()) };
                }
            }
        }
        "csharp_unity_namespace" => {
            if export.client.is_none() {
                export.client = Some(Default::default());
            }
            if let Some(ref mut client) = export.client {
                if client.csharp_unity.is_none() {
                    client.csharp_unity = Some(Default::default());
                }
                if let Some(ref mut cs) = client.csharp_unity {
                    cs.namespace = if value.is_empty() { None } else { Some(value.to_string()) };
                }
            }
        }
        "csharp_unity_code_output" => {
            if export.client.is_none() {
                export.client = Some(Default::default());
            }
            if let Some(ref mut client) = export.client {
                if client.csharp_unity.is_none() {
                    client.csharp_unity = Some(Default::default());
                }
                if let Some(ref mut cs) = client.csharp_unity {
                    cs.code_output = if value.is_empty() { None } else { Some(value.to_string()) };
                }
            }
        }
        "csharp_godot_namespace" => {
            if export.client.is_none() {
                export.client = Some(Default::default());
            }
            if let Some(ref mut client) = export.client {
                if client.csharp_godot.is_none() {
                    client.csharp_godot = Some(Default::default());
                }
                if let Some(ref mut cs) = client.csharp_godot {
                    cs.namespace = if value.is_empty() { None } else { Some(value.to_string()) };
                }
            }
        }
        "csharp_godot_code_output" => {
            if export.client.is_none() {
                export.client = Some(Default::default());
            }
            if let Some(ref mut client) = export.client {
                if client.csharp_godot.is_none() {
                    client.csharp_godot = Some(Default::default());
                }
                if let Some(ref mut cs) = client.csharp_godot {
                    cs.code_output = if value.is_empty() { None } else { Some(value.to_string()) };
                }
            }
        }
        _ => {}
    }
}

/// 「确定」逻辑。更新所有字段到内存，标记 dirty，等"保存项目"才写盘。
fn run(state: &Rc<RefCell<AppState>>) {
    // 取出 buf，先放下 borrow
    let (old_id, new_id, name, category, version, sep, export) = {
        let st = state.borrow();
        let ps = &st.project_settings;
        (
            ps.project_id.clone(),
            ps.id_buf.clone(),
            ps.name_buf.clone(),
            ps.category_buf.clone(),
            ps.version_buf.clone(),
            ps.sep.clone(),
            ps.export.clone(),
        )
    };

    // 更新所有字段到内存（id / name / category / version / separators），
    // 只标记 dirty，等"保存项目"才写盘。
    let mut has_changes = false;
    {
        let mut st = state.borrow_mut();
        if let Some(p) = st.engine.find_project_mut(&old_id) {
            if p.schema.meta.id != new_id {
                p.schema.meta.id = new_id.clone();
                // 未落盘项目改 id 时需同步更新 project_root 路径
                // 子节点路径会在首次保存时由 save_project_files 自动推演
                if p.state.is_pending() {
                    p.project_root = p.workdir.join("projects").join(&new_id);
                }
                has_changes = true;
            }
            if p.schema.meta.name != name {
                p.schema.meta.name = name.clone();
                has_changes = true;
            }
            if p.schema.meta.category != category {
                p.schema.meta.category = category.clone();
                has_changes = true;
            }
            if p.schema.meta.version != version {
                p.schema.meta.version = version.clone();
                has_changes = true;
            }
            if p.schema.separators != sep {
                p.schema.separators = sep.clone();
                has_changes = true;
            }
            if p.config.export != export {
                p.config.export = export.clone();
                has_changes = true;
            }
            if has_changes {
                p.schema_dirty = true;
            }
        }
    }

    if has_changes {
        let mut st = state.borrow_mut();

        // ID 改变时，需要同步更新 engine 内部的所有索引结构
        let id_changed = old_id != new_id;
        if id_changed {
            // 更新 available_projects 列表中的 id
            if let Some(avail) = st.engine.available_projects.iter_mut().find(|a| a.id == old_id) {
                avail.id = new_id.clone();
            }
            // 更新 opened_projects 列表
            for opened_id in &mut st.engine.global_config.project_management.opened_projects {
                if opened_id == &old_id {
                    *opened_id = new_id.clone();
                }
            }
            // 更新 last_project（如果是当前项目）
            if st.engine.global_config.project_management.last_project == old_id {
                st.engine.global_config.project_management.last_project = new_id.clone();
            }
            // 更新 project_order 列表
            for order_id in &mut st.engine.global_config.project_management.project_order {
                if order_id == &old_id {
                    *order_id = new_id.clone();
                }
            }
            // 更新 validation_errors 中的 project_id
            let old_errors: Vec<_> = st.engine.validation_errors.iter()
                .filter(|(pid, _, _, _, _)| pid == &old_id)
                .cloned()
                .collect();
            for (_, g, n, r, c) in old_errors {
                st.engine.validation_errors.remove(&(old_id.clone(), g.clone(), n.clone(), r, c));
                st.engine.validation_errors.insert((new_id.clone(), g, n, r, c));
            }
        }

        // schema 改了（特别是 separators）→ 重新生成合并后的 ProjectConfig
        // 注意：ID 改变后要用新 ID 查找
        let target_id = if id_changed { &new_id } else { &old_id };
        st.engine.remerge_project_config(target_id);

        // 分隔符变了要全表重校验（active scope）：临时切到目标 project 跑 revalidate_all 再切回。
        let prev_active = st.engine.active_project_id().map(|s| s.to_string());
        if st.engine.set_active_by_id(target_id) {
            st.engine.revalidate_all();
        }
        match prev_active {
            Some(id) => { st.engine.set_active_by_id(&id); }
            None => st.engine.set_active_none(),
        }

        st.engine.ui_log(format!("[项目设置] 已应用修改: {}（需保存项目才写盘）", target_id));
    }
}

pub fn wire(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    // tab 切换
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ps_tab_changed(move |i| {
            s.borrow_mut().project_settings.tab = i;
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    // 身份字段 edited
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ps_id_edited(move |v| {
            s.borrow_mut().project_settings.id_buf = v.to_string();
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ps_name_edited(move |v| {
            s.borrow_mut().project_settings.name_buf = v.to_string();
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ps_category_edited(move |v| {
            s.borrow_mut().project_settings.category_buf = v.to_string();
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ps_version_edited(move |v| {
            s.borrow_mut().project_settings.version_buf = v.to_string();
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    // 分隔符任一字段 edited（slint 端统一通过 sep-edited(key, value) 上来）
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ps_sep_edited(move |key, value| {
            apply_sep_kv(&mut s.borrow_mut().project_settings.sep, &key, &value);
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    // 导出配置字段 edited（slint 端统一通过 export-field-edited(key, value) 上来）
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ps_export_field_edited(move |key, value| {
            apply_export_field(&mut s.borrow_mut().project_settings.export, &key, &value);
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    // 应用模板（分隔符 tab）：应用当前程序内存中 globalConfig 的模板值
    // 注意：不是恢复到 SeparatorsSection::default()，default() 仅用于 globalConfig 自身初始化
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ps_reset_sep(move || {
            let template_sep = {
                let st = s.borrow();
                st.engine.global_config.separators.clone()
            };
            s.borrow_mut().project_settings.sep = template_sep;
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    // 应用模板（导出配置 tab）：应用当前程序内存中 globalConfig 的模板值
    // 注意：不是恢复到各枚举的 ::default()，default() 仅用于 globalConfig 自身初始化
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ps_reset_export(move || {
            let template_export = {
                let st = s.borrow();
                st.engine.global_config.export.clone().unwrap_or_default()
            };
            s.borrow_mut().project_settings.export = template_export;
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    // 撤销按钮
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ps_undo_clicked(move || {
            s.borrow_mut().project_settings.undo();
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    // 确定 / 取消
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ps_confirm(move || {
            // 防止 id 校验未通过时被 enter 触发
            let ok = {
                let st = s.borrow();
                st.project_settings.id_error.is_empty()
            };
            if !ok {
                if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
                return;
            }
            run(&s);
            s.borrow_mut().project_settings.close();
            if let Some(ui_h) = weak.upgrade() {
                push(&ui_h, &s);
                refresh::after_tree_change(&ui_h, &s);
            }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ps_cancel(move || {
            s.borrow_mut().project_settings.close();
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
}
