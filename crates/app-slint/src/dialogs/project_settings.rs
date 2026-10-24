// 项目设置对话框：身份（name / category / version，**id 只读**）+ 分隔符（25 个叶子）
// + 导出配置（29 个叶子）。
//
// **没有 apply buffer、没有快照、没有修改标记。** 编辑直接落内存里的 `Project`，
// 「这个字段和盘上不一样吗」由 `Project::original_meta` / `original_separators` /
// `original_export` 回答（@02.26.2）——和树上的分层脏同一个道理：脏是算出来的。
// 于是「改回去高亮自动消失」是免费的，不需要在编辑路径上补任何一笔。
//
// `确定` 只是关闭；提交点仍然是「保存项目」（未落盘项目连 `project_root` 目录都还没有，
// 见 @02.26.3）。
//
// 分隔符改动要全表重校验，但**不在每次按键时做**——对话框是模态的，背后的 grid 没人看。
// 统一挪到关闭时按 `sep_touched` 做一次，见 `close_and_apply`。

use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;
use tablet_core::enums::{CppJsonLib, Encoding, JsonEmptyAs, LineEnding, XmlEmptyAs};
use tablet_core::export_keys::ExportKey;
use tablet_core::model::{Project, SeverityAction};
use tablet_core::types::SepKey;

use crate::state::AppState;
use crate::{refresh, AppWindow};

/// 由项目右键「项目设置...」入口调用。
///
/// 什么都不用拷 —— 对话框读写的就是 `Project` 本身，参照物是它自己的 `original_*`。
pub fn open_for(state: &Rc<RefCell<AppState>>, project_id: &str) {
    let mut st = state.borrow_mut();
    if st.engine.find_project(project_id).is_none() {
        st.engine.ui_log(format!("[项目设置] 项目不存在: {}", project_id));
        return;
    }
    let ps = &mut st.project_settings;
    ps.open = true;
    ps.tab = 0;
    ps.project_id = project_id.to_string();
    ps.sep_touched = false;
}

// ─── 往内存里落一次编辑 ────────────────────────────────────────────────────

/// 项目没了就什么都不做（下一帧 `push` 会把对话框关掉）。
fn edit_project(state: &Rc<RefCell<AppState>>, f: impl FnOnce(&mut Project)) {
    let mut st = state.borrow_mut();
    let id = st.project_settings.project_id.clone();
    if let Some(p) = st.engine.find_project_mut(&id) {
        f(p);
    }
}

/// 把盘上的快照取一份出来。**先 clone 再拿 mutable**，避开同时借 `engine` 两次。
fn original_of<T>(state: &Rc<RefCell<AppState>>, f: impl FnOnce(&Project) -> T) -> Option<T> {
    let st = state.borrow();
    st.engine.find_project(&st.project_settings.project_id).map(f)
}

/// 写分隔符的统一入口：**凡写过就置 `sep_touched`**（关闭时要据此重校验）。
fn write_sep(state: &Rc<RefCell<AppState>>, f: impl FnOnce(&mut tablet_core::types::SeparatorsSection)) {
    let mut st = state.borrow_mut();
    let id = st.project_settings.project_id.clone();
    if let Some(p) = st.engine.find_project_mut(&id) {
        f(&mut p.schema.separators);
        st.project_settings.sep_touched = true;
    }
}

// ─── push ─────────────────────────────────────────────────────────────────

pub fn push(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let (open, tab, project_id) = {
        let st = state.borrow();
        let ps = &st.project_settings;
        (ps.open, ps.tab, ps.project_id.clone())
    };
    ui_h.set_dlg_ps_open(open);
    if !open {
        return;
    }

    let st = state.borrow();
    let Some(p) = st.engine.find_project(&project_id) else {
        // 项目在别处被关掉 / 删掉了 —— 对话框自己退出，不拿着悬空的 id 到处找。
        // `push` 是 `refresh::after_ctx_menu` 每次右键都会调的，这条路径不是理论上的。
        drop(st);
        state.borrow_mut().project_settings.close();
        ui_h.set_dlg_ps_open(false);
        return;
    };

    ui_h.set_ps_tab(tab);
    // 「全部还原」只在 meta / separators / [export] 真的偏离盘上时可用。
    // 刻意不含 groups —— 数据改动它还原不了，不该让按钮亮起来。
    ui_h.set_ps_can_revert_all(p.settings_changed());

    match tab {
        0 => push_identity(ui_h, p),
        1 => push_sep(ui_h, p),
        2 => push_export(ui_h, p),
        3 => push_validate(ui_h, p),
        _ => {}
    }
}

fn push_identity(ui_h: &AppWindow, p: &Project) {
    let (m, o) = (&p.schema.meta, &p.original_meta);
    // id 只读：它在新建 / 克隆对话框里定，改 id 要动目录，风险大（@02.26）。
    ui_h.set_ps_id(m.id.clone().into());
    ui_h.set_ps_name(m.name.clone().into());
    ui_h.set_ps_name_modified(m.name != o.name);
    ui_h.set_ps_category(m.category.clone().into());
    ui_h.set_ps_category_modified(m.category != o.category);
    ui_h.set_ps_version(m.version.clone().into());
    ui_h.set_ps_version_modified(m.version != o.version);
}

/// 分隔符 tab：25 个叶子，每个推「值」+「和盘上不一样吗」两件。
///
/// 成对写在相邻两行，是为了加字段时不会只加一半 —— 值推了、高亮忘了，
/// 表现就是「改了没反应」，最难查的那一类。
fn push_sep(ui_h: &AppWindow, p: &Project) {
    let (live, orig) = (&p.schema.separators, &p.original_separators);
    let v = |k: SepKey| k.get(live).to_string();
    let m = |k: SepKey| k.get(live) != k.get(orig);

    ui_h.set_ps_sep_tuple2(v(SepKey::Tuple2).into());
    ui_h.set_ps_sep_tuple2_modified(m(SepKey::Tuple2));
    ui_h.set_ps_sep_tuple3(v(SepKey::Tuple3).into());
    ui_h.set_ps_sep_tuple3_modified(m(SepKey::Tuple3));
    ui_h.set_ps_sep_tuple4(v(SepKey::Tuple4).into());
    ui_h.set_ps_sep_tuple4_modified(m(SepKey::Tuple4));
    ui_h.set_ps_sep_list(v(SepKey::List).into());
    ui_h.set_ps_sep_list_modified(m(SepKey::List));
    ui_h.set_ps_sep_set(v(SepKey::Set).into());
    ui_h.set_ps_sep_set_modified(m(SepKey::Set));
    ui_h.set_ps_sep_map_kv(v(SepKey::MapKv).into());
    ui_h.set_ps_sep_map_kv_modified(m(SepKey::MapKv));
    ui_h.set_ps_sep_map_entry(v(SepKey::MapEntry).into());
    ui_h.set_ps_sep_map_entry_modified(m(SepKey::MapEntry));
    ui_h.set_ps_sep_lt2_tuple(v(SepKey::ListTuple2Tuple).into());
    ui_h.set_ps_sep_lt2_tuple_modified(m(SepKey::ListTuple2Tuple));
    ui_h.set_ps_sep_lt2_list(v(SepKey::ListTuple2List).into());
    ui_h.set_ps_sep_lt2_list_modified(m(SepKey::ListTuple2List));
    ui_h.set_ps_sep_lt3_tuple(v(SepKey::ListTuple3Tuple).into());
    ui_h.set_ps_sep_lt3_tuple_modified(m(SepKey::ListTuple3Tuple));
    ui_h.set_ps_sep_lt3_list(v(SepKey::ListTuple3List).into());
    ui_h.set_ps_sep_lt3_list_modified(m(SepKey::ListTuple3List));
    ui_h.set_ps_sep_lt4_tuple(v(SepKey::ListTuple4Tuple).into());
    ui_h.set_ps_sep_lt4_tuple_modified(m(SepKey::ListTuple4Tuple));
    ui_h.set_ps_sep_lt4_list(v(SepKey::ListTuple4List).into());
    ui_h.set_ps_sep_lt4_list_modified(m(SepKey::ListTuple4List));
    ui_h.set_ps_sep_mt2_kv(v(SepKey::MapTuple2Kv).into());
    ui_h.set_ps_sep_mt2_kv_modified(m(SepKey::MapTuple2Kv));
    ui_h.set_ps_sep_mt2_tuple(v(SepKey::MapTuple2Tuple).into());
    ui_h.set_ps_sep_mt2_tuple_modified(m(SepKey::MapTuple2Tuple));
    ui_h.set_ps_sep_mt2_entry(v(SepKey::MapTuple2Entry).into());
    ui_h.set_ps_sep_mt2_entry_modified(m(SepKey::MapTuple2Entry));
    ui_h.set_ps_sep_mt3_kv(v(SepKey::MapTuple3Kv).into());
    ui_h.set_ps_sep_mt3_kv_modified(m(SepKey::MapTuple3Kv));
    ui_h.set_ps_sep_mt3_tuple(v(SepKey::MapTuple3Tuple).into());
    ui_h.set_ps_sep_mt3_tuple_modified(m(SepKey::MapTuple3Tuple));
    ui_h.set_ps_sep_mt3_entry(v(SepKey::MapTuple3Entry).into());
    ui_h.set_ps_sep_mt3_entry_modified(m(SepKey::MapTuple3Entry));
    ui_h.set_ps_sep_mt4_kv(v(SepKey::MapTuple4Kv).into());
    ui_h.set_ps_sep_mt4_kv_modified(m(SepKey::MapTuple4Kv));
    ui_h.set_ps_sep_mt4_tuple(v(SepKey::MapTuple4Tuple).into());
    ui_h.set_ps_sep_mt4_tuple_modified(m(SepKey::MapTuple4Tuple));
    ui_h.set_ps_sep_mt4_entry(v(SepKey::MapTuple4Entry).into());
    ui_h.set_ps_sep_mt4_entry_modified(m(SepKey::MapTuple4Entry));
    ui_h.set_ps_sep_ml_kv(v(SepKey::MapListKv).into());
    ui_h.set_ps_sep_ml_kv_modified(m(SepKey::MapListKv));
    ui_h.set_ps_sep_ml_item(v(SepKey::MapListItem).into());
    ui_h.set_ps_sep_ml_item_modified(m(SepKey::MapListItem));
    ui_h.set_ps_sep_ml_entry(v(SepKey::MapListEntry).into());
    ui_h.set_ps_sep_ml_entry_modified(m(SepKey::MapListEntry));
}

/// 一个下拉字段：选项 + 选中下标 + 「改过吗」。
fn push_combo(
    ui_h: &AppWindow,
    options: Vec<&'static str>,
    value: &str,
    modified: bool,
    set_options: impl Fn(&AppWindow, slint::ModelRc<slint::SharedString>),
    set_index: impl Fn(&AppWindow, i32),
    set_modified: impl Fn(&AppWindow, bool),
) {
    // 值不在选项里（手改过 project.toml）时落到第一项，和原来 `unwrap_or(0)` 一致
    let idx = options.iter().position(|o| *o == value).unwrap_or(0) as i32;
    let model: Vec<slint::SharedString> = options.into_iter().map(Into::into).collect();
    set_options(ui_h, slint::ModelRc::new(slint::VecModel::from(model)));
    set_index(ui_h, idx);
    set_modified(ui_h, modified);
}

/// 导出 tab：29 个叶子。值 / 下标 / 布尔三态，成对推「值 + 改没改」。
fn push_validate(ui_h: &AppWindow, p: &Project) {
    let v = &p.config.validate;
    ui_h.set_ps_validate_error_index(v.error.to_index());
    ui_h.set_ps_validate_warning_index(v.warning.to_index());
    ui_h.set_ps_validate_info_index(v.info.to_index());
}

fn push_export(ui_h: &AppWindow, p: &Project) {
    let (live, orig) = (&p.config.export, &p.original_export);
    let v = |k: ExportKey| k.get(live);
    let m = |k: ExportKey| k.modified(live, orig);

    push_combo(ui_h, Encoding::all_str(), &v(ExportKey::Encoding), m(ExportKey::Encoding),
        |u, x| u.set_ps_encoding_options(x),
        |u, i| u.set_ps_export_encoding_index(i),
        |u, b| u.set_ps_export_encoding_modified(b));
    push_combo(ui_h, LineEnding::all_str(), &v(ExportKey::LineEnding), m(ExportKey::LineEnding),
        |u, x| u.set_ps_line_ending_options(x),
        |u, i| u.set_ps_export_line_ending_index(i),
        |u, b| u.set_ps_export_line_ending_modified(b));
    push_combo(ui_h, JsonEmptyAs::all_str(), &v(ExportKey::JsonEmptyAs), m(ExportKey::JsonEmptyAs),
        |u, x| u.set_ps_json_empty_as_options(x),
        |u, i| u.set_ps_export_json_empty_as_index(i),
        |u, b| u.set_ps_export_json_empty_as_modified(b));
    push_combo(ui_h, XmlEmptyAs::all_str(), &v(ExportKey::XmlEmptyAs), m(ExportKey::XmlEmptyAs),
        |u, x| u.set_ps_xml_empty_as_options(x),
        |u, i| u.set_ps_export_xml_empty_as_index(i),
        |u, b| u.set_ps_export_xml_empty_as_modified(b));
    push_combo(ui_h, CppJsonLib::all_str(), &v(ExportKey::CppJsonLib), m(ExportKey::CppJsonLib),
        |u, x| u.set_ps_cpp_json_lib_options(x),
        |u, i| u.set_ps_export_cpp_json_lib_index(i),
        |u, b| u.set_ps_export_cpp_json_lib_modified(b));

    // 服务端 —— 字符串
    ui_h.set_ps_export_server_data_output(v(ExportKey::ServerDataOutput).into());
    ui_h.set_ps_export_server_data_output_modified(m(ExportKey::ServerDataOutput));
    ui_h.set_ps_export_java_package(v(ExportKey::JavaPackage).into());
    ui_h.set_ps_export_java_package_modified(m(ExportKey::JavaPackage));
    ui_h.set_ps_export_java_code_output(v(ExportKey::JavaCodeOutput).into());
    ui_h.set_ps_export_java_code_output_modified(m(ExportKey::JavaCodeOutput));
    ui_h.set_ps_export_go_package(v(ExportKey::GoPackage).into());
    ui_h.set_ps_export_go_package_modified(m(ExportKey::GoPackage));
    ui_h.set_ps_export_go_code_output(v(ExportKey::GoCodeOutput).into());
    ui_h.set_ps_export_go_code_output_modified(m(ExportKey::GoCodeOutput));
    ui_h.set_ps_export_cpp_namespace(v(ExportKey::CppNamespace).into());
    ui_h.set_ps_export_cpp_namespace_modified(m(ExportKey::CppNamespace));
    ui_h.set_ps_export_cpp_code_output(v(ExportKey::CppCodeOutput).into());
    ui_h.set_ps_export_cpp_code_output_modified(m(ExportKey::CppCodeOutput));
    ui_h.set_ps_export_csharp_dotnet_namespace(v(ExportKey::CsharpDotnetNamespace).into());
    ui_h.set_ps_export_csharp_dotnet_namespace_modified(m(ExportKey::CsharpDotnetNamespace));
    ui_h.set_ps_export_csharp_dotnet_code_output(v(ExportKey::CsharpDotnetCodeOutput).into());
    ui_h.set_ps_export_csharp_dotnet_code_output_modified(m(ExportKey::CsharpDotnetCodeOutput));
    ui_h.set_ps_export_typescript_server_code_output(v(ExportKey::TypescriptServerCodeOutput).into());
    ui_h.set_ps_export_typescript_server_code_output_modified(m(ExportKey::TypescriptServerCodeOutput));
    // 服务端 —— 布尔
    ui_h.set_ps_export_java_lombok(v(ExportKey::JavaLombok) == "true");
    ui_h.set_ps_export_java_lombok_modified(m(ExportKey::JavaLombok));
    ui_h.set_ps_export_java_custom_regions(v(ExportKey::JavaCustomRegions) == "true");
    ui_h.set_ps_export_java_custom_regions_modified(m(ExportKey::JavaCustomRegions));
    ui_h.set_ps_export_go_custom_regions(v(ExportKey::GoCustomRegions) == "true");
    ui_h.set_ps_export_go_custom_regions_modified(m(ExportKey::GoCustomRegions));
    ui_h.set_ps_export_cpp_custom_regions(v(ExportKey::CppCustomRegions) == "true");
    ui_h.set_ps_export_cpp_custom_regions_modified(m(ExportKey::CppCustomRegions));
    ui_h.set_ps_export_csharp_dotnet_custom_regions(v(ExportKey::CsharpDotnetCustomRegions) == "true");
    ui_h.set_ps_export_csharp_dotnet_custom_regions_modified(m(ExportKey::CsharpDotnetCustomRegions));

    // 客户端 —— 字符串
    ui_h.set_ps_export_lua_output(v(ExportKey::LuaOutput).into());
    ui_h.set_ps_export_lua_output_modified(m(ExportKey::LuaOutput));
    ui_h.set_ps_export_gdscript_output(v(ExportKey::GdscriptOutput).into());
    ui_h.set_ps_export_gdscript_output_modified(m(ExportKey::GdscriptOutput));
    ui_h.set_ps_export_typescript_client_output(v(ExportKey::TypescriptClientOutput).into());
    ui_h.set_ps_export_typescript_client_output_modified(m(ExportKey::TypescriptClientOutput));
    ui_h.set_ps_export_csharp_unity_namespace(v(ExportKey::CsharpUnityNamespace).into());
    ui_h.set_ps_export_csharp_unity_namespace_modified(m(ExportKey::CsharpUnityNamespace));
    ui_h.set_ps_export_csharp_unity_code_output(v(ExportKey::CsharpUnityCodeOutput).into());
    ui_h.set_ps_export_csharp_unity_code_output_modified(m(ExportKey::CsharpUnityCodeOutput));
    ui_h.set_ps_export_csharp_godot_namespace(v(ExportKey::CsharpGodotNamespace).into());
    ui_h.set_ps_export_csharp_godot_namespace_modified(m(ExportKey::CsharpGodotNamespace));
    ui_h.set_ps_export_csharp_godot_code_output(v(ExportKey::CsharpGodotCodeOutput).into());
    ui_h.set_ps_export_csharp_godot_code_output_modified(m(ExportKey::CsharpGodotCodeOutput));
    // 客户端 —— 布尔
    ui_h.set_ps_export_csharp_unity_custom_regions(v(ExportKey::CsharpUnityCustomRegions) == "true");
    ui_h.set_ps_export_csharp_unity_custom_regions_modified(m(ExportKey::CsharpUnityCustomRegions));
    ui_h.set_ps_export_csharp_godot_custom_regions(v(ExportKey::CsharpGodotCustomRegions) == "true");
    ui_h.set_ps_export_csharp_godot_custom_regions_modified(m(ExportKey::CsharpGodotCustomRegions));
}

// ─── 关闭 ─────────────────────────────────────────────────────────────────

/// **所有关闭路径都走这里**（`确定`、点遮罩）。少了这个收口，
/// 某条路径就会带着已经改了的分隔符关掉而不重校验，grid 上的红框一直是旧的。
fn close_and_apply(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let (sep_touched, project_id) = {
        let st = state.borrow();
        (st.project_settings.sep_touched, st.project_settings.project_id.clone())
    };

    if sep_touched {
        // `revalidate_all` 是 **active 作用域**的，而对话框可能开在非 active 项目上。
        // 临时切过去、跑完切回来 —— 与原 `run()` 里那段一致。
        let mut st = state.borrow_mut();
        let prev = st.engine.active_project_id().map(str::to_string);
        if st.engine.set_active_by_id(&project_id) {
            st.engine.revalidate_all();
        }
        match prev {
            Some(id) => { st.engine.set_active_by_id(&id); }
            None => st.engine.set_active_none(),
        }
    }

    state.borrow_mut().project_settings.close();
    push(ui_h, state);
    // 树上项目的 🟡、grid 里的红框都可能变了，两处都要刷
    refresh::after_tree_change(ui_h, state);
}

// ─── 接线 ─────────────────────────────────────────────────────────────────

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

    // 身份三字段
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ps_name_edited(move |v| {
            edit_project(&s, |p| p.schema.meta.name = v.to_string());
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ps_category_edited(move |v| {
            edit_project(&s, |p| p.schema.meta.category = v.to_string());
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ps_version_edited(move |v| {
            edit_project(&s, |p| p.schema.meta.version = v.to_string());
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    // 身份字段还原
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ps_identity_revert(move |field| {
            let mut err: Option<String> = None;
            match original_of(&s, |p| p.original_meta.clone()) {
                None => {}
                Some(o) => match field.as_str() {
                    "name" => edit_project(&s, |p| p.schema.meta.name = o.name.clone()),
                    "category" => edit_project(&s, |p| p.schema.meta.category = o.category.clone()),
                    "version" => edit_project(&s, |p| p.schema.meta.version = o.version.clone()),
                    // 认不出来的 key 要吵 —— 导出那边静默吞掉三个 key，就是这么漏的
                    _ => err = Some(format!("不认识的身份字段: {}", field)),
                },
            }
            if let Some(e) = err {
                s.borrow_mut().engine.error_log(format!("[项目设置] {}", e));
            }
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }

    // 分隔符叶子编辑 / 还原
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ps_sep_edited(move |key, value| {
            match SepKey::from_directive_key(&key) {
                Some(k) => write_sep(&s, |sep| k.set(sep, value.to_string())),
                None => s.borrow_mut().engine.error_log(format!("[项目设置] 不认识的分隔符字段: {}", key)),
            }
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ps_sep_revert(move |key| {
            match SepKey::from_directive_key(&key) {
                Some(k) => {
                    let orig = original_of(&s, |p| p.original_separators.clone());
                    if let Some(o) = orig {
                        write_sep(&s, |sep| k.set(sep, k.get(&o).to_string()));
                    }
                }
                None => s.borrow_mut().engine.error_log(format!("[项目设置] 不认识的分隔符字段: {}", key)),
            }
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }

    // 校验策略（@01.10.7）：级别 → 动作
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ps_validate_field_edited(move |level, label| {
            let action = match label.as_str() {
                "仅提示" => SeverityAction::Report,
                "忽略" => SeverityAction::Ignore,
                _ => SeverityAction::Block,
            };
            {
                let mut st = s.borrow_mut();
                let id = st.project_settings.project_id.clone();
                if let Some(p) = st.engine.find_project_mut(&id) {
                    match level.as_str() {
                        "error" => p.config.validate.error = action,
                        "warning" => p.config.validate.warning = action,
                        "info" => p.config.validate.info = action,
                        _ => {}
                    }
                }
                // 策略变了 → block / report 的分流要重算（对话框是模态的，重算一次即可）
                st.engine.revalidate_all();
            }
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }

    // 导出叶子编辑 / 还原
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ps_export_field_edited(move |key, value| {
            let mut err: Option<String> = None;
            match ExportKey::from_str(&key) {
                None => err = Some(format!("不认识的导出字段: {}", key)),
                Some(k) => {
                    let ok = {
                        let mut st = s.borrow_mut();
                        let id = st.project_settings.project_id.clone();
                        match st.engine.find_project_mut(&id) {
                            Some(p) => k.set(&mut p.config.export, &value),
                            None => true,
                        }
                    };
                    if !ok {
                        err = Some(format!("导出配置不接受 {} = {:?}（值非法，保持原样）", key, value));
                    }
                }
            }
            if let Some(e) = err {
                s.borrow_mut().engine.error_log(format!("[项目设置] {}", e));
            }
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ps_export_revert(move |key| {
            match ExportKey::from_str(&key) {
                None => s.borrow_mut().engine.error_log(format!("[项目设置] 不认识的导出字段: {}", key)),
                Some(k) => {
                    let orig = original_of(&s, |p| p.original_export.clone());
                    if let Some(o) = orig {
                        edit_project(&s, |p| k.revert(&mut p.config.export, &o));
                    }
                }
            }
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }

    // 应用模板：把全局配置的 separators / export 灌进本项目。
    // 刻意保留 —— 它和「还原」不是一回事：还原的目标是**上次保存值**，套用的目标是**全局值**。
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ps_reset_sep(move || {
            let template = {
                let st = s.borrow();
                st.engine.global_config.separators.clone()
            };
            write_sep(&s, |sep| *sep = template);
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ps_reset_export(move || {
            let template = {
                let st = s.borrow();
                st.engine.global_config.export.clone().unwrap_or_default()
            };
            edit_project(&s, |p| p.config.export = template);
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }

    // 全部还原
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ps_revert_all(move || {
            {
                let mut st = s.borrow_mut();
                let id = st.project_settings.project_id.clone();
                let mut sep_changed = false;
                if let Some(p) = st.engine.find_project_mut(&id) {
                    let before = p.schema.separators.clone();
                    p.revert_settings();
                    sep_changed = p.schema.separators != before;
                }
                if sep_changed {
                    st.project_settings.sep_touched = true;
                }
            }
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }

    // 确认（= 关闭）与遮罩点击，都走同一条收口
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ps_confirm(move || {
            if let Some(ui_h) = weak.upgrade() { close_and_apply(&ui_h, &s); }
        });
    }
}
