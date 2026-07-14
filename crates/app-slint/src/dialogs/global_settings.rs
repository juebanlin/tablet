// 全局设置对话框：UI设置 / 分隔符 / 导出设置 3 tab。
//
// 数据范式：current + apply 模式
// - current: engine.global_config（当前已保存的内存值，等于硬盘值）
// - apply: global_settings 各 buf 字段（待应用的编辑缓冲）
// - current_snapshot: 打开对话框时 current 的快照，用于撤销和对比
// - modified: 后端对比计算（apply != current_snapshot）
//
// 与项目设置的区别：点击「确定」后立即写 tablet.toml，不是标记 dirty。
// 修改全局配置后会重新合并所有已打开项目的配置，分隔符改变时重新验证数据。

use std::cell::RefCell;
use std::rc::Rc;
use std::str::FromStr;

use slint::ComponentHandle;
use tablet_core::types::{SepKey, SeparatorsSection};

use crate::state::AppState;
use crate::{refresh, AppWindow};

/// 打开全局设置对话框：从 engine.global_config 拷贝数据到 apply buffer，并保存 current 快照
pub fn open(state: &Rc<RefCell<AppState>>, ui_h: &AppWindow) {
    let mut st = state.borrow_mut();

    // current: 当前已保存的值（等于硬盘值）
    let current = st.engine.global_config.clone();

    // 调试日志：打印加载的 UI 配置
    if let Some(ref ui) = current.ui {
        st.engine.ui_log(format!(
            "[全局设置] 加载的 UI 配置: picker_trigger_header={:?}, picker_trigger_data={:?}",
            ui.picker_trigger_header,
            ui.picker_trigger_data
        ));
    } else {
        st.engine.ui_log("[全局设置] UI 配置为 None，使用默认值".to_string());
    }

    // 初始化 apply buffer：直接克隆结构体
    let gs = &mut st.global_settings;
    gs.open = true;
    gs.tab = 0;
    gs.ui_tab_modified = false;
    gs.sep_tab_modified = false;
    gs.export_tab_modified = false;
    gs.ui = current.ui.clone().unwrap_or_default();
    gs.sep = current.separators.clone();
    gs.export = current.export.clone().unwrap_or_default();
    gs.current_snapshot = Some(current);

    // 保存需要打印的调试值
    let debug_header = gs.ui.picker_trigger_header.as_str().to_string();
    let debug_data = gs.ui.picker_trigger_data.as_str().to_string();

    // 初始化枚举选项列表，并立即设置当前值，避免 ComboBox 默认选择第一项
    // 注意：必须先设置 index 再设置 model，否则设置 model 时会触发 selected 回调覆盖后端值
    use tablet_core::enums::*;

    // LogLevel：先计算 index，再设置 index，最后设置 model
    let log_level = gs.ui.log_level.unwrap_or(LogLevel::default());
    let log_level_idx = LogLevel::all().iter().position(|&l| l == log_level).unwrap_or(0) as i32;
    ui_h.set_gs_ui_log_level_index(log_level_idx);
    let log_level_opts: Vec<slint::SharedString> = LogLevel::all_str().iter().map(|s| (*s).into()).collect();
    ui_h.set_gs_log_level_options(slint::ModelRc::new(slint::VecModel::from(log_level_opts)));

    // PickerTrigger：先计算 index，再设置 index，最后设置 model
    let header_idx = PickerTrigger::all().iter().position(|&t| t == gs.ui.picker_trigger_header).unwrap_or(0) as i32;
    let data_idx = PickerTrigger::all().iter().position(|&t| t == gs.ui.picker_trigger_data).unwrap_or(0) as i32;
    ui_h.set_gs_ui_picker_trigger_header_index(header_idx);
    ui_h.set_gs_ui_picker_trigger_data_index(data_idx);
    let picker_trigger_opts: Vec<slint::SharedString> = PickerTrigger::all_str().iter().map(|s| (*s).into()).collect();
    ui_h.set_gs_picker_trigger_options(slint::ModelRc::new(slint::VecModel::from(picker_trigger_opts)));

    // RefPickerStrategy：先计算 index，再设置 index，最后设置 model
    let ref_picker_strategy_idx = RefPickerStrategy::all().iter().position(|&s| s == gs.ui.ref_picker.default_strategy).unwrap_or(0) as i32;
    ui_h.set_gs_ui_ref_picker_strategy_index(ref_picker_strategy_idx);
    let ref_picker_strategy_opts: Vec<slint::SharedString> = RefPickerStrategy::all_str().iter().map(|s| (*s).into()).collect();
    ui_h.set_gs_ref_picker_strategy_options(slint::ModelRc::new(slint::VecModel::from(ref_picker_strategy_opts)));

    // Encoding：先计算 index，再设置 index，最后设置 model
    let encoding = gs.export.encoding.unwrap_or(Encoding::default());
    let encoding_idx = Encoding::all().iter().position(|&e| e == encoding).unwrap_or(0) as i32;
    ui_h.set_gs_export_encoding_index(encoding_idx);
    let encoding_opts: Vec<slint::SharedString> = Encoding::all_str().iter().map(|s| (*s).into()).collect();
    ui_h.set_gs_encoding_options(slint::ModelRc::new(slint::VecModel::from(encoding_opts)));

    // LineEnding：先计算 index，再设置 index，最后设置 model
    let line_ending = gs.export.line_ending.unwrap_or(LineEnding::default());
    let line_ending_idx = LineEnding::all().iter().position(|&l| l == line_ending).unwrap_or(0) as i32;
    ui_h.set_gs_export_line_ending_index(line_ending_idx);
    let line_ending_opts: Vec<slint::SharedString> = LineEnding::all_str().iter().map(|s| (*s).into()).collect();
    ui_h.set_gs_line_ending_options(slint::ModelRc::new(slint::VecModel::from(line_ending_opts)));

    // JsonEmptyAs：先设置 index，再设置 model（与 Encoding/LineEnding 保持一致）
    let json_empty_as = gs.export.json.as_ref().and_then(|j| j.empty_as).unwrap_or(JsonEmptyAs::default());
    let json_empty_as_idx = JsonEmptyAs::all().iter().position(|&e| e == json_empty_as).unwrap_or(0) as i32;
    ui_h.set_gs_export_json_empty_as_index(json_empty_as_idx);
    let json_empty_as_opts: Vec<slint::SharedString> = JsonEmptyAs::all_str().iter().map(|s| (*s).into()).collect();
    ui_h.set_gs_json_empty_as_options(slint::ModelRc::new(slint::VecModel::from(json_empty_as_opts)));

    // XmlEmptyAs：先设置 index，再设置 model（与 Encoding/LineEnding 保持一致）
    let xml_empty_as = gs.export.xml.as_ref().and_then(|x| x.empty_as).unwrap_or(XmlEmptyAs::default());
    let xml_empty_as_idx = XmlEmptyAs::all().iter().position(|&e| e == xml_empty_as).unwrap_or(0) as i32;
    ui_h.set_gs_export_xml_empty_as_index(xml_empty_as_idx);
    let xml_empty_as_opts: Vec<slint::SharedString> = XmlEmptyAs::all_str().iter().map(|s| (*s).into()).collect();
    ui_h.set_gs_xml_empty_as_options(slint::ModelRc::new(slint::VecModel::from(xml_empty_as_opts)));

    // CppJsonLib：先设置 index，再设置 model（与 Encoding/LineEnding 保持一致）
    let cpp_json_lib = gs.export.server.as_ref()
        .and_then(|s| s.cpp.as_ref())
        .and_then(|c| c.json_lib)
        .unwrap_or(CppJsonLib::default());
    let cpp_json_lib_idx = CppJsonLib::all().iter().position(|&l| l == cpp_json_lib).unwrap_or(0) as i32;
    ui_h.set_gs_export_cpp_json_lib_index(cpp_json_lib_idx);
    let cpp_json_lib_opts: Vec<slint::SharedString> = CppJsonLib::all_str().iter().map(|s| (*s).into()).collect();
    ui_h.set_gs_cpp_json_lib_options(slint::ModelRc::new(slint::VecModel::from(cpp_json_lib_opts)));

    // 调试日志：打印 apply buffer 的值
    st.engine.ui_log(format!(
        "[全局设置] open 设置初始值: picker_trigger_header={}, picker_trigger_data={}",
        debug_header, debug_data
    ));
}

pub fn push(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let st = state.borrow();
    let gs = &st.global_settings;

    ui_h.set_dlg_gs_open(gs.open);
    if !gs.open {
        return;
    }
    ui_h.set_gs_tab(gs.tab);
    ui_h.set_gs_ui_tab_modified(gs.ui_tab_modified);
    ui_h.set_gs_sep_tab_modified(gs.sep_tab_modified);
    ui_h.set_gs_export_tab_modified(gs.export_tab_modified);

    // UI 设置
    ui_h.set_gs_ui_auto_commit_on_blur(gs.ui.auto_commit_on_blur);
    ui_h.set_gs_ui_realtime_validate(gs.ui.realtime_validate);

    let log_level = gs.ui.log_level.unwrap_or(tablet_core::enums::LogLevel::default());
    let log_level_idx = tablet_core::enums::LogLevel::all().iter().position(|&l| l == log_level).unwrap_or(0) as i32;
    ui_h.set_gs_ui_log_level_index(log_level_idx);

    // 使用 current-index 而不是 current-value
    let header_idx = tablet_core::enums::PickerTrigger::all().iter().position(|&t| t == gs.ui.picker_trigger_header).unwrap_or(0) as i32;
    let data_idx = tablet_core::enums::PickerTrigger::all().iter().position(|&t| t == gs.ui.picker_trigger_data).unwrap_or(0) as i32;
    ui_h.set_gs_ui_picker_trigger_header_index(header_idx);
    ui_h.set_gs_ui_picker_trigger_data_index(data_idx);
    ui_h.set_gs_ui_show_meta_id(gs.ui.show_meta_id);
    ui_h.set_gs_ui_constant_ref_allowed(gs.ui.constant_ref_allowed);

    let ref_picker_strategy_idx = tablet_core::enums::RefPickerStrategy::all().iter().position(|&s| s == gs.ui.ref_picker.default_strategy).unwrap_or(0) as i32;
    ui_h.set_gs_ui_ref_picker_strategy_index(ref_picker_strategy_idx);

    // 分隔符（25 个）
    push_separators(ui_h, &gs.sep);

    // 导出设置
    let encoding = gs.export.encoding.unwrap_or(tablet_core::enums::Encoding::default());
    let encoding_idx = tablet_core::enums::Encoding::all().iter().position(|&e| e == encoding).unwrap_or(0) as i32;
    ui_h.set_gs_export_encoding_index(encoding_idx);

    let line_ending = gs.export.line_ending.unwrap_or(tablet_core::enums::LineEnding::default());
    let line_ending_idx = tablet_core::enums::LineEnding::all().iter().position(|&l| l == line_ending).unwrap_or(0) as i32;
    ui_h.set_gs_export_line_ending_index(line_ending_idx);

    // JSON/XML 空值处理
    let json_empty_as = gs.export.json.as_ref().and_then(|j| j.empty_as).unwrap_or(tablet_core::enums::JsonEmptyAs::default());
    let json_empty_as_idx = tablet_core::enums::JsonEmptyAs::all().iter().position(|&e| e == json_empty_as).unwrap_or(0) as i32;
    ui_h.set_gs_export_json_empty_as_index(json_empty_as_idx);

    let xml_empty_as = gs.export.xml.as_ref().and_then(|x| x.empty_as).unwrap_or(tablet_core::enums::XmlEmptyAs::default());
    let xml_empty_as_idx = tablet_core::enums::XmlEmptyAs::all().iter().position(|&e| e == xml_empty_as).unwrap_or(0) as i32;
    ui_h.set_gs_export_xml_empty_as_index(xml_empty_as_idx);

    // Server
    ui_h.set_gs_export_server_data_output(
        gs.export.server.as_ref().and_then(|s| s.data_output.as_ref()).cloned().unwrap_or_default().into()
    );
    ui_h.set_gs_export_java_package(
        gs.export.server.as_ref().and_then(|s| s.java.as_ref()).and_then(|j| j.package.as_ref()).cloned().unwrap_or_default().into()
    );
    ui_h.set_gs_export_java_code_output(
        gs.export.server.as_ref().and_then(|s| s.java.as_ref()).and_then(|j| j.code_output.as_ref()).cloned().unwrap_or_default().into()
    );
    ui_h.set_gs_export_go_package(
        gs.export.server.as_ref().and_then(|s| s.go.as_ref()).and_then(|g| g.package.as_ref()).cloned().unwrap_or_default().into()
    );
    ui_h.set_gs_export_go_code_output(
        gs.export.server.as_ref().and_then(|s| s.go.as_ref()).and_then(|g| g.code_output.as_ref()).cloned().unwrap_or_default().into()
    );
    ui_h.set_gs_export_cpp_namespace(
        gs.export.server.as_ref().and_then(|s| s.cpp.as_ref()).and_then(|c| c.namespace.as_ref()).cloned().unwrap_or_default().into()
    );
    ui_h.set_gs_export_cpp_code_output(
        gs.export.server.as_ref().and_then(|s| s.cpp.as_ref()).and_then(|c| c.code_output.as_ref()).cloned().unwrap_or_default().into()
    );
    let cpp_json_lib = gs.export.server.as_ref().and_then(|s| s.cpp.as_ref()).and_then(|c| c.json_lib).unwrap_or(tablet_core::enums::CppJsonLib::default());
    let cpp_json_lib_idx = tablet_core::enums::CppJsonLib::all().iter().position(|&l| l == cpp_json_lib).unwrap_or(0) as i32;
    ui_h.set_gs_export_cpp_json_lib_index(cpp_json_lib_idx);

    ui_h.set_gs_export_csharp_dotnet_namespace(
        gs.export.server.as_ref().and_then(|s| s.csharp_dotnet.as_ref()).and_then(|c| c.namespace.as_ref()).cloned().unwrap_or_default().into()
    );
    ui_h.set_gs_export_csharp_dotnet_code_output(
        gs.export.server.as_ref().and_then(|s| s.csharp_dotnet.as_ref()).and_then(|c| c.code_output.as_ref()).cloned().unwrap_or_default().into()
    );
    ui_h.set_gs_export_typescript_server_code_output(
        gs.export.server.as_ref().and_then(|s| s.typescript.as_ref()).and_then(|t| t.output.as_ref()).cloned().unwrap_or_default().into()
    );

    // Client
    ui_h.set_gs_export_lua_output(
        gs.export.client.as_ref().and_then(|c| c.lua.as_ref()).and_then(|l| l.output.as_ref()).cloned().unwrap_or_default().into()
    );
    ui_h.set_gs_export_gdscript_output(
        gs.export.client.as_ref().and_then(|c| c.gdscript.as_ref()).and_then(|g| g.output.as_ref()).cloned().unwrap_or_default().into()
    );
    ui_h.set_gs_export_typescript_client_output(
        gs.export.client.as_ref().and_then(|c| c.typescript.as_ref()).and_then(|t| t.output.as_ref()).cloned().unwrap_or_default().into()
    );
    ui_h.set_gs_export_csharp_unity_namespace(
        gs.export.client.as_ref().and_then(|c| c.csharp_unity.as_ref()).and_then(|u| u.namespace.as_ref()).cloned().unwrap_or_default().into()
    );
    ui_h.set_gs_export_csharp_unity_code_output(
        gs.export.client.as_ref().and_then(|c| c.csharp_unity.as_ref()).and_then(|u| u.code_output.as_ref()).cloned().unwrap_or_default().into()
    );
    ui_h.set_gs_export_csharp_godot_namespace(
        gs.export.client.as_ref().and_then(|c| c.csharp_godot.as_ref()).and_then(|g| g.namespace.as_ref()).cloned().unwrap_or_default().into()
    );
    ui_h.set_gs_export_csharp_godot_code_output(
        gs.export.client.as_ref().and_then(|c| c.csharp_godot.as_ref()).and_then(|g| g.code_output.as_ref()).cloned().unwrap_or_default().into()
    );
}

fn push_separators(ui_h: &AppWindow, sep: &SeparatorsSection) {
    ui_h.set_gs_sep_tuple2(SepKey::Tuple2.get(sep).into());
    ui_h.set_gs_sep_tuple3(SepKey::Tuple3.get(sep).into());
    ui_h.set_gs_sep_tuple4(SepKey::Tuple4.get(sep).into());
    ui_h.set_gs_sep_list(SepKey::List.get(sep).into());
    ui_h.set_gs_sep_set(SepKey::Set.get(sep).into());
    ui_h.set_gs_sep_map_kv(SepKey::MapKv.get(sep).into());
    ui_h.set_gs_sep_map_entry(SepKey::MapEntry.get(sep).into());
    ui_h.set_gs_sep_lt2_tuple(SepKey::ListTuple2Tuple.get(sep).into());
    ui_h.set_gs_sep_lt2_list(SepKey::ListTuple2List.get(sep).into());
    ui_h.set_gs_sep_lt3_tuple(SepKey::ListTuple3Tuple.get(sep).into());
    ui_h.set_gs_sep_lt3_list(SepKey::ListTuple3List.get(sep).into());
    ui_h.set_gs_sep_lt4_tuple(SepKey::ListTuple4Tuple.get(sep).into());
    ui_h.set_gs_sep_lt4_list(SepKey::ListTuple4List.get(sep).into());
    ui_h.set_gs_sep_mt2_kv(SepKey::MapTuple2Kv.get(sep).into());
    ui_h.set_gs_sep_mt2_tuple(SepKey::MapTuple2Tuple.get(sep).into());
    ui_h.set_gs_sep_mt2_entry(SepKey::MapTuple2Entry.get(sep).into());
    ui_h.set_gs_sep_mt3_kv(SepKey::MapTuple3Kv.get(sep).into());
    ui_h.set_gs_sep_mt3_tuple(SepKey::MapTuple3Tuple.get(sep).into());
    ui_h.set_gs_sep_mt3_entry(SepKey::MapTuple3Entry.get(sep).into());
    ui_h.set_gs_sep_mt4_kv(SepKey::MapTuple4Kv.get(sep).into());
    ui_h.set_gs_sep_mt4_tuple(SepKey::MapTuple4Tuple.get(sep).into());
    ui_h.set_gs_sep_mt4_entry(SepKey::MapTuple4Entry.get(sep).into());
    ui_h.set_gs_sep_ml_kv(SepKey::MapListKv.get(sep).into());
    ui_h.set_gs_sep_ml_item(SepKey::MapListItem.get(sep).into());
    ui_h.set_gs_sep_ml_entry(SepKey::MapListEntry.get(sep).into());
}

fn apply_sep_kv(sep: &mut SeparatorsSection, key: &str, value: &str) {
    if let Some(k) = SepKey::from_directive_key(key) {
        k.set(sep, value.to_string());
    }
}

/// 「确定」逻辑：立即写 tablet.toml，重新合并所有项目配置，分隔符改变时重新验证。
fn run(state: &Rc<RefCell<AppState>>) {
    // 取出 apply buffer
    let (ui, sep, export) = {
        let st = state.borrow();
        let gs = &st.global_settings;
        (gs.ui.clone(), gs.sep.clone(), gs.export.clone())
    };

    let separators_changed;
    {
        let mut st = state.borrow_mut();
        let old_sep = st.engine.global_config.separators.clone();

        // 更新配置：直接替换结构体
        st.engine.global_config.ui = Some(ui.clone());
        st.engine.global_config.separators = sep.clone();
        st.engine.global_config.export = Some(export.clone());
        separators_changed = old_sep != sep;

        // 立即写盘
        if let Err(e) = tablet_core::project::persist_global_config_sections(&st.engine.workdir, &st.engine.global_config) {
            st.engine.error_log(format!("[全局设置] 写入失败: {}", e));
            return;
        }
        st.engine.ui_log("[全局设置] 已保存到 tablet.toml".to_string());
    }

    // 配置变更后不再需要重新合并：项目配置独立维护
    // 仅更新 AppState 中的 UI 配置缓存
    {
        let mut st = state.borrow_mut();
        st.realtime_validate = ui.realtime_validate;
        st.picker_trigger_header_single = ui.picker_trigger_header == tablet_core::enums::PickerTrigger::Single;
        st.picker_trigger_data_single = ui.picker_trigger_data == tablet_core::enums::PickerTrigger::Single;
        st.constant_ref_allowed = ui.constant_ref_allowed;
    }

    // 分隔符改变时重新验证所有数据
    if separators_changed {
        let mut st = state.borrow_mut();
        st.engine.revalidate_all_projects();
        let err_count = st.engine.validation_errors.len();
        st.engine.error_log(format!("[全局设置] 分隔符已更新，重新验证了 {} 个错误", err_count));
    }

    // 确定成功：更新 current_snapshot 为当前配置，并重置改动标记
    {
        let mut st = state.borrow_mut();
        st.global_settings.current_snapshot = Some(st.engine.global_config.clone());
        st.global_settings.ui_tab_modified = false;
        st.global_settings.sep_tab_modified = false;
        st.global_settings.export_tab_modified = false;
    }
}

pub fn wire(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    state.borrow_mut().engine.ui_log("[全局设置] wire 函数已调用，注册回调".to_string());

    // tab 切换
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_gs_tab_changed(move |i| {
            s.borrow_mut().global_settings.tab = i;
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }

    // UI 字段编辑
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_gs_ui_field_edited(move |key, value| {
            let mut st = s.borrow_mut();
            let gs = &mut st.global_settings;
            match key.as_str() {
                "auto_commit_on_blur" => {
                    gs.ui.auto_commit_on_blur = value == "true";
                }
                "realtime_validate" => {
                    gs.ui.realtime_validate = value == "true";
                }
                "show_meta_id" => {
                    gs.ui.show_meta_id = value == "true";
                }
                "constant_ref_allowed" => {
                    gs.ui.constant_ref_allowed = value == "true";
                }
                "log_level" => {
                    gs.ui.log_level = if value.is_empty() {
                        None
                    } else {
                        tablet_core::enums::LogLevel::from_str(&value).ok()
                    };
                }
                "picker_trigger_header" => {
                    gs.ui.picker_trigger_header = tablet_core::enums::PickerTrigger::from_str(&value).unwrap_or_default();
                }
                "picker_trigger_data" => {
                    gs.ui.picker_trigger_data = tablet_core::enums::PickerTrigger::from_str(&value).unwrap_or_default();
                }
                "ref_picker_strategy" => {
                    gs.ui.ref_picker.default_strategy = tablet_core::enums::RefPickerStrategy::from_str(&value).unwrap_or_default();
                }
                _ => {}
            }
            // 计算 modified
            gs.ui_tab_modified = check_ui_modified(gs);
            drop(st);
            if let Some(ui_h) = weak.upgrade() {
                push(&ui_h, &s);
            }
        });
    }

    // 导出字段编辑
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_gs_export_field_edited(move |key, value| {
            let mut st = s.borrow_mut();
            let gs = &mut st.global_settings;
            match key.as_str() {
                "encoding" => {
                    gs.export.encoding = if value.is_empty() {
                        None
                    } else {
                        tablet_core::enums::Encoding::from_str(&value).ok()
                    };
                }
                "line_ending" => {
                    gs.export.line_ending = if value.is_empty() {
                        None
                    } else {
                        tablet_core::enums::LineEnding::from_str(&value).ok()
                    };
                }
                "json_empty_as" => {
                    if gs.export.json.is_none() {
                        gs.export.json = Some(tablet_core::model::JsonExport::default());
                    }
                    gs.export.json.as_mut().unwrap().empty_as =
                        tablet_core::enums::JsonEmptyAs::from_str(&value).ok();
                }
                "xml_empty_as" => {
                    if gs.export.xml.is_none() {
                        gs.export.xml = Some(tablet_core::model::XmlExport::default());
                    }
                    gs.export.xml.as_mut().unwrap().empty_as =
                        tablet_core::enums::XmlEmptyAs::from_str(&value).ok();
                }
                "server_data_output" => {
                    if gs.export.server.is_none() {
                        gs.export.server = Some(tablet_core::model::ServerExport::default());
                    }
                    gs.export.server.as_mut().unwrap().data_output = Some(value.to_string());
                }
                "java_package" => {
                    if gs.export.server.is_none() {
                        gs.export.server = Some(tablet_core::model::ServerExport::default());
                    }
                    if gs.export.server.as_mut().unwrap().java.is_none() {
                        gs.export.server.as_mut().unwrap().java = Some(tablet_core::model::JavaExport::default());
                    }
                    gs.export.server.as_mut().unwrap().java.as_mut().unwrap().package = Some(value.to_string());
                }
                "java_code_output" => {
                    if gs.export.server.is_none() {
                        gs.export.server = Some(tablet_core::model::ServerExport::default());
                    }
                    if gs.export.server.as_mut().unwrap().java.is_none() {
                        gs.export.server.as_mut().unwrap().java = Some(tablet_core::model::JavaExport::default());
                    }
                    gs.export.server.as_mut().unwrap().java.as_mut().unwrap().code_output = Some(value.to_string());
                }
                "go_package" => {
                    if gs.export.server.is_none() {
                        gs.export.server = Some(tablet_core::model::ServerExport::default());
                    }
                    if gs.export.server.as_mut().unwrap().go.is_none() {
                        gs.export.server.as_mut().unwrap().go = Some(tablet_core::model::GoExport::default());
                    }
                    gs.export.server.as_mut().unwrap().go.as_mut().unwrap().package = Some(value.to_string());
                }
                "go_code_output" => {
                    if gs.export.server.is_none() {
                        gs.export.server = Some(tablet_core::model::ServerExport::default());
                    }
                    if gs.export.server.as_mut().unwrap().go.is_none() {
                        gs.export.server.as_mut().unwrap().go = Some(tablet_core::model::GoExport::default());
                    }
                    gs.export.server.as_mut().unwrap().go.as_mut().unwrap().code_output = Some(value.to_string());
                }
                "cpp_namespace" => {
                    if gs.export.server.is_none() {
                        gs.export.server = Some(tablet_core::model::ServerExport::default());
                    }
                    if gs.export.server.as_mut().unwrap().cpp.is_none() {
                        gs.export.server.as_mut().unwrap().cpp = Some(tablet_core::model::CppExport::default());
                    }
                    gs.export.server.as_mut().unwrap().cpp.as_mut().unwrap().namespace = Some(value.to_string());
                }
                "cpp_code_output" => {
                    if gs.export.server.is_none() {
                        gs.export.server = Some(tablet_core::model::ServerExport::default());
                    }
                    if gs.export.server.as_mut().unwrap().cpp.is_none() {
                        gs.export.server.as_mut().unwrap().cpp = Some(tablet_core::model::CppExport::default());
                    }
                    gs.export.server.as_mut().unwrap().cpp.as_mut().unwrap().code_output = Some(value.to_string());
                }
                "cpp_json_lib" => {
                    if gs.export.server.is_none() {
                        gs.export.server = Some(tablet_core::model::ServerExport::default());
                    }
                    if gs.export.server.as_mut().unwrap().cpp.is_none() {
                        gs.export.server.as_mut().unwrap().cpp = Some(tablet_core::model::CppExport::default());
                    }
                    gs.export.server.as_mut().unwrap().cpp.as_mut().unwrap().json_lib =
                        tablet_core::enums::CppJsonLib::from_str(&value).ok();
                }
                "csharp_dotnet_namespace" => {
                    if gs.export.server.is_none() {
                        gs.export.server = Some(tablet_core::model::ServerExport::default());
                    }
                    if gs.export.server.as_mut().unwrap().csharp_dotnet.is_none() {
                        gs.export.server.as_mut().unwrap().csharp_dotnet = Some(tablet_core::model::DotNetExport::default());
                    }
                    gs.export.server.as_mut().unwrap().csharp_dotnet.as_mut().unwrap().namespace = Some(value.to_string());
                }
                "csharp_dotnet_code_output" => {
                    if gs.export.server.is_none() {
                        gs.export.server = Some(tablet_core::model::ServerExport::default());
                    }
                    if gs.export.server.as_mut().unwrap().csharp_dotnet.is_none() {
                        gs.export.server.as_mut().unwrap().csharp_dotnet = Some(tablet_core::model::DotNetExport::default());
                    }
                    gs.export.server.as_mut().unwrap().csharp_dotnet.as_mut().unwrap().code_output = Some(value.to_string());
                }
                "typescript_server_code_output" => {
                    if gs.export.server.is_none() {
                        gs.export.server = Some(tablet_core::model::ServerExport::default());
                    }
                    if gs.export.server.as_mut().unwrap().typescript.is_none() {
                        gs.export.server.as_mut().unwrap().typescript = Some(tablet_core::model::ServerTypeScriptExport::default());
                    }
                    gs.export.server.as_mut().unwrap().typescript.as_mut().unwrap().output = Some(value.to_string());
                }
                "lua_output" => {
                    if gs.export.client.is_none() {
                        gs.export.client = Some(tablet_core::model::ClientConfig::default());
                    }
                    if gs.export.client.as_mut().unwrap().lua.is_none() {
                        gs.export.client.as_mut().unwrap().lua = Some(tablet_core::model::LuaExport::default());
                    }
                    gs.export.client.as_mut().unwrap().lua.as_mut().unwrap().output = Some(value.to_string());
                }
                "gdscript_output" => {
                    if gs.export.client.is_none() {
                        gs.export.client = Some(tablet_core::model::ClientConfig::default());
                    }
                    if gs.export.client.as_mut().unwrap().gdscript.is_none() {
                        gs.export.client.as_mut().unwrap().gdscript = Some(tablet_core::model::GdScriptExport::default());
                    }
                    gs.export.client.as_mut().unwrap().gdscript.as_mut().unwrap().output = Some(value.to_string());
                }
                "typescript_client_output" => {
                    if gs.export.client.is_none() {
                        gs.export.client = Some(tablet_core::model::ClientConfig::default());
                    }
                    if gs.export.client.as_mut().unwrap().typescript.is_none() {
                        gs.export.client.as_mut().unwrap().typescript = Some(tablet_core::model::ClientTypeScriptExport::default());
                    }
                    gs.export.client.as_mut().unwrap().typescript.as_mut().unwrap().output = Some(value.to_string());
                }
                "csharp_unity_namespace" => {
                    if gs.export.client.is_none() {
                        gs.export.client = Some(tablet_core::model::ClientConfig::default());
                    }
                    if gs.export.client.as_mut().unwrap().csharp_unity.is_none() {
                        gs.export.client.as_mut().unwrap().csharp_unity = Some(tablet_core::model::UnityCSharpExport::default());
                    }
                    gs.export.client.as_mut().unwrap().csharp_unity.as_mut().unwrap().namespace = Some(value.to_string());
                }
                "csharp_unity_code_output" => {
                    if gs.export.client.is_none() {
                        gs.export.client = Some(tablet_core::model::ClientConfig::default());
                    }
                    if gs.export.client.as_mut().unwrap().csharp_unity.is_none() {
                        gs.export.client.as_mut().unwrap().csharp_unity = Some(tablet_core::model::UnityCSharpExport::default());
                    }
                    gs.export.client.as_mut().unwrap().csharp_unity.as_mut().unwrap().code_output = Some(value.to_string());
                }
                "csharp_godot_namespace" => {
                    if gs.export.client.is_none() {
                        gs.export.client = Some(tablet_core::model::ClientConfig::default());
                    }
                    if gs.export.client.as_mut().unwrap().csharp_godot.is_none() {
                        gs.export.client.as_mut().unwrap().csharp_godot = Some(tablet_core::model::GodotCSharpExport::default());
                    }
                    gs.export.client.as_mut().unwrap().csharp_godot.as_mut().unwrap().namespace = Some(value.to_string());
                }
                "csharp_godot_code_output" => {
                    if gs.export.client.is_none() {
                        gs.export.client = Some(tablet_core::model::ClientConfig::default());
                    }
                    if gs.export.client.as_mut().unwrap().csharp_godot.is_none() {
                        gs.export.client.as_mut().unwrap().csharp_godot = Some(tablet_core::model::GodotCSharpExport::default());
                    }
                    gs.export.client.as_mut().unwrap().csharp_godot.as_mut().unwrap().code_output = Some(value.to_string());
                }
                _ => {}
            }
            // 计算 modified
            gs.export_tab_modified = check_export_modified(gs);
            drop(st);
            if let Some(ui_h) = weak.upgrade() {
                push(&ui_h, &s);
            }
        });
    }

    // 分隔符编辑
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_gs_sep_edited(move |key, value| {
            let mut st = s.borrow_mut();
            apply_sep_kv(&mut st.global_settings.sep, &key, &value);
            st.global_settings.sep_tab_modified = check_sep_modified(&st.global_settings);
            let modified = st.global_settings.sep_tab_modified;
            drop(st);
            s.borrow_mut().engine.ui_log(format!("[全局设置] 分隔符 {} 编辑，modified={}", key, modified));
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }

    // 使用默认值：根据当前 tab 填充硬编码默认值到 apply buffer，计算 modified
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_gs_reset_clicked(move || {
            let tab = s.borrow().global_settings.tab;
            let mut st = s.borrow_mut();
            if tab == 0 {
                // UI 设置：填充硬编码默认值
                st.global_settings.ui = tablet_core::model::UiConfig::default();
                st.global_settings.ui_tab_modified = check_ui_modified(&st.global_settings);
            } else if tab == 1 {
                // 分隔符：填充硬编码默认值
                st.global_settings.sep = SeparatorsSection::default();
                st.global_settings.sep_tab_modified = check_sep_modified(&st.global_settings);
            } else if tab == 2 {
                // 导出设置：填充硬编码默认值
                st.global_settings.export = tablet_core::model::ExportConfig::default();
                st.global_settings.export_tab_modified = check_export_modified(&st.global_settings);
            }
            drop(st);
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }

    // 撤销改动：根据当前 tab 从 current_snapshot 恢复到 apply buffer
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_gs_undo_clicked(move || {
            let (tab, snapshot) = {
                let st = s.borrow();
                (st.global_settings.tab, st.global_settings.current_snapshot.clone())
            };

            if let Some(current) = snapshot {
                let mut st = s.borrow_mut();
                if tab == 0 {
                    // UI 设置：从 current 恢复
                    st.global_settings.ui = current.ui.clone().unwrap_or_default();
                    st.global_settings.ui_tab_modified = false;
                } else if tab == 1 {
                    // 分隔符：从 current 恢复
                    st.global_settings.sep = current.separators.clone();
                    st.global_settings.sep_tab_modified = false;
                } else if tab == 2 {
                    // 导出设置：从 current 恢复
                    st.global_settings.export = current.export.clone().unwrap_or_default();
                    st.global_settings.export_tab_modified = false;
                }
            }

            if let Some(ui_h) = weak.upgrade() {
                push(&ui_h, &s);
            }
        });
    }

    // 确定
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_gs_confirm(move || {
            run(&s);
            s.borrow_mut().global_settings.close();
            if let Some(ui_h) = weak.upgrade() {
                push(&ui_h, &s);
                refresh::after_global_settings(&ui_h, &s);
            }
        });
    }

    // 取消：不需要恢复 GlobalConfig（用户未点确定，内存值未变），直接关闭对话框
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_gs_cancel(move || {
            // 关闭对话框
            s.borrow_mut().global_settings.close();
            // 推送关闭状态到 UI
            if let Some(ui_h) = weak.upgrade() {
                ui_h.set_dlg_gs_open(false);
            }
        });
    }
}

/// 检查 UI 设置 tab 是否有变动（apply != current）
fn check_ui_modified(gs: &crate::state::GlobalSettingsDialogState) -> bool {
    let current = match &gs.current_snapshot {
        Some(c) => c,
        None => return false,
    };
    let current_ui = current.ui.as_ref().cloned().unwrap_or_default();
    gs.ui != current_ui
}

/// 检查分隔符 tab 是否有变动（apply != current）
fn check_sep_modified(gs: &crate::state::GlobalSettingsDialogState) -> bool {
    let current = match &gs.current_snapshot {
        Some(c) => c,
        None => return false,
    };
    gs.sep != current.separators
}

/// 检查导出设置 tab 是否有变动（apply != current）
fn check_export_modified(gs: &crate::state::GlobalSettingsDialogState) -> bool {
    let current = match &gs.current_snapshot {
        Some(c) => c,
        None => return false,
    };
    let current_export = current.export.as_ref().cloned().unwrap_or_default();
    gs.export != current_export
}
