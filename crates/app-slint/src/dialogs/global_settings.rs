// 全局设置对话框：UI设置 / 分隔符 / 导出设置 3 tab。
//
// 数据范式：**编辑缓冲 + 打开时快照**（@02.26.6）
// - apply：`global_settings` 的 ui / sep / export —— 待应用的编辑缓冲。
//   **这个缓冲必须留**：`engine.global_config` 是活的工作区状态，别的东西会读它
//   （分隔符驱动校验、导出配置），而且它的 `project_management` 段自己会变。
// - 参照物：`current_snapshot`（打开对话框那一刻的 tablet.toml 值）。
//   因为「确定」就是写盘点，「内存 ≠ 盘上」这段窗口**恰好只有对话框开着的时候** ——
//   关掉就没有东西需要留存了，所以这里不需要像项目设置那样把 `original_*` 存进数据结构。
// - modified：每个字段自己回答「和快照不一样吗」，后端算好推上来；**按 tab 分块比**
//   （`GlobalConfig` 里的 `project_management` 段不归这个对话框管）。
//
// 点击「确定」后立即写 tablet.toml（工作区级、一定已存在，@02.26.3）。
// **保留「取消」**：确定就是落盘，去掉取消等于「关窗即改工作区配置」——这个副作用比
// 项目设置重得多（工作区级、立即生效，分隔符还会触发全项目重校验）。

use std::cell::RefCell;
use std::rc::Rc;
use std::str::FromStr;

use slint::ComponentHandle;
use tablet_core::export_keys::ExportKey;
use tablet_core::model::{ExportConfig, UiConfig};
use tablet_core::types::{SepKey, SeparatorsSection};
use tablet_core::ui_keys::UiKey;

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

/// 下拉字段：在选项表里找当前值，得到下标。找不到（手改过 tablet.toml）落第一项。
fn index_of(options: Vec<&'static str>, value: &str) -> i32 {
    options.iter().position(|s| *s == value).unwrap_or(0) as i32
}

pub fn push(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let st = state.borrow();
    let gs = &st.global_settings;

    ui_h.set_dlg_gs_open(gs.open);
    if !gs.open {
        return;
    }
    ui_h.set_gs_tab(gs.tab);

    // 参照物 = 打开对话框那一刻的 tablet.toml。**按 tab 分块比** —— `GlobalConfig` 里的
    // `project_management` 段不归这个对话框管（拖项目排序也会写 tablet.toml），
    // 整份 `!=` 会让「拖一下排序」把对话框误判成有未保存改动。
    let snap = gs.current_snapshot.clone().unwrap_or_default();
    match gs.tab {
        0 => push_ui(ui_h, &gs.ui, &snap.ui.clone().unwrap_or_default()),
        1 => push_sep(ui_h, &gs.sep, &snap.separators),
        2 => push_export(ui_h, &gs.export, &snap.export.clone().unwrap_or_default()),
        _ => {}
    }
}

fn push_ui(ui_h: &AppWindow, live: &UiConfig, orig: &UiConfig) {
    use tablet_core::enums::{LogLevel, PickerTrigger, RefPickerStrategy};
    let v = |k: UiKey| k.get(live);
    let m = |k: UiKey| k.modified(live, orig);

    // 布尔：值 + 「和快照不一样吗」。成对写，加字段时不会只加一半。
    ui_h.set_gs_ui_auto_commit_on_blur(v(UiKey::AutoCommitOnBlur) == "true");
    ui_h.set_gs_ui_auto_commit_on_blur_modified(m(UiKey::AutoCommitOnBlur));
    ui_h.set_gs_ui_realtime_validate(v(UiKey::RealtimeValidate) == "true");
    ui_h.set_gs_ui_realtime_validate_modified(m(UiKey::RealtimeValidate));
    ui_h.set_gs_ui_show_meta_id(v(UiKey::ShowMetaId) == "true");
    ui_h.set_gs_ui_show_meta_id_modified(m(UiKey::ShowMetaId));
    ui_h.set_gs_ui_constant_ref_allowed(v(UiKey::ConstantRefAllowed) == "true");
    ui_h.set_gs_ui_constant_ref_allowed_modified(m(UiKey::ConstantRefAllowed));

    // 下拉：下标 + 改没改
    ui_h.set_gs_ui_log_level_index(index_of(LogLevel::all_str(), &v(UiKey::LogLevel)));
    ui_h.set_gs_ui_log_level_modified(m(UiKey::LogLevel));
    ui_h.set_gs_ui_ref_picker_strategy_index(
        index_of(RefPickerStrategy::all_str(), &v(UiKey::RefPickerStrategy)));
    ui_h.set_gs_ui_ref_picker_strategy_modified(m(UiKey::RefPickerStrategy));
    ui_h.set_gs_ui_picker_trigger_header_index(
        index_of(PickerTrigger::all_str(), &v(UiKey::PickerTriggerHeader)));
    ui_h.set_gs_ui_picker_trigger_header_modified(m(UiKey::PickerTriggerHeader));
    ui_h.set_gs_ui_picker_trigger_data_index(
        index_of(PickerTrigger::all_str(), &v(UiKey::PickerTriggerData)));
    ui_h.set_gs_ui_picker_trigger_data_modified(m(UiKey::PickerTriggerData));
}

fn push_sep(ui_h: &AppWindow, live: &SeparatorsSection, orig: &SeparatorsSection) {
    let v = |k: SepKey| k.get(live).to_string();
    let m = |k: SepKey| k.get(live) != k.get(orig);

    ui_h.set_gs_sep_tuple2(v(SepKey::Tuple2).into());
    ui_h.set_gs_sep_tuple2_modified(m(SepKey::Tuple2));
    ui_h.set_gs_sep_tuple3(v(SepKey::Tuple3).into());
    ui_h.set_gs_sep_tuple3_modified(m(SepKey::Tuple3));
    ui_h.set_gs_sep_tuple4(v(SepKey::Tuple4).into());
    ui_h.set_gs_sep_tuple4_modified(m(SepKey::Tuple4));
    ui_h.set_gs_sep_list(v(SepKey::List).into());
    ui_h.set_gs_sep_list_modified(m(SepKey::List));
    ui_h.set_gs_sep_set(v(SepKey::Set).into());
    ui_h.set_gs_sep_set_modified(m(SepKey::Set));
    ui_h.set_gs_sep_map_kv(v(SepKey::MapKv).into());
    ui_h.set_gs_sep_map_kv_modified(m(SepKey::MapKv));
    ui_h.set_gs_sep_map_entry(v(SepKey::MapEntry).into());
    ui_h.set_gs_sep_map_entry_modified(m(SepKey::MapEntry));
    ui_h.set_gs_sep_lt2_tuple(v(SepKey::ListTuple2Tuple).into());
    ui_h.set_gs_sep_lt2_tuple_modified(m(SepKey::ListTuple2Tuple));
    ui_h.set_gs_sep_lt2_list(v(SepKey::ListTuple2List).into());
    ui_h.set_gs_sep_lt2_list_modified(m(SepKey::ListTuple2List));
    ui_h.set_gs_sep_lt3_tuple(v(SepKey::ListTuple3Tuple).into());
    ui_h.set_gs_sep_lt3_tuple_modified(m(SepKey::ListTuple3Tuple));
    ui_h.set_gs_sep_lt3_list(v(SepKey::ListTuple3List).into());
    ui_h.set_gs_sep_lt3_list_modified(m(SepKey::ListTuple3List));
    ui_h.set_gs_sep_lt4_tuple(v(SepKey::ListTuple4Tuple).into());
    ui_h.set_gs_sep_lt4_tuple_modified(m(SepKey::ListTuple4Tuple));
    ui_h.set_gs_sep_lt4_list(v(SepKey::ListTuple4List).into());
    ui_h.set_gs_sep_lt4_list_modified(m(SepKey::ListTuple4List));
    ui_h.set_gs_sep_mt2_kv(v(SepKey::MapTuple2Kv).into());
    ui_h.set_gs_sep_mt2_kv_modified(m(SepKey::MapTuple2Kv));
    ui_h.set_gs_sep_mt2_tuple(v(SepKey::MapTuple2Tuple).into());
    ui_h.set_gs_sep_mt2_tuple_modified(m(SepKey::MapTuple2Tuple));
    ui_h.set_gs_sep_mt2_entry(v(SepKey::MapTuple2Entry).into());
    ui_h.set_gs_sep_mt2_entry_modified(m(SepKey::MapTuple2Entry));
    ui_h.set_gs_sep_mt3_kv(v(SepKey::MapTuple3Kv).into());
    ui_h.set_gs_sep_mt3_kv_modified(m(SepKey::MapTuple3Kv));
    ui_h.set_gs_sep_mt3_tuple(v(SepKey::MapTuple3Tuple).into());
    ui_h.set_gs_sep_mt3_tuple_modified(m(SepKey::MapTuple3Tuple));
    ui_h.set_gs_sep_mt3_entry(v(SepKey::MapTuple3Entry).into());
    ui_h.set_gs_sep_mt3_entry_modified(m(SepKey::MapTuple3Entry));
    ui_h.set_gs_sep_mt4_kv(v(SepKey::MapTuple4Kv).into());
    ui_h.set_gs_sep_mt4_kv_modified(m(SepKey::MapTuple4Kv));
    ui_h.set_gs_sep_mt4_tuple(v(SepKey::MapTuple4Tuple).into());
    ui_h.set_gs_sep_mt4_tuple_modified(m(SepKey::MapTuple4Tuple));
    ui_h.set_gs_sep_mt4_entry(v(SepKey::MapTuple4Entry).into());
    ui_h.set_gs_sep_mt4_entry_modified(m(SepKey::MapTuple4Entry));
    ui_h.set_gs_sep_ml_kv(v(SepKey::MapListKv).into());
    ui_h.set_gs_sep_ml_kv_modified(m(SepKey::MapListKv));
    ui_h.set_gs_sep_ml_item(v(SepKey::MapListItem).into());
    ui_h.set_gs_sep_ml_item_modified(m(SepKey::MapListItem));
    ui_h.set_gs_sep_ml_entry(v(SepKey::MapListEntry).into());
    ui_h.set_gs_sep_ml_entry_modified(m(SepKey::MapListEntry));
}

fn push_export(ui_h: &AppWindow, live: &ExportConfig, orig: &ExportConfig) {
    use tablet_core::enums::{CppJsonLib, Encoding, JsonEmptyAs, LineEnding, XmlEmptyAs};
    let v = |k: ExportKey| k.get(live);
    let m = |k: ExportKey| k.modified(live, orig);

    ui_h.set_gs_export_encoding_index(index_of(Encoding::all_str(), &v(ExportKey::Encoding)));
    ui_h.set_gs_export_encoding_modified(m(ExportKey::Encoding));
    ui_h.set_gs_export_line_ending_index(index_of(LineEnding::all_str(), &v(ExportKey::LineEnding)));
    ui_h.set_gs_export_line_ending_modified(m(ExportKey::LineEnding));
    ui_h.set_gs_export_json_empty_as_index(index_of(JsonEmptyAs::all_str(), &v(ExportKey::JsonEmptyAs)));
    ui_h.set_gs_export_json_empty_as_modified(m(ExportKey::JsonEmptyAs));
    ui_h.set_gs_export_xml_empty_as_index(index_of(XmlEmptyAs::all_str(), &v(ExportKey::XmlEmptyAs)));
    ui_h.set_gs_export_xml_empty_as_modified(m(ExportKey::XmlEmptyAs));
    ui_h.set_gs_export_cpp_json_lib_index(index_of(CppJsonLib::all_str(), &v(ExportKey::CppJsonLib)));
    ui_h.set_gs_export_cpp_json_lib_modified(m(ExportKey::CppJsonLib));

    ui_h.set_gs_export_server_data_output(v(ExportKey::ServerDataOutput).into());
    ui_h.set_gs_export_server_data_output_modified(m(ExportKey::ServerDataOutput));
    ui_h.set_gs_export_java_package(v(ExportKey::JavaPackage).into());
    ui_h.set_gs_export_java_package_modified(m(ExportKey::JavaPackage));
    ui_h.set_gs_export_java_code_output(v(ExportKey::JavaCodeOutput).into());
    ui_h.set_gs_export_java_code_output_modified(m(ExportKey::JavaCodeOutput));
    ui_h.set_gs_export_go_package(v(ExportKey::GoPackage).into());
    ui_h.set_gs_export_go_package_modified(m(ExportKey::GoPackage));
    ui_h.set_gs_export_go_code_output(v(ExportKey::GoCodeOutput).into());
    ui_h.set_gs_export_go_code_output_modified(m(ExportKey::GoCodeOutput));
    ui_h.set_gs_export_cpp_namespace(v(ExportKey::CppNamespace).into());
    ui_h.set_gs_export_cpp_namespace_modified(m(ExportKey::CppNamespace));
    ui_h.set_gs_export_cpp_code_output(v(ExportKey::CppCodeOutput).into());
    ui_h.set_gs_export_cpp_code_output_modified(m(ExportKey::CppCodeOutput));
    ui_h.set_gs_export_csharp_dotnet_namespace(v(ExportKey::CsharpDotnetNamespace).into());
    ui_h.set_gs_export_csharp_dotnet_namespace_modified(m(ExportKey::CsharpDotnetNamespace));
    ui_h.set_gs_export_csharp_dotnet_code_output(v(ExportKey::CsharpDotnetCodeOutput).into());
    ui_h.set_gs_export_csharp_dotnet_code_output_modified(m(ExportKey::CsharpDotnetCodeOutput));
    ui_h.set_gs_export_typescript_server_code_output(v(ExportKey::TypescriptServerCodeOutput).into());
    ui_h.set_gs_export_typescript_server_code_output_modified(m(ExportKey::TypescriptServerCodeOutput));
    ui_h.set_gs_export_lua_output(v(ExportKey::LuaOutput).into());
    ui_h.set_gs_export_lua_output_modified(m(ExportKey::LuaOutput));
    ui_h.set_gs_export_gdscript_output(v(ExportKey::GdscriptOutput).into());
    ui_h.set_gs_export_gdscript_output_modified(m(ExportKey::GdscriptOutput));
    ui_h.set_gs_export_typescript_client_output(v(ExportKey::TypescriptClientOutput).into());
    ui_h.set_gs_export_typescript_client_output_modified(m(ExportKey::TypescriptClientOutput));
    ui_h.set_gs_export_csharp_unity_namespace(v(ExportKey::CsharpUnityNamespace).into());
    ui_h.set_gs_export_csharp_unity_namespace_modified(m(ExportKey::CsharpUnityNamespace));
    ui_h.set_gs_export_csharp_unity_code_output(v(ExportKey::CsharpUnityCodeOutput).into());
    ui_h.set_gs_export_csharp_unity_code_output_modified(m(ExportKey::CsharpUnityCodeOutput));
    ui_h.set_gs_export_csharp_godot_namespace(v(ExportKey::CsharpGodotNamespace).into());
    ui_h.set_gs_export_csharp_godot_namespace_modified(m(ExportKey::CsharpGodotNamespace));
    ui_h.set_gs_export_csharp_godot_code_output(v(ExportKey::CsharpGodotCodeOutput).into());
    ui_h.set_gs_export_csharp_godot_code_output_modified(m(ExportKey::CsharpGodotCodeOutput));

    ui_h.set_gs_export_cpp_custom_regions(v(ExportKey::CppCustomRegions) == "true");
    ui_h.set_gs_export_cpp_custom_regions_modified(m(ExportKey::CppCustomRegions));
    ui_h.set_gs_export_java_lombok(v(ExportKey::JavaLombok) == "true");
    ui_h.set_gs_export_java_lombok_modified(m(ExportKey::JavaLombok));
    ui_h.set_gs_export_java_custom_regions(v(ExportKey::JavaCustomRegions) == "true");
    ui_h.set_gs_export_java_custom_regions_modified(m(ExportKey::JavaCustomRegions));
    ui_h.set_gs_export_go_custom_regions(v(ExportKey::GoCustomRegions) == "true");
    ui_h.set_gs_export_go_custom_regions_modified(m(ExportKey::GoCustomRegions));
    ui_h.set_gs_export_csharp_dotnet_custom_regions(v(ExportKey::CsharpDotnetCustomRegions) == "true");
    ui_h.set_gs_export_csharp_dotnet_custom_regions_modified(m(ExportKey::CsharpDotnetCustomRegions));
    ui_h.set_gs_export_csharp_unity_custom_regions(v(ExportKey::CsharpUnityCustomRegions) == "true");
    ui_h.set_gs_export_csharp_unity_custom_regions_modified(m(ExportKey::CsharpUnityCustomRegions));
    ui_h.set_gs_export_csharp_godot_custom_regions(v(ExportKey::CsharpGodotCustomRegions) == "true");
    ui_h.set_gs_export_csharp_godot_custom_regions_modified(m(ExportKey::CsharpGodotCustomRegions));
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

        // 立即整写盘：project 布局由引擎运行时重建，ui/export/separators 用刚改的内存值
        let sort_enum = tablet_core::enums::ProjectSort::from_str(&st.project_sort).unwrap_or_default();
        let order = st.project_order.clone();
        if let Err(e) = st.engine.persist_global_config(sort_enum, &order) {
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

    // 确定成功：参照物往前挪到刚写下去的值（于是所有字段的 -modified 自动清掉）
    {
        let mut st = state.borrow_mut();
        st.global_settings.current_snapshot = Some(st.engine.global_config.clone());
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
            match UiKey::from_str(&key) {
                Some(k) => {
                    let mut st = s.borrow_mut();
                    if !k.set(&mut st.global_settings.ui, &value) {
                        st.engine.error_log(format!(
                            "[全局设置] {} 不接受值 {:?}（非法，保持原样）", key, value));
                    }
                }
                // 认不出的 key 要吵 —— 导出那边静默吞掉三个 key，就是这么漏的
                None => s.borrow_mut().engine.error_log(format!("[全局设置] 不认识的 UI 字段: {}", key)),
            }
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }

    // 导出字段编辑
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_gs_export_field_edited(move |key, value| {
            match ExportKey::from_str(&key) {
                Some(k) => {
                    let mut st = s.borrow_mut();
                    if !k.set(&mut st.global_settings.export, &value) {
                        st.engine.error_log(format!(
                            "[全局设置] 导出配置不接受 {} = {:?}（非法，保持原样）", key, value));
                    }
                }
                None => s.borrow_mut().engine.error_log(format!("[全局设置] 不认识的导出字段: {}", key)),
            }
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }

    // 分隔符编辑
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_gs_sep_edited(move |key, value| {
            match SepKey::from_directive_key(&key) {
                Some(k) => k.set(&mut s.borrow_mut().global_settings.sep, value.to_string()),
                None => s.borrow_mut().engine.error_log(format!("[全局设置] 不认识的分隔符字段: {}", key)),
            }
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
            } else if tab == 1 {
                // 分隔符：填充硬编码默认值
                st.global_settings.sep = SeparatorsSection::default();
            } else if tab == 2 {
                // 导出设置：填充硬编码默认值
                st.global_settings.export = tablet_core::model::ExportConfig::default();
            }
            drop(st);
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }

    // 单字段还原：把**打开对话框那一刻的 tablet.toml 值**写回编辑缓冲（@02.26.6）。
    // 刻意**不做**页面级「全部还原」——「取消」已经是它了（区别只是关不关窗）。
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_gs_ui_revert(move |key| {
            let mut st = s.borrow_mut();
            let snap = st.global_settings.current_snapshot.as_ref()
                .and_then(|c| c.ui.clone()).unwrap_or_default();
            match UiKey::from_str(&key) {
                Some(k) => k.revert(&mut st.global_settings.ui, &snap),
                None => st.engine.error_log(format!("[全局设置] 不认识的 UI 字段: {}", key)),
            }
            drop(st);
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_gs_sep_revert(move |key| {
            let mut st = s.borrow_mut();
            let snap = st.global_settings.current_snapshot.as_ref()
                .map(|c| c.separators.clone()).unwrap_or_default();
            match SepKey::from_directive_key(&key) {
                Some(k) => k.set(&mut st.global_settings.sep, k.get(&snap).to_string()),
                None => st.engine.error_log(format!("[全局设置] 不认识的分隔符字段: {}", key)),
            }
            drop(st);
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_gs_export_revert(move |key| {
            let mut st = s.borrow_mut();
            let snap = st.global_settings.current_snapshot.as_ref()
                .and_then(|c| c.export.clone()).unwrap_or_default();
            match ExportKey::from_str(&key) {
                Some(k) => k.revert(&mut st.global_settings.export, &snap),
                None => st.engine.error_log(format!("[全局设置] 不认识的导出字段: {}", key)),
            }
            drop(st);
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
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
