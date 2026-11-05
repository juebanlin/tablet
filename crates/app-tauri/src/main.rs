// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 juebanlin <juebanlin@gmail.com>

//! Tauri 版 GUI：窗口 + 命令层。
//!
//! 启动分流 / 锁 / 日志 / 崩溃日志走本 crate 的 [`shell`]（@docs/17 §4）；
//! 业务状态在 [`backend::Backend`]（内部包 `tablet_core::ops::ProjectEngine`）。

mod backend;
mod shell;

use std::sync::Mutex;

use tauri::{Emitter, Manager};

use backend::{
    Backend, CreateProjectRequest, ExcelMapRequest, ExcelPollItem, ExcelPreview, ExportFormatsUi,
    CellIssue, ExcelSyncRowUi, ExportReport, ExportUi, GlobalSettingsUi, PasteGroupPreviewUi,
    PasteGroupRowUi,
    ProjectSettings, RefPickerData, RuleItem, RulesPayload,
    SchemaImportPreview, SchemaOutlineGroup, SchemaPreview, SepInput, Snapshot, TemplateItem,
    TypePickerInfo, TypePreview, UniverApplyResult, UniverGroupPayload, UniverOpenRequest,
    ValidateUi,
};

struct AppState {
    backend: Mutex<Backend>,
}

fn lock(state: &AppState) -> Result<std::sync::MutexGuard<'_, Backend>, String> {
    state.backend.lock().map_err(|e| format!("状态锁已损坏: {e}"))
}

#[tauri::command]
fn snapshot(state: tauri::State<'_, AppState>) -> Result<Snapshot, String> {
    Ok(lock(&state)?.snapshot())
}

#[tauri::command]
fn select_node(
    state: tauri::State<'_, AppState>,
    project: String,
    kind: String,
    group: Option<String>,
    name: Option<String>,
) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.select(&project, &kind, group.as_deref(), name.as_deref());
    Ok(b.snapshot())
}

#[tauri::command]
fn commit_cell(
    state: tauri::State<'_, AppState>,
    row: usize,
    col: usize,
    value: String,
) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.ensure_unlocked()?;
    b.commit_cell(row, col, value);
    Ok(b.snapshot())
}

#[tauri::command]
fn commit_header(
    state: tauri::State<'_, AppState>,
    header_row: usize,
    col: usize,
    value: String,
) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.ensure_unlocked()?;
    b.commit_header(header_row, col, value);
    Ok(b.snapshot())
}

#[tauri::command]
fn clear_cells(
    state: tauri::State<'_, AppState>,
    cells: Vec<(usize, usize)>,
) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.ensure_unlocked()?;
    b.clear_cells(cells);
    Ok(b.snapshot())
}

#[tauri::command]
fn paste_cells(
    state: tauri::State<'_, AppState>,
    row: usize,
    col: usize,
    cells: Vec<Vec<String>>,
) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.ensure_unlocked()?;
    b.paste_cells(row, col, cells);
    Ok(b.snapshot())
}

#[tauri::command]
fn insert_row(state: tauri::State<'_, AppState>, at: usize) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.ensure_unlocked()?;
    b.insert_row(at);
    Ok(b.snapshot())
}

#[tauri::command]
fn delete_rows(
    state: tauri::State<'_, AppState>,
    start: usize,
    end: usize,
) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.ensure_unlocked()?;
    b.delete_rows(start, end);
    Ok(b.snapshot())
}

#[tauri::command]
fn insert_column(state: tauri::State<'_, AppState>, at: usize) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.ensure_unlocked()?;
    b.insert_column(at);
    Ok(b.snapshot())
}

#[tauri::command]
fn delete_column(state: tauri::State<'_, AppState>, at: usize) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.ensure_unlocked()?;
    b.delete_column(at);
    Ok(b.snapshot())
}

// ── 节点增删改 ──────────────────────────────────────────────
#[tauri::command]
fn new_group(
    state: tauri::State<'_, AppState>,
    project: String,
    name: String,
    xlsx: bool,
) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.new_group(&project, name, xlsx);
    Ok(b.snapshot())
}

#[tauri::command]
fn new_node(
    state: tauri::State<'_, AppState>,
    project: String,
    group: String,
    kind: String,
    name: String,
    fmt: Option<String>,
) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.ensure_group_unlocked(&project, &group)?;
    b.new_node(&project, &group, &kind, name, fmt);
    Ok(b.snapshot())
}

#[tauri::command]
fn rename_group(
    state: tauri::State<'_, AppState>,
    project: String,
    old_name: String,
    new_name: String,
) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.ensure_group_unlocked(&project, &old_name)?;
    b.rename_group(&project, old_name, new_name);
    Ok(b.snapshot())
}

#[tauri::command]
fn rename_node(
    state: tauri::State<'_, AppState>,
    project: String,
    group: String,
    old_name: String,
    new_name: String,
) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.ensure_group_unlocked(&project, &group)?;
    b.rename_node(&project, &group, old_name, new_name);
    Ok(b.snapshot())
}

#[tauri::command]
fn delete_group(
    state: tauri::State<'_, AppState>,
    project: String,
    group: String,
) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.ensure_group_unlocked(&project, &group)?;
    b.delete_group(&project, &group);
    Ok(b.snapshot())
}

#[tauri::command]
fn delete_node(
    state: tauri::State<'_, AppState>,
    project: String,
    group: String,
    name: String,
) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.ensure_group_unlocked(&project, &group)?;
    b.delete_node(&project, &group, &name);
    Ok(b.snapshot())
}

#[tauri::command]
fn copy_node(
    state: tauri::State<'_, AppState>,
    project: String,
    group: String,
    name: String,
    kind: String,
) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.copy_node(&project, &group, &name, &kind);
    Ok(b.snapshot())
}

#[tauri::command]
fn copy_group(
    state: tauri::State<'_, AppState>,
    project: String,
    group: String,
) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.copy_group(&project, &group);
    Ok(b.snapshot())
}

#[tauri::command]
fn paste_node(
    state: tauri::State<'_, AppState>,
    project: String,
    group: String,
    fmt: Option<String>,
) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.ensure_group_unlocked(&project, &group)?;
    b.paste_node(&project, &group, fmt);
    Ok(b.snapshot())
}

#[tauri::command]
fn revert_node(
    state: tauri::State<'_, AppState>,
    project: String,
    group: String,
    name: String,
) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.ensure_group_unlocked(&project, &group)?;
    b.revert_node(&project, &group, &name);
    Ok(b.snapshot())
}

// ── 新建项目 / 项目设置 ─────────────────────────────────────
#[tauri::command]
fn create_project(
    state: tauri::State<'_, AppState>,
    request: CreateProjectRequest,
) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.create_project(request)?;
    Ok(b.snapshot())
}

#[tauri::command]
fn schema_preview(
    state: tauri::State<'_, AppState>,
    path: String,
) -> Result<SchemaPreview, String> {
    lock(&state)?.schema_preview(path)
}

#[tauri::command]
fn template_preview(
    state: tauri::State<'_, AppState>,
    template_id: String,
) -> Result<SchemaPreview, String> {
    lock(&state)?.template_preview(template_id)
}

#[tauri::command]
fn list_templates(state: tauri::State<'_, AppState>) -> Result<Vec<TemplateItem>, String> {
    Ok(lock(&state)?.list_templates())
}

#[tauri::command]
fn create_project_empty(
    state: tauri::State<'_, AppState>,
    id: String,
    name: String,
) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.create_project_empty(id, name)?;
    Ok(b.snapshot())
}

#[tauri::command]
fn create_project_from_template(
    state: tauri::State<'_, AppState>,
    id: String,
    name: String,
    template_id: String,
) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.create_project_from_template(id, name, template_id)?;
    Ok(b.snapshot())
}

#[tauri::command]
fn create_project_from_file(
    state: tauri::State<'_, AppState>,
    id: String,
    name: String,
    path: String,
) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.create_project_from_file(id, name, path)?;
    Ok(b.snapshot())
}

#[tauri::command]
fn project_settings(
    state: tauri::State<'_, AppState>,
    project: String,
) -> Result<ProjectSettings, String> {
    lock(&state)?
        .project_settings(&project)
        .ok_or_else(|| format!("项目不存在: {project}"))
}

#[tauri::command]
fn save_project_settings(
    state: tauri::State<'_, AppState>,
    project: String,
    name: String,
    category: String,
    version: String,
    separators: Vec<SepInput>,
    export_settings: ExportUi,
    validate: ValidateUi,
) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.save_project_settings(
        &project,
        name,
        category,
        version,
        separators,
        export_settings,
        validate,
    );
    Ok(b.snapshot())
}

// ── 校验规则 ────────────────────────────────────────────────
#[tauri::command]
fn rules_payload(
    state: tauri::State<'_, AppState>,
    project: String,
    group: String,
    name: String,
) -> Result<RulesPayload, String> {
    lock(&state)?.rules_payload(&project, &group, &name)
}

#[tauri::command]
fn rules_save(
    state: tauri::State<'_, AppState>,
    project: String,
    group: String,
    name: String,
    rules: Vec<RuleItem>,
) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.ensure_group_unlocked(&project, &group)?;
    b.rules_save(&project, &group, &name, rules)?;
    Ok(b.snapshot())
}

// ── 引用显示 ────────────────────────────────────────────────
#[tauri::command]
fn set_show_ref_name(state: tauri::State<'_, AppState>, on: bool) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.set_show_ref_name(on);
    Ok(b.snapshot())
}

#[tauri::command]
fn set_display_field(
    state: tauri::State<'_, AppState>,
    field: Option<String>,
) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.ensure_unlocked()?;
    b.set_display_field_selected(field);
    Ok(b.snapshot())
}

#[tauri::command]
fn save_grid_view(
    state: tauri::State<'_, AppState>,
    project: String,
    group: String,
    kind: String,
    name: String,
    auto: bool,
    widths: std::collections::BTreeMap<String, f32>,
) -> Result<(), String> {
    lock(&state)?.save_grid_view(&project, &group, &kind, &name, auto, widths)
}

#[tauri::command]
fn xlsx_poll(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<backend::XlsxExternalChangeUi>, String> {
    Ok(lock(&state)?.xlsx_poll())
}

#[tauri::command]
fn reload_xlsx_external(
    state: tauri::State<'_, AppState>,
    project: String,
    group: String,
) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.reload_xlsx_external(&project, &group)?;
    Ok(b.snapshot())
}

#[tauri::command]
fn set_project_sort(state: tauri::State<'_, AppState>, sort: String) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.set_project_sort(&sort)?;
    Ok(b.snapshot())
}

// ── 全局设置 / 导出执行 ─────────────────────────────────────
#[tauri::command]
fn global_settings(state: tauri::State<'_, AppState>) -> Result<GlobalSettingsUi, String> {
    Ok(lock(&state)?.global_settings())
}

#[tauri::command]
fn save_global_settings(
    state: tauri::State<'_, AppState>,
    settings: GlobalSettingsUi,
) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.save_global_settings(settings)?;
    Ok(b.snapshot())
}

#[tauri::command]
fn export_project(
    state: tauri::State<'_, AppState>,
    formats: ExportFormatsUi,
) -> Result<ExportReport, String> {
    Ok(lock(&state)?.export_project(formats))
}

// ── Schema / Excel ──────────────────────────────────────────
#[tauri::command]
fn schema_export(
    state: tauri::State<'_, AppState>,
    path: String,
    with_preset: bool,
    groups: Vec<String>,
    nodes: Vec<String>,
    meta_id: Option<String>,
    meta_name: Option<String>,
    meta_category: Option<String>,
    meta_version: Option<String>,
    as_template: bool,
) -> Result<String, String> {
    lock(&state)?.schema_export(
        path,
        with_preset,
        groups,
        nodes,
        meta_id,
        meta_name,
        meta_category,
        meta_version,
        as_template,
    )
}

#[tauri::command]
fn schema_outline(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<SchemaOutlineGroup>, String> {
    lock(&state)?.schema_outline()
}

#[tauri::command]
fn schema_import_preview(
    state: tauri::State<'_, AppState>,
    path: String,
) -> Result<SchemaImportPreview, String> {
    lock(&state)?.schema_import_preview(&path)
}

#[tauri::command]
fn schema_import(
    state: tauri::State<'_, AppState>,
    path: String,
    with_preset: bool,
    sections: Option<Vec<String>>,
) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.ensure_active_project_unlocked()?;
    b.schema_import(path, with_preset, sections)?;
    Ok(b.snapshot())
}

#[tauri::command]
fn excel_export(
    state: tauri::State<'_, AppState>,
    group: String,
    path: String,
) -> Result<String, String> {
    lock(&state)?.excel_export(&group, path)
}

#[tauri::command]
fn excel_import(
    state: tauri::State<'_, AppState>,
    group: String,
    path: String,
) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.excel_import(&group, path)?;
    Ok(b.snapshot())
}

#[tauri::command]
fn excel_files(state: tauri::State<'_, AppState>) -> Result<Vec<String>, String> {
    Ok(lock(&state)?.excel_files())
}

#[tauri::command]
fn excel_delete(state: tauri::State<'_, AppState>, file: String) -> Result<(), String> {
    lock(&state)?.excel_delete(&file)
}

#[tauri::command]
fn excel_sync_plan(state: tauri::State<'_, AppState>) -> Result<Vec<ExcelSyncRowUi>, String> {
    Ok(lock(&state)?.excel_sync_plan())
}

#[tauri::command]
fn excel_sync_action(
    state: tauri::State<'_, AppState>,
    group: String,
    node: String,
    code: u32,
) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.excel_sync_action(&group, &node, code)?;
    Ok(b.snapshot())
}

#[tauri::command]
fn excel_preview(
    state: tauri::State<'_, AppState>,
    path: String,
    sheet: Option<String>,
    header_row: Option<u32>,
) -> Result<ExcelPreview, String> {
    lock(&state)?.excel_preview(path, sheet, header_row)
}

#[tauri::command]
fn excel_import_mapped(
    state: tauri::State<'_, AppState>,
    request: ExcelMapRequest,
) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.excel_import_mapped(request)?;
    Ok(b.snapshot())
}

#[tauri::command]
fn open_folder(state: tauri::State<'_, AppState>, path: Option<String>) -> Result<(), String> {
    lock(&state)?.open_folder(path)
}

#[tauri::command]
fn open_url(state: tauri::State<'_, AppState>, url: String) -> Result<(), String> {
    lock(&state)?.open_url(&url)
}

#[tauri::command]
fn cell_issue(
    state: tauri::State<'_, AppState>,
    row: usize,
    col: usize,
) -> Result<Option<CellIssue>, String> {
    Ok(lock(&state)?.cell_issue(row, col))
}

#[tauri::command]
fn ref_picker(
    state: tauri::State<'_, AppState>,
    target: String,
    strategy: Option<String>,
) -> Result<RefPickerData, String> {
    lock(&state)?.ref_picker_data(&target, strategy.as_deref())
}

#[tauri::command]
fn type_picker_info(
    state: tauri::State<'_, AppState>,
    group: String,
    name: String,
    current: String,
) -> Result<TypePickerInfo, String> {
    lock(&state)?.type_picker_info(&group, &name, &current)
}

#[tauri::command]
fn type_preview(
    state: tauri::State<'_, AppState>,
    paradigm: usize,
    params: Vec<String>,
) -> TypePreview {
    lock(&state).map(|b| b.type_preview(paradigm, params)).unwrap_or_else(|e| {
        TypePreview {
            valid: false,
            type_string: String::new(),
            example: String::new(),
            java: String::new(),
            go: String::new(),
            lua: String::new(),
            error: Some(e),
        }
    })
}

// ── 原生文件对话框 ──────────────────────────────────────────
#[tauri::command]
fn pick_file(title: String, extensions: Vec<String>) -> Option<String> {
    let mut dialog = rfd::FileDialog::new().set_title(&title);
    for ext in &extensions {
        if !ext.is_empty() {
            dialog = dialog.add_filter(ext, &[ext.as_str()]);
        }
    }
    dialog.pick_file().map(|p| p.display().to_string())
}

#[tauri::command]
fn pick_files(title: String, extensions: Vec<String>) -> Option<Vec<String>> {
    let mut dialog = rfd::FileDialog::new().set_title(&title);
    for ext in &extensions {
        if !ext.is_empty() {
            dialog = dialog.add_filter(ext, &[ext.as_str()]);
        }
    }
    dialog
        .pick_files()
        .map(|files| files.iter().map(|p| p.display().to_string()).collect())
}

#[tauri::command]
fn pick_save(title: String, default_name: String, extensions: Vec<String>) -> Option<String> {
    let mut dialog = rfd::FileDialog::new()
        .set_title(&title)
        .set_file_name(&default_name);
    for ext in &extensions {
        if !ext.is_empty() {
            dialog = dialog.add_filter(ext, &[ext.as_str()]);
        }
    }
    dialog.save_file().map(|p| p.display().to_string())
}

#[tauri::command]
fn pick_folder(title: String) -> Option<String> {
    rfd::FileDialog::new()
        .set_title(&title)
        .pick_folder()
        .map(|p| p.display().to_string())
}

#[tauri::command]
fn set_title(window: tauri::Window, title: String) -> Result<(), String> {
    window.set_title(&title).map_err(|e| e.to_string())
}

#[tauri::command]
fn excel_begin_edit(
    state: tauri::State<'_, AppState>,
    project: String,
    group: String,
    open: Option<bool>,
) -> Result<String, String> {
    let mut b = lock(&state)?;
    b.ensure_group_unlocked(&project, &group)?;
    b.excel_begin_edit(&project, &group, open.unwrap_or(true))
}

#[tauri::command]
fn excel_poll(state: tauri::State<'_, AppState>) -> Result<Vec<ExcelPollItem>, String> {
    Ok(lock(&state)?.excel_poll())
}

#[tauri::command]
fn excel_sync_group(
    state: tauri::State<'_, AppState>,
    project: String,
    group: String,
) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.excel_sync(&project, &group)?;
    Ok(b.snapshot())
}

#[tauri::command]
fn excel_end_edit(
    state: tauri::State<'_, AppState>,
    project: String,
    group: String,
) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.excel_end_edit(&project, &group)?;
    Ok(b.snapshot())
}

#[tauri::command]
fn excel_abandon(
    state: tauri::State<'_, AppState>,
    project: String,
    group: String,
) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.excel_abandon(&project, &group);
    Ok(b.snapshot())
}

#[tauri::command]
fn open_xlsx_book(
    state: tauri::State<'_, AppState>,
    project: String,
    group: String,
) -> Result<(), String> {
    lock(&state)?.open_xlsx_book(&project, &group)
}

// ── Univer 编辑窗口 ─────────────────────────────────────────
/// 一个组一个独立窗口（类似 Excel 多开），可并排查看 / 比对多个组。
#[tauri::command]
async fn open_univer_editor(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    project: String,
    group: String,
) -> Result<(), String> {
    if env!("TABLET_UNIVER_ASSETS") != "1" {
        return Err(
            "Univer 编辑器资源未构建：cd crates/app-tauri/univer-editor && npm ci && npm run build"
                .to_string(),
        );
    }
    let label = Backend::univer_window_label(&project, &group);
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return Ok(());
    }
    {
        let mut b = lock(&state)?;
        b.prepare_univer_window(&label, &project, &group)?;
    }
    let built = tauri::WebviewWindowBuilder::new(
        &app,
        label.clone(),
        tauri::WebviewUrl::App("univer/index.html".into()),
    )
    .title(format!("Univer 编辑 — {group}"))
    .inner_size(1180.0, 760.0)
    .min_inner_size(860.0, 520.0)
    .build();
    let win = match built {
        Ok(w) => w,
        Err(e) => {
            if let Ok(mut b) = lock(&state) {
                b.release_univer_window(&label);
            }
            return Err(format!("打开 Univer 窗口失败: {e}"));
        }
    };
    // 窗口被关掉（含强制关闭）时释放这一组的锁。
    let handle = app.clone();
    let window_label = label.clone();
    win.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            let state = handle.state::<AppState>();
            if let Ok(mut b) = state.backend.lock() {
                b.release_univer_window(&window_label);
            }
            let _ = handle.emit_to("main", "tablet://state-changed", ());
        }
    });
    Ok(())
}

/// 窗口加载时凭自己的 label 取「这个窗口该打开哪个组」。
#[tauri::command]
fn univer_initial_context(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
) -> Result<Option<UniverOpenRequest>, String> {
    let label = window.label().to_string();
    Ok(lock(&state)?.univer_window_context(&label))
}

#[tauri::command]
fn univer_load_group(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    project: String,
    group: String,
) -> Result<UniverGroupPayload, String> {
    let payload = lock(&state)?.univer_load_group(&project, &group)?;
    // 组进入 Univer，主界面立刻切成只读并显示提示。
    let _ = app.emit_to("main", "tablet://state-changed", ());
    Ok(payload)
}

#[tauri::command]
fn univer_apply_group(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    project: String,
    group: String,
    data: String,
) -> Result<UniverApplyResult, String> {
    let mut b = lock(&state)?;
    let result = b.univer_apply_group(&project, &group, &data)?;
    // 主界面立刻刷新（脏标记 / 表格内容），但这里不落盘。
    let _ = app.emit_to("main", "tablet://state-changed", ());
    Ok(result)
}

#[tauri::command]
fn univer_close_group(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    project: String,
    group: String,
) -> Result<(), String> {
    lock(&state)?.univer_close_group(&project, &group);
    let _ = app.emit_to("main", "tablet://state-changed", ());
    Ok(())
}

#[tauri::command]
fn import_tbl(
    state: tauri::State<'_, AppState>,
    project: String,
    group: String,
    paths: Vec<String>,
) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.ensure_group_unlocked(&project, &group)?;
    b.import_tbl(&project, &group, paths)?;
    Ok(b.snapshot())
}

#[tauri::command]
fn import_xlsx_group(
    state: tauri::State<'_, AppState>,
    project: String,
    path: String,
) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.import_xlsx_group(&project, path)?;
    Ok(b.snapshot())
}

#[tauri::command]
fn open_project_dir(
    state: tauri::State<'_, AppState>,
    project: String,
) -> Result<(), String> {
    lock(&state)?.open_project_dir(&project)
}

#[tauri::command]
fn reload_group(
    state: tauri::State<'_, AppState>,
    project: String,
    group: String,
) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.ensure_group_unlocked(&project, &group)?;
    b.reload_group(&project, &group)?;
    Ok(b.snapshot())
}

#[tauri::command]
fn export_template(
    state: tauri::State<'_, AppState>,
    id: String,
    name: String,
) -> Result<String, String> {
    lock(&state)?.export_template(id, name)
}

#[tauri::command]
fn clear_logs(state: tauri::State<'_, AppState>) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.clear_logs();
    Ok(b.snapshot())
}

#[tauri::command]
fn save_workspace(state: tauri::State<'_, AppState>) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    if !b.save() {
        return Err("保存失败：校验未通过或文件写入失败，请查看下方日志".to_string());
    }
    Ok(b.snapshot())
}

#[tauri::command]
fn undo(state: tauri::State<'_, AppState>) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.ensure_active_project_unlocked()?;
    b.undo();
    Ok(b.snapshot())
}

#[tauri::command]
fn redo(state: tauri::State<'_, AppState>) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.ensure_active_project_unlocked()?;
    b.redo();
    Ok(b.snapshot())
}

#[tauri::command]
fn reload_workspace(state: tauri::State<'_, AppState>) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.ensure_no_univer_windows()?;
    b.reload();
    Ok(b.snapshot())
}

#[tauri::command]
fn validate_workspace(state: tauri::State<'_, AppState>) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.validate();
    Ok(b.snapshot())
}

#[tauri::command]
fn open_project(state: tauri::State<'_, AppState>, project: String) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.open_project(&project);
    Ok(b.snapshot())
}

#[tauri::command]
fn close_project(state: tauri::State<'_, AppState>, project: String) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.ensure_project_unlocked(&project)?;
    b.close_project(&project);
    Ok(b.snapshot())
}

#[tauri::command]
fn delete_project(state: tauri::State<'_, AppState>, project: String) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.ensure_project_unlocked(&project)?;
    b.delete_project(&project)?;
    Ok(b.snapshot())
}

#[tauri::command]
fn clone_project(
    state: tauri::State<'_, AppState>,
    source: String,
    new_id: String,
    new_name: String,
) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.clone_project(&source, new_id, new_name)?;
    Ok(b.snapshot())
}

#[tauri::command]
fn paste_group(
    state: tauri::State<'_, AppState>,
    project: String,
    storage: Option<String>,
) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.paste_group(&project, storage);
    Ok(b.snapshot())
}

#[tauri::command]
fn paste_group_preview(
    state: tauri::State<'_, AppState>,
    project: String,
) -> Result<Option<PasteGroupPreviewUi>, String> {
    Ok(lock(&state)?.paste_group_preview(&project))
}

#[tauri::command]
fn paste_group_named(
    state: tauri::State<'_, AppState>,
    project: String,
    storage: Option<String>,
    group_name: String,
    nodes: Vec<PasteGroupRowUi>,
) -> Result<Snapshot, String> {
    let mut b = lock(&state)?;
    b.paste_group_named(&project, storage, group_name, nodes)?;
    Ok(b.snapshot())
}

fn main() {
    shell::dispatch("tablet-tauri", run_gui);
}

fn run_gui(workdir_arg: Option<std::path::PathBuf>) -> anyhow::Result<()> {
    // 锁 / 日志 / 崩溃日志；shell 必须活到窗口结束（锁在 drop 时释放）。
    let shell = shell::Shell::init("tablet-tauri", workdir_arg)?;
    let backend = Backend::load(&shell.workdir)?;

    tauri::Builder::default()
        .manage(AppState {
            backend: Mutex::new(backend),
        })
        .invoke_handler(tauri::generate_handler![
            snapshot,
            select_node,
            commit_cell,
            commit_header,
            clear_cells,
            paste_cells,
            insert_row,
            delete_rows,
            insert_column,
            delete_column,
            new_group,
            new_node,
            rename_group,
            rename_node,
            delete_group,
            delete_node,
            copy_node,
            copy_group,
            paste_node,
            revert_node,
            list_templates,
            create_project,
            schema_preview,
            template_preview,
            create_project_empty,
            create_project_from_template,
            create_project_from_file,
            project_settings,
            save_project_settings,
            rules_payload,
            rules_save,
            set_show_ref_name,
            set_display_field,
            save_grid_view,
            xlsx_poll,
            reload_xlsx_external,
            set_project_sort,
            global_settings,
            save_global_settings,
            export_project,
            schema_export,
            schema_outline,
            schema_import_preview,
            schema_import,
            excel_export,
            excel_import,
            excel_begin_edit,
            excel_poll,
            excel_sync_group,
            excel_end_edit,
            excel_abandon,
            open_xlsx_book,
            open_univer_editor,
            univer_initial_context,
            univer_load_group,
            univer_apply_group,
            univer_close_group,
            excel_files,
            excel_delete,
            excel_sync_plan,
            excel_sync_action,
            excel_preview,
            excel_import_mapped,
            open_folder,
            open_url,
            ref_picker,
            cell_issue,
            type_picker_info,
            type_preview,
            pick_file,
            pick_files,
            pick_save,
            pick_folder,
            set_title,
            save_workspace,
            clear_logs,
            undo,
            redo,
            reload_workspace,
            validate_workspace,
            open_project,
            close_project,
            delete_project,
            clone_project,
            paste_group,
            paste_group_preview,
            paste_group_named,
            import_tbl,
            import_xlsx_group,
            open_project_dir,
            reload_group,
            export_template,
        ])
        .run(tauri::generate_context!())
        .map_err(|e| anyhow::anyhow!("Tauri 启动失败: {e}"))
}
