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

use slint::ComponentHandle;
use tablet_core::types::{SepKey, SeparatorsSection};

use crate::state::AppState;
use crate::{refresh, AppWindow};

/// 打开全局设置对话框：从 engine.global_config 拷贝数据到 apply buffer，并保存 current 快照
pub fn open(state: &Rc<RefCell<AppState>>) {
    let mut st = state.borrow_mut();

    // current: 当前已保存的值（等于硬盘值）
    let current = st.engine.global_config.clone();

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
    ui_h.set_gs_ui_log_level(gs.ui.log_level.as_deref().unwrap_or("info").into());
    ui_h.set_gs_ui_picker_trigger_header(gs.ui.picker_trigger_header.clone().into());
    ui_h.set_gs_ui_picker_trigger_data(gs.ui.picker_trigger_data.clone().into());
    ui_h.set_gs_ui_show_meta_id(gs.ui.show_meta_id);
    ui_h.set_gs_ui_constant_ref_allowed(gs.ui.constant_ref_allowed);
    ui_h.set_gs_ui_ref_picker_strategy(gs.ui.ref_picker.default_strategy.clone().into());

    // 分隔符（25 个）
    push_separators(ui_h, &gs.sep);

    // 导出设置
    ui_h.set_gs_export_encoding(gs.export.encoding.as_deref().unwrap_or("utf-8").into());
    ui_h.set_gs_export_line_ending(gs.export.line_ending.as_deref().unwrap_or("lf").into());
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
            st.engine.log(format!("[全局设置] 写入失败: {}", e));
            return;
        }
        st.engine.log("[全局设置] 已保存到 tablet.toml".to_string());
    }

    // 重新合并所有项目的配置
    {
        let mut st = state.borrow_mut();
        let global_config = st.engine.global_config.clone();

        for p in &mut st.engine.projects {
            let merged = tablet_core::project::merge_config(
                &global_config,
                &p.raw_config,
                &p.schema,
            );
            p.config = merged;
        }

        // 更新 AppState 中的 UI 配置缓存
        st.realtime_validate = ui.realtime_validate;
        st.picker_trigger_header_single = ui.picker_trigger_header == "single";
        st.picker_trigger_data_single = ui.picker_trigger_data == "single";
        st.constant_ref_allowed = ui.constant_ref_allowed;
    }

    // 分隔符改变时重新验证所有数据
    if separators_changed {
        let mut st = state.borrow_mut();
        st.engine.revalidate_all_projects();
        let err_count = st.engine.validation_errors.len();
        st.engine.log(format!("[全局设置] 分隔符已更新，重新验证了 {} 个错误", err_count));
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
    state.borrow_mut().engine.log("[全局设置] wire 函数已调用，注册回调".to_string());

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
                    gs.ui.log_level = if value.is_empty() { None } else { Some(value.to_string()) };
                }
                "picker_trigger_header" => {
                    gs.ui.picker_trigger_header = value.to_string();
                }
                "picker_trigger_data" => {
                    gs.ui.picker_trigger_data = value.to_string();
                }
                "ref_picker_strategy" => {
                    gs.ui.ref_picker.default_strategy = value.to_string();
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
                    gs.export.encoding = if value.is_empty() { None } else { Some(value.to_string()) };
                }
                "line_ending" => {
                    gs.export.line_ending = if value.is_empty() { None } else { Some(value.to_string()) };
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
            s.borrow_mut().engine.log(format!("[全局设置] 分隔符 {} 编辑，modified={}", key, modified));
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
