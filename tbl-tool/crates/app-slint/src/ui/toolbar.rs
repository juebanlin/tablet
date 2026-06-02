// 顶部工具栏：generate-test / clear / save / reload + 导入 Schema + 模板库。
// 数据导出 / Schema 导出 已迁到 TreeProject 右键菜单（按 project 走）。

use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;

use crate::state;
use crate::state::{AppState, GridSelection};
use crate::{dialogs, refresh, ui, AppWindow};

pub fn wire(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let s = state.clone();
    let weak = ui_h.as_weak();
    ui_h.on_toolbar_btn_clicked(move |id| {
        let id = id.to_string();
        let mut full_refresh = false;
        let mut schema_import_dlg = false;
        let mut template_lib_dlg = false;
        match id.as_str() {
            "generate-test" => {
                s.borrow_mut().engine.generate_test_config();
                reset_view_after_reload(&s);
                full_refresh = true;
            }
            "clear" => {
                s.borrow_mut().engine.clear_all_config();
                reset_view_after_reload(&s);
                full_refresh = true;
            }
            "save" => {
                s.borrow_mut().engine.save_all_projects();
                // save_all_projects 内部跑 revalidate_all_projects 重算 validation_errors，
                // 必须刷新 tree（! 标记）和 grid（红框）才能让用户看到错误
                full_refresh = true;
            }
            "reload" => {
                s.borrow_mut().engine.reload();
                reset_view_after_reload(&s);
                full_refresh = true;
            }
            "import-schema" => {
                {
                    let mut st = s.borrow_mut();
                    st.schema_import = state::SchemaImportState::default();
                    st.schema_import.open = true;
                }
                schema_import_dlg = true;
            }
            "template-library" => {
                {
                    let mut st = s.borrow_mut();
                    st.template_lib.open = true;
                    st.template_lib.tab = 0;
                    st.template_lib.search.clear();
                    st.template_lib.selected_id.clear();
                }
                template_lib_dlg = true;
            }
            _ => {}
        }
        if let Some(ui_h) = weak.upgrade() {
            if full_refresh {
                ui::tree::push(&ui_h, &s);
                ui::grid::push(&ui_h, &s);
                dialogs::type_selector::push(&ui_h, &s);
                dialogs::ref_picker::push(&ui_h, &s);
                dialogs::context_menu::push(&ui_h, &s);
                dialogs::pending::push_input(&ui_h, &s);
                dialogs::pending::push_confirm(&ui_h, &s);
            }
            if schema_import_dlg { dialogs::schema_io::push_import(&ui_h, &s); }
            if template_lib_dlg { dialogs::template_library::push(&ui_h, &s); }
            // 任何 toolbar 操作都可能产生日志（save/reload/generate/clear 全会 log）
            refresh::after_log(&ui_h, &s);
        }
    });
}

/// reload / generate / clear 后清掉 UI 临时态：选中节点、grid 选区、编辑 buffer。
/// 同时重新展开 active project 下所有 group + active project 根；并跑一次全 Project 验证。
pub(crate) fn reset_view_after_reload(state: &Rc<RefCell<AppState>>) {
    let mut st = state.borrow_mut();
    st.selected = None;
    st.grid_selection = GridSelection::None;
    st.editing = None;
    st.editing_buffer.clear();
    st.editing_in_formula = false;
    st.editing_header_row = -1;
    st.editing_header_col = -1;
    st.type_selector.close();
    st.ref_picker.close();
    st.ctx_menu.close();
    st.pending.close();
    st.template_lib.open = false;
    st.new_project.close();
    if let Some(active_id) = st.engine.active_project_id().map(str::to_string) {
        st.tree_expanded = st.engine.project().groups.iter()
            .map(|g| (active_id.clone(), g.name.clone()))
            .collect();
        st.project_expanded = std::iter::once(active_id).collect();
    } else {
        st.tree_expanded.clear();
        st.project_expanded.clear();
    }
    // 跑一遍全 Project 验证，让所有节点的红框/`!` 标记可见。
    st.engine.revalidate_all_projects();
    if !st.engine.validation_errors.is_empty() {
        let n = st.engine.validation_errors.len();
        st.engine.log(format!("[验证] 共 {} 个错误", n));
    }
}
