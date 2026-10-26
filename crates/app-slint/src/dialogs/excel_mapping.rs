//! 「导入任意 Excel（映射）」对话框。
//!
//! 布局：顶部文件/组名 → Sheet tabs → 每个 tab 的 A（原始数据）/ B（映射调整）/ C（最终效果）
//! → 固定底部操作栏。每个 sheet 独立保存映射状态。

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use slint::{ComponentHandle, ModelRc, VecModel};

use crate::state::{AppState, ExcelMapFieldState, ExcelMapSheetState, ExcelMappingState};
use crate::{refresh, AppWindow, ExcelMapRow, ExcelPreviewRow, ExcelSheetTab, ExcelSourceCol};

const TYPE_OPTIONS: [&str; 5] = ["int", "float", "bool", "str", "txt"];
const EXPORT_OPTIONS: [&str; 4] = ["前后端", "客户端", "服务器", "不导出"];

fn export_index(e: &tablet_core::model::Export) -> i32 {
    tablet_core::model::Export::options()
        .iter()
        .position(|x| x == e)
        .unwrap_or(0) as i32
}

fn export_from_index(i: i32) -> tablet_core::model::Export {
    tablet_core::model::Export::options()
        .get(i.max(0) as usize)
        .cloned()
        .unwrap_or(tablet_core::model::Export::ClientServer)
}

fn type_index(t: &str) -> i32 {
    TYPE_OPTIONS.iter().position(|x| x == &t).unwrap_or(3) as i32
}

fn default_group_name(path: &Path) -> String {
    let raw = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    if tablet_core::validate::is_valid_group_name(&raw) { raw } else { "imported".to_string() }
}

pub fn open(state: &Rc<RefCell<AppState>>, path: PathBuf, project_id: &str) -> Result<(), String> {
    let sheets = tablet_core::excel_mapping::list_sheets(&path)
        .map_err(|e| format!("{:#}", e))?;
    if sheets.is_empty() {
        return Err("工作簿没有 sheet".to_string());
    }

    {
        let mut st = state.borrow_mut();
        let mut em = ExcelMappingState::default();
        em.open = true;
        em.project_id = project_id.to_string();
        em.file = path.to_string_lossy().to_string();
        em.group_name = default_group_name(&path);
        for sheet in sheets {
            let mut ss = ExcelMapSheetState::new(sheet);
            let p = PathBuf::from(&em.file);
            if let Ok(hr) = tablet_core::excel_mapping::detect_header_row(&p, &ss.sheet) {
                ss.header_row = hr as i32;
                ss.data_start = hr as i32 + 1;
                if let Ok((d, t, e)) = tablet_core::excel_mapping::detect_semantic_rows(&p, &ss.sheet, hr) {
                    ss.desc_row = d as i32;
                    ss.type_row = t as i32;
                    ss.export_row = e as i32;
                }
            }
            em.sheets.push(ss);
        }
        em.active_index = 0;
        st.excel_mapping = em;
    }
    let n = state.borrow().excel_mapping.sheets.len();
    for i in 0..n {
        refresh_sheet(state, i, true);
    }
    Ok(())
}

fn refresh_sheet(state: &Rc<RefCell<AppState>>, index: usize, reset_fields: bool) {
    let (path, sheet, header, data, columns_len) = {
        let st = state.borrow();
        let Some(s) = st.excel_mapping.sheets.get(index) else { return };
        (
            PathBuf::from(&st.excel_mapping.file),
            s.sheet.clone(),
            s.header_row.max(1) as u32,
            s.data_start.max(1) as u32,
            s.columns.len(),
        )
    };
    match tablet_core::excel_mapping::read_sheet_preview(&path, &sheet, header, data, 30) {
        Ok(preview) => {
            let mut st = state.borrow_mut();
            let Some(s) = st.excel_mapping.sheets.get_mut(index) else { return };
            s.columns = preview.columns.clone();
            s.preview_rows = preview.rows.clone();
            if reset_fields || s.fields.is_empty() || columns_len != preview.columns.len() {
                s.fields = tablet_core::excel_mapping::suggest_fields(&preview)
                    .into_iter()
                    .map(|f| ExcelMapFieldState {
                        target_name: f.target_name,
                        source_index: f.source_col as i32 - 1,
                        tbl_type: f.tbl_type,
                        export: f.export,
                        desc: f.desc,
                    })
                    .collect();
            }
        }
        Err(e) => {
            let mut st = state.borrow_mut();
            if let Some(s) = st.excel_mapping.sheets.get_mut(index) {
                s.error = format!("{:#}", e);
                s.can_import = false;
            }
            return;
        }
    }
    update_sheet_status(state, index);
}

fn apply_semantic_rows(state: &Rc<RefCell<AppState>>, index: usize) {
    let (path, sheet, desc_row, type_row, export_row, source_cols) = {
        let st = state.borrow();
        let Some(s) = st.excel_mapping.sheets.get(index) else { return };
        (
            PathBuf::from(&st.excel_mapping.file),
            s.sheet.clone(),
            s.desc_row.max(0) as u32,
            s.type_row.max(0) as u32,
            s.export_row.max(0) as u32,
            s.fields.iter().map(|f| f.source_index).collect::<Vec<_>>(),
        )
    };
    let read = |row: u32| -> Vec<String> {
        if row == 0 { Vec::new() } else {
            tablet_core::excel_mapping::read_row_values(&path, &sheet, row).unwrap_or_default()
        }
    };
    let descs = read(desc_row);
    let types = read(type_row);
    let exports = read(export_row);
    let mut st = state.borrow_mut();
    let Some(s) = st.excel_mapping.sheets.get_mut(index) else { return };
    for (i, src) in source_cols.iter().enumerate() {
        let Some(f) = s.fields.get_mut(i) else { continue };
        let idx = *src;
        if idx < 0 { continue; }
        let idx = idx as usize;
        if let Some(v) = descs.get(idx) {
            if !v.trim().is_empty() { f.desc = v.clone(); }
        }
        if let Some(v) = types.get(idx) {
            if tablet_core::types::TblType::parse(v).is_some() { f.tbl_type = v.trim().to_string(); }
        }
        if let Some(v) = exports.get(idx) {
            if let Some(e) = tablet_core::excel_mapping::parse_export_value(v) { f.export = e; }
        }
    }
}

fn plan_for_sheet(
    state: &Rc<RefCell<AppState>>,
    index: usize,
) -> Result<tablet_core::excel_mapping::MappingPlan, String> {
    let st = state.borrow();
    let s = st.excel_mapping.sheets.get(index).ok_or_else(|| "没有选择 sheet".to_string())?;
    if s.header_row <= 0 || s.data_start <= 0 {
        return Err("表头行 / 数据起始行必须是正整数".to_string());
    }
    let mut fields = Vec::with_capacity(s.fields.len());
    for f in &s.fields {
        if f.source_index < 0 || f.source_index as usize >= s.columns.len() {
            return Err(format!("字段 {} 没有选择源列", f.target_name));
        }
        fields.push(tablet_core::excel_mapping::MappedField {
            target_name: f.target_name.clone(),
            source_col: f.source_index as u32 + 1,
            tbl_type: f.tbl_type.clone(),
            export: f.export.clone(),
            desc: f.desc.clone(),
        });
    }
    let plan = tablet_core::excel_mapping::MappingPlan {
        source: PathBuf::from(&st.excel_mapping.file),
        sheet: s.sheet.clone(),
        header_row: s.header_row as u32,
        data_start: s.data_start as u32,
        desc_row: s.desc_row.max(0) as u32,
        type_row: s.type_row.max(0) as u32,
        export_row: s.export_row.max(0) as u32,
        group_name: st.excel_mapping.group_name.clone(),
        node_name: s.node_name.clone(),
        fields,
    };
    tablet_core::excel_mapping::validate_plan(&plan)?;
    Ok(plan)
}

fn header_values(s: &ExcelMapSheetState) -> (Vec<String>, Vec<String>, Vec<String>, Vec<String>) {
    let desc: Vec<String> = s.fields.iter().map(|f| f.desc.clone()).collect();
    let export: Vec<String> = s.fields.iter()
        .map(|f| EXPORT_OPTIONS[export_index(&f.export).max(0) as usize].to_string())
        .collect();
    let types: Vec<String> = s.fields.iter().map(|f| f.tbl_type.clone()).collect();
    let names: Vec<String> = s.fields.iter().map(|f| f.target_name.clone()).collect();
    (desc, export, types, names)
}

fn set_header_values(ui_h: &AppWindow, s: &ExcelMapSheetState) {
    let (desc, export, types, names) = header_values(s);
    let to_model = |v: Vec<String>| ModelRc::new(VecModel::from(
        v.into_iter().map(slint::SharedString::from).collect::<Vec<_>>()
    ));
    ui_h.set_em_header_desc(to_model(desc));
    ui_h.set_em_header_export(to_model(export));
    ui_h.set_em_header_type(to_model(types));
    ui_h.set_em_header_field(to_model(names));
}

fn update_sheet_status(state: &Rc<RefCell<AppState>>, index: usize) {
    let result = plan_for_sheet(state, index);
    let mut st = state.borrow_mut();
    if let Some(s) = st.excel_mapping.sheets.get_mut(index) {
        match result {
            Ok(_) => {
                s.error.clear();
                s.can_import = true;
            }
            Err(e) => {
                s.error = e;
                s.can_import = false;
            }
        }
    }
}

fn update_all_statuses(state: &Rc<RefCell<AppState>>) {
    let n = state.borrow().excel_mapping.sheets.len();
    for i in 0..n {
        update_sheet_status(state, i);
    }
}

fn source_cols_model(s: &ExcelMapSheetState) -> Vec<ExcelSourceCol> {
    s.columns.iter().enumerate().map(|(i, label)| {
        let mapped = s.fields.iter().find(|f| f.source_index == i as i32);
        ExcelSourceCol {
            label: label.clone().into(),
            target: mapped.map(|f| f.target_name.clone()).unwrap_or_else(|| "忽略".to_string()).into(),
            mapped: mapped.is_some(),
        }
    }).collect()
}

fn mapped_result_preview(s: &ExcelMapSheetState) -> (Vec<String>, Vec<Vec<String>>) {
    let columns = s.fields.iter().map(|f| f.target_name.clone()).collect();
    let rows = s.preview_rows.iter().map(|r| {
        s.fields.iter().map(|f| {
            if f.source_index >= 0 && (f.source_index as usize) < r.len() {
                r[f.source_index as usize].clone()
            } else {
                String::new()
            }
        }).collect()
    }).collect();
    (columns, rows)
}

fn preview_rows_model(rows: &[Vec<String>]) -> Vec<ExcelPreviewRow> {
    rows.iter().map(|r| ExcelPreviewRow {
        cells: ModelRc::new(VecModel::from(
            r.iter().map(|s| slint::SharedString::from(s.as_str())).collect::<Vec<_>>()
        )),
    }).collect()
}

fn tabs_model(em: &ExcelMappingState) -> Vec<ExcelSheetTab> {
    em.sheets.iter().map(|s| ExcelSheetTab {
        name: s.sheet.clone().into(),
        status: if s.can_import { 0 } else if !s.error.is_empty() { 1 } else { 2 },
    }).collect()
}

fn push_mapped_previews(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let st = state.borrow();
    let Some(s) = st.excel_mapping.active() else { return };
    let source_cols = source_cols_model(s);
    let (_out_cols, out_rows) = mapped_result_preview(s);
    ui_h.set_em_source_cols(ModelRc::new(VecModel::from(source_cols)));
    ui_h.set_em_mapped_rows(ModelRc::new(VecModel::from(preview_rows_model(&out_rows))));
}

fn push_status(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let st = state.borrow();
    let em = &st.excel_mapping;
    if let Some(s) = em.active() {
        ui_h.set_em_error_text(s.error.clone().into());
        ui_h.set_em_can_import(s.can_import);
        set_header_values(ui_h, s);
    }
    ui_h.set_em_tabs(ModelRc::new(VecModel::from(tabs_model(em))));
    ui_h.set_em_active_tab(em.active_index);
    ui_h.set_em_ready_count(em.sheets.iter().filter(|s| s.can_import).count() as i32);
    ui_h.set_em_sheet_count(em.sheets.len() as i32);
    drop(st);
    push_mapped_previews(ui_h, state);
}

pub fn push(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let st = state.borrow();
    let em = &st.excel_mapping;
    ui_h.set_dlg_excel_map_open(em.open);
    if !em.open { return; }
    ui_h.set_em_file_name(em.file.clone().into());
    ui_h.set_em_group_name(em.group_name.clone().into());
    ui_h.set_em_tabs(ModelRc::new(VecModel::from(tabs_model(em))));
    ui_h.set_em_active_tab(em.active_index);
    ui_h.set_em_ready_count(em.sheets.iter().filter(|s| s.can_import).count() as i32);
    ui_h.set_em_sheet_count(em.sheets.len() as i32);
    let Some(s) = em.active() else { return };
    ui_h.set_em_header_row(s.header_row.to_string().into());
    ui_h.set_em_data_start(s.data_start.to_string().into());
    ui_h.set_em_desc_row(s.desc_row.to_string().into());
    ui_h.set_em_type_row(s.type_row.to_string().into());
    ui_h.set_em_export_row(s.export_row.to_string().into());
    ui_h.set_em_node_name(s.node_name.clone().into());
    ui_h.set_em_error_text(s.error.clone().into());
    ui_h.set_em_can_import(s.can_import);

    let columns: Vec<slint::SharedString> = s.columns.iter().map(|c| c.clone().into()).collect();
    ui_h.set_em_preview_columns(ModelRc::new(VecModel::from(columns.clone())));
    ui_h.set_em_column_options(ModelRc::new(VecModel::from(columns)));
    ui_h.set_em_preview_rows(ModelRc::new(VecModel::from(preview_rows_model(&s.preview_rows))));

    let fields: Vec<ExcelMapRow> = s.fields.iter().map(|f| ExcelMapRow {
        target_name: f.target_name.clone().into(),
        source_index: f.source_index,
        source_label: if f.source_index >= 0 && (f.source_index as usize) < s.columns.len() {
            s.columns[f.source_index as usize].clone().into()
        } else {
            "".into()
        },
        type_index: type_index(&f.tbl_type),
        export_index: export_index(&f.export),
        desc: f.desc.clone().into(),
    }).collect();
    ui_h.set_em_fields(ModelRc::new(VecModel::from(fields)));

    let types: Vec<slint::SharedString> = TYPE_OPTIONS.iter().map(|s| (*s).into()).collect();
    let exports: Vec<slint::SharedString> = EXPORT_OPTIONS.iter().map(|s| (*s).into()).collect();
    ui_h.set_em_type_options(ModelRc::new(VecModel::from(types)));
    ui_h.set_em_export_options(ModelRc::new(VecModel::from(exports)));
    set_header_values(ui_h, s);
    drop(st);
    push_mapped_previews(ui_h, state);
}

fn with_active<F: FnOnce(&mut ExcelMapSheetState)>(state: &Rc<RefCell<AppState>>, f: F) {
    if let Some(s) = state.borrow_mut().excel_mapping.active_mut() {
        f(s);
    }
}

pub fn wire(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_em_source_col_toggled(move |i, checked| {
            let idx = s.borrow().excel_mapping.active_index.max(0) as usize;
            {
                let mut st = s.borrow_mut();
                if let Some(sh) = st.excel_mapping.sheets.get_mut(idx) {
                    let src = i.max(0) as usize;
                    if checked {
                        if !sh.fields.iter().any(|f| f.source_index == src as i32) {
                            let raw = sh.columns.get(src).cloned().unwrap_or_default();
                            let mut name = tablet_core::excel_mapping::normalize_field_name(&raw);
                            if name.is_empty() { name = format!("field_{}", src + 1); }
                            let base = name.clone();
                            let mut n = 2;
                            while sh.fields.iter().any(|f| f.target_name.eq_ignore_ascii_case(&name)) {
                                name = format!("{}_{}", base, n);
                                n += 1;
                            }
                            let samples: Vec<String> = sh.preview_rows.iter()
                                .filter_map(|r| r.get(src).cloned()).collect();
                            let ty = if name == "id" {
                                "int".to_string()
                            } else {
                                tablet_core::excel_mapping::infer_type(&samples).to_string()
                            };
                            sh.fields.push(ExcelMapFieldState {
                                target_name: name,
                                source_index: src as i32,
                                tbl_type: ty,
                                export: tablet_core::model::Export::ClientServer,
                                desc: raw,
                            });
                        }
                    } else {
                        sh.fields.retain(|f| f.source_index != src as i32);
                    }
                    // id 固定第一行；其余按源列顺序。
                    sh.fields.sort_by_key(|f| if f.target_name == "id" { 0 } else { f.source_index + 1 });
                }
            }
            update_sheet_status(&s, idx);
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_em_sheet_selected(move |i| {
            s.borrow_mut().excel_mapping.active_index = i;
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_em_header_row_changed(move |v| {
            if let Ok(n) = v.trim().parse::<i32>() {
                if n > 0 {
                    with_active(&s, |sh| {
                        sh.header_row = n;
                        sh.data_start = n + 1;
                        sh.fields.clear();
                    });
                    let i = s.borrow().excel_mapping.active_index.max(0) as usize;
                    refresh_sheet(&s, i, true);
                    if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
                }
            }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_em_data_start_changed(move |v| {
            if let Ok(n) = v.trim().parse::<i32>() {
                if n > 0 {
                    with_active(&s, |sh| sh.data_start = n);
                    let i = s.borrow().excel_mapping.active_index.max(0) as usize;
                    refresh_sheet(&s, i, false);
                    if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
                }
            }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_em_group_changed(move |v| {
            s.borrow_mut().excel_mapping.group_name = v.to_string();
            update_all_statuses(&s);
            if let Some(ui_h) = weak.upgrade() { push_status(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_em_node_changed(move |v| {
            with_active(&s, |sh| sh.node_name = v.to_string());
            let i = s.borrow().excel_mapping.active_index.max(0) as usize;
            update_sheet_status(&s, i);
            if let Some(ui_h) = weak.upgrade() { push_status(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_em_field_target_changed(move |i, v| {
            let mut moved = false;
            {
                let mut st = s.borrow_mut();
                if let Some(sh) = st.excel_mapping.active_mut() {
                    let i = i.max(0) as usize;
                    if let Some(f) = sh.fields.get_mut(i) {
                        f.target_name = v.to_string();
                    }
                    if i > 0 && sh.fields.get(i).map(|f| f.target_name == "id").unwrap_or(false) {
                        let f = sh.fields.remove(i);
                        sh.fields.insert(0, f);
                        moved = true;
                    }
                }
            }
            let i = s.borrow().excel_mapping.active_index.max(0) as usize;
            update_sheet_status(&s, i);
            if let Some(ui_h) = weak.upgrade() {
                if moved { push(&ui_h, &s); } else { push_status(&ui_h, &s); }
            }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_em_field_source_changed(move |i, v| {
            with_active(&s, |sh| {
                if let Some(f) = sh.fields.get_mut(i.max(0) as usize) { f.source_index = v; }
            });
            let idx = s.borrow().excel_mapping.active_index.max(0) as usize;
            update_sheet_status(&s, idx);
            if let Some(ui_h) = weak.upgrade() { push_status(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_em_field_type_changed(move |i, v| {
            let ty = TYPE_OPTIONS.get(v.max(0) as usize).copied().unwrap_or("str").to_string();
            with_active(&s, |sh| {
                if let Some(f) = sh.fields.get_mut(i.max(0) as usize) { f.tbl_type = ty; }
            });
            let idx = s.borrow().excel_mapping.active_index.max(0) as usize;
            update_sheet_status(&s, idx);
            if let Some(ui_h) = weak.upgrade() { push_status(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_em_field_export_changed(move |i, v| {
            let ex = export_from_index(v);
            with_active(&s, |sh| {
                if let Some(f) = sh.fields.get_mut(i.max(0) as usize) { f.export = ex; }
            });
            let idx = s.borrow().excel_mapping.active_index.max(0) as usize;
            update_sheet_status(&s, idx);
            if let Some(ui_h) = weak.upgrade() { push_status(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_em_field_desc_changed(move |i, v| {
            with_active(&s, |sh| {
                if let Some(f) = sh.fields.get_mut(i.max(0) as usize) { f.desc = v.to_string(); }
            });
            let idx = s.borrow().excel_mapping.active_index.max(0) as usize;
            update_sheet_status(&s, idx);
            if let Some(ui_h) = weak.upgrade() { push_status(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_em_desc_row_changed(move |v| {
            if let Ok(n) = v.trim().parse::<i32>() {
                if n >= 0 {
                    with_active(&s, |sh| sh.desc_row = n);
                    let idx = s.borrow().excel_mapping.active_index.max(0) as usize;
                    apply_semantic_rows(&s, idx);
                    update_sheet_status(&s, idx);
                    if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
                }
            }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_em_type_row_changed(move |v| {
            if let Ok(n) = v.trim().parse::<i32>() {
                if n >= 0 {
                    with_active(&s, |sh| sh.type_row = n);
                    let idx = s.borrow().excel_mapping.active_index.max(0) as usize;
                    apply_semantic_rows(&s, idx);
                    update_sheet_status(&s, idx);
                    if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
                }
            }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_em_export_row_changed(move |v| {
            if let Ok(n) = v.trim().parse::<i32>() {
                if n >= 0 {
                    with_active(&s, |sh| sh.export_row = n);
                    let idx = s.borrow().excel_mapping.active_index.max(0) as usize;
                    apply_semantic_rows(&s, idx);
                    update_sheet_status(&s, idx);
                    if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
                }
            }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_em_add_field(move || {
            with_active(&s, |sh| {
                let n = sh.fields.len() + 1;
                sh.fields.push(ExcelMapFieldState {
                    target_name: format!("field_{}", n),
                    source_index: 0,
                    tbl_type: "str".into(),
                    export: tablet_core::model::Export::ClientServer,
                    desc: String::new(),
                });
            });
            let idx = s.borrow().excel_mapping.active_index.max(0) as usize;
            update_sheet_status(&s, idx);
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_em_remove_field(move |i| {
            with_active(&s, |sh| {
                let i = i.max(0) as usize;
                if i < sh.fields.len() { sh.fields.remove(i); }
            });
            let idx = s.borrow().excel_mapping.active_index.max(0) as usize;
            update_sheet_status(&s, idx);
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_em_auto_match(move || {
            let (path, sheet, idx) = {
                let st = s.borrow();
                let idx = st.excel_mapping.active_index.max(0) as usize;
                let sh = st.excel_mapping.sheets.get(idx);
                (PathBuf::from(&st.excel_mapping.file), sh.map(|x| x.sheet.clone()), idx)
            };
            if let Some(sheet) = sheet {
                if let Ok(hr) = tablet_core::excel_mapping::detect_header_row(&path, &sheet) {
                    let mut st = s.borrow_mut();
                    if let Some(sh) = st.excel_mapping.sheets.get_mut(idx) {
                        sh.header_row = hr as i32;
                        sh.data_start = hr as i32 + 1;
                        sh.fields.clear();
                        if let Ok((d, t, e)) = tablet_core::excel_mapping::detect_semantic_rows(&path, &sheet, hr) {
                            sh.desc_row = d as i32;
                            sh.type_row = t as i32;
                            sh.export_row = e as i32;
                        }
                    }
                }
            }
            refresh_sheet(&s, idx, true);
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_em_cancel(move || {
            s.borrow_mut().excel_mapping = ExcelMappingState::default();
            if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_em_import_current(move || {
            let idx = s.borrow().excel_mapping.active_index.max(0) as usize;
            let plan = match plan_for_sheet(&s, idx) {
                Ok(p) => p,
                Err(e) => {
                    let mut st = s.borrow_mut();
                    if let Some(sh) = st.excel_mapping.sheets.get_mut(idx) { sh.error = e; }
                    drop(st);
                    if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
                    return;
                }
            };
            let project_id = s.borrow().excel_mapping.project_id.clone();
            let result = { s.borrow_mut().engine.import_mapped_excel(&project_id, &plan) };
            match result {
                Ok(out) => finish_import(&s, &weak, &project_id, &out.group, &[]),
                Err(e) => {
                    let mut st = s.borrow_mut();
                    if let Some(sh) = st.excel_mapping.sheets.get_mut(idx) { sh.error = e; }
                    drop(st);
                    if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
                }
            }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_em_import_all(move || {
            let project_id = s.borrow().excel_mapping.project_id.clone();
            let n = s.borrow().excel_mapping.sheets.len();
            let mut plans = Vec::new();
            let mut skipped = Vec::new();
            for i in 0..n {
                match plan_for_sheet(&s, i) {
                    Ok(p) => plans.push(p),
                    Err(e) => {
                        let name = s.borrow().excel_mapping.sheets.get(i)
                            .map(|x| x.sheet.clone()).unwrap_or_default();
                        skipped.push(format!("{}: {}", name, e));
                    }
                }
            }
            let mut imported = 0;
            let mut group = String::new();
            let mut errors = Vec::new();
            for plan in plans {
                match s.borrow_mut().engine.import_mapped_excel(&project_id, &plan) {
                    Ok(out) => {
                        imported += 1;
                        group = out.group;
                    }
                    Err(e) => errors.push(e),
                }
            }
            if imported == 0 {
                let mut st = s.borrow_mut();
                let msg = if errors.is_empty() { skipped.join("；") } else { errors.join("；") };
                if let Some(sh) = st.excel_mapping.active_mut() { sh.error = msg; }
                drop(st);
                if let Some(ui_h) = weak.upgrade() { push(&ui_h, &s); }
                return;
            }
            finish_import(&s, &weak, &project_id, &group, &skipped);
        });
    }
}

fn finish_import(
    state: &Rc<RefCell<AppState>>,
    weak: &slint::Weak<AppWindow>,
    project_id: &str,
    group: &str,
    skipped: &[String],
) {
    {
        let mut st = state.borrow_mut();
        st.excel_mapping = ExcelMappingState::default();
        st.project_expanded.insert(project_id.to_string());
        st.tree_expanded.insert((project_id.to_string(), group.to_string()));
        st.select_pasted_group(project_id, group);
        for s in skipped {
            st.engine.warn_log(format!("[映射导入] 跳过 {}", s));
        }
    }
    if let Some(ui_h) = weak.upgrade() {
        refresh::after_tree_change(&ui_h, state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn unique_tmp(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "tablet_excel_map_dialog_{}_{}_{}",
            label,
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ))
    }

    #[test]
    fn opening_a_workbook_initializes_every_sheet_without_double_borrow() {
        let dir = unique_tmp("open");
        let proj = dir.join("projects/demo");
        std::fs::create_dir_all(proj.join("config")).unwrap();
        std::fs::write(
            proj.join("project.tblmanifest"),
            "#!tblmanifest v1\n# @meta id: demo\n",
        ).unwrap();
        std::fs::write(dir.join("tablet.toml"), "").unwrap();
        let (st, _, _) = AppState::load(&dir).expect("load workspace");
        let state = Rc::new(RefCell::new(st));

        let path = dir.join("foreign.xlsx");
        let mut wb = rust_xlsxwriter::Workbook::new();
        for name in ["Sheet1", "Sheet2"] {
            let ws = wb.add_worksheet();
            ws.set_name(name).unwrap();
            ws.write_string(0, 0, "ID").unwrap();
            ws.write_string(0, 1, "Name").unwrap();
            ws.write_string(1, 0, "1").unwrap();
            ws.write_string(1, 1, "亚瑟").unwrap();
        }
        std::fs::write(&path, wb.save_to_buffer().unwrap()).unwrap();

        open(&state, path, "demo").expect("open");
        let st = state.borrow();
        assert_eq!(st.excel_mapping.sheets.len(), 2);
        assert!(!st.excel_mapping.sheets[0].fields.is_empty());
        assert!(!st.excel_mapping.sheets[1].fields.is_empty());

        drop(st);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
