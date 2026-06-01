// 模板库 + 新建项目对话框（@docs/04-UI设计.md §6.6 / §6.7、@08 S15-E2）。
//
// - 模板库：浏览 BuiltinTemplates / LocalTemplates，列表选中后点击"使用此模板"
//   关此弹窗 → 弹"新建项目"对话框（mode = FromTemplate）。
// - NewProject 对话框统一 3 模式（Empty / FromTemplate / Clone）：
//   - Empty：树面板「新建项目」按钮触发，落地一个空 schema 项目
//   - FromTemplate：模板库「使用此模板」触发，按模板 schema 落地
//   - Clone：opened project 右键「复制(克隆)…」触发，内存深拷贝 + 标 dirty 待保存

use eframe::egui;
use tbl_core::template::{
    default_local_dir, BuiltinTemplates, LocalTemplates, TemplateMeta, TemplateSource,
};
use tbl_core::tblschema::is_valid_metadata_id;

use crate::app::TblApp;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemplateTab {
    Builtin,
    Local,
}

pub struct TemplateLibraryState {
    pub open: bool,
    pub tab: TemplateTab,
    pub selected_id: String,
    pub search: String,
}

impl Default for TemplateLibraryState {
    fn default() -> Self {
        Self {
            open: false,
            tab: TemplateTab::Builtin,
            selected_id: String::new(),
            search: String::new(),
        }
    }
}

/// NewProject 对话框的 3 种模式。决定标题、介绍行、id/name 默认值与落地路径。
#[derive(Clone, Debug)]
pub enum NewProjectMode {
    /// 空项目（来自树面板「新建项目」按钮）
    Empty,
    /// 从模板（来自「模板库」对话框）
    FromTemplate {
        template_id: String,
        template_source: String, // "builtin" / "local"
        template_display: String, // 顶部 "基于模板：xxx" 行
    },
    /// 克隆已有 project（来自 opened project 右键菜单）
    Clone {
        source_project_id: String,
        source_display: String,
    },
}

impl Default for NewProjectMode {
    fn default() -> Self {
        NewProjectMode::Empty
    }
}

#[derive(Default)]
pub struct NewProjectState {
    pub open: bool,
    pub mode: NewProjectMode,
    pub project_id: String,
    pub project_name: String,
    /// 「立即打开新项目」勾选项；默认 true。
    pub open_after: bool,
    /// 是否已经按 mode 默认值预填过 id（避免覆盖用户手输）
    pub id_prefilled: bool,
}

impl NewProjectState {
    pub fn open_empty(&mut self) {
        self.open = true;
        self.mode = NewProjectMode::Empty;
        self.project_id.clear();
        self.project_name.clear();
        self.open_after = true;
        self.id_prefilled = true; // 空项目不 prefill id
    }

    pub fn open_from_template(&mut self, meta: &TemplateMeta) {
        self.open = true;
        self.mode = NewProjectMode::FromTemplate {
            template_id: meta.id.clone(),
            template_source: meta.source.to_string(),
            template_display: if meta.name.is_empty() {
                meta.id.clone()
            } else {
                meta.name.clone()
            },
        };
        self.project_id.clear();
        self.project_name.clear();
        self.open_after = true;
        self.id_prefilled = false;
    }

    pub fn open_clone(&mut self, source_id: &str, source_display: &str) {
        self.open = true;
        self.mode = NewProjectMode::Clone {
            source_project_id: source_id.to_string(),
            source_display: source_display.to_string(),
        };
        self.project_id = format!("{}_copy", source_id);
        self.project_name = format!("{}_copy", source_display);
        // Clone 模式必须保存才落盘，"立即打开"无意义 → 默认 false
        self.open_after = false;
        self.id_prefilled = true;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.mode = NewProjectMode::Empty;
        self.project_id.clear();
        self.project_name.clear();
        self.id_prefilled = false;
    }
}

// ──────── 模板库弹窗 ────────

pub fn render_library_dialog(ctx: &egui::Context, app: &mut TblApp) {
    if !app.template_lib.open {
        return;
    }

    let mut open = true;
    let mut want_use = false;

    let builtin_list = BuiltinTemplates::new().list();
    let local_src = LocalTemplates::new(default_local_dir());
    let local_list = local_src.list();

    egui::Window::new("模板库")
        .collapsible(false)
        .resizable(true)
        .default_width(560.0)
        .default_height(420.0)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                let builtin_label = format!("内置 ({})", builtin_list.len());
                let local_label = format!("本地 ({})", local_list.len());
                ui.selectable_value(&mut app.template_lib.tab, TemplateTab::Builtin, builtin_label);
                ui.selectable_value(&mut app.template_lib.tab, TemplateTab::Local, local_label);
                ui.add_enabled(false, egui::Button::new("网络 (-)"))
                    .on_hover_text("S15-H 待实现");
            });
            ui.separator();

            ui.horizontal(|ui| {
                ui.label("🔍");
                ui.add(
                    egui::TextEdit::singleline(&mut app.template_lib.search)
                        .desired_width(220.0)
                        .hint_text("名称 / id 关键字"),
                );
            });

            let active = match app.template_lib.tab {
                TemplateTab::Builtin => &builtin_list,
                TemplateTab::Local => &local_list,
            };
            let q = app.template_lib.search.to_lowercase();
            let filtered: Vec<&TemplateMeta> = active
                .iter()
                .filter(|m| {
                    if q.is_empty() {
                        return true;
                    }
                    m.id.to_lowercase().contains(&q)
                        || m.name.to_lowercase().contains(&q)
                        || m.category.to_lowercase().contains(&q)
                })
                .collect();

            ui.separator();

            // 列表 + 详情拆分：列表占大头，详情面板高度固定
            egui::ScrollArea::vertical()
                .max_height(220.0)
                .show(ui, |ui| {
                    if filtered.is_empty() {
                        ui.label(egui::RichText::new("（无匹配模板）").italics().weak());
                    }
                    for m in &filtered {
                        let selected = app.template_lib.selected_id == m.id;
                        let label = render_list_label(m);
                        if ui.selectable_label(selected, label).clicked() {
                            app.template_lib.selected_id = m.id.clone();
                        }
                    }
                });

            ui.separator();

            // 详情面板
            let detail = filtered
                .iter()
                .find(|m| m.id == app.template_lib.selected_id);
            ui.label(egui::RichText::new("详情").strong());
            match detail {
                Some(m) => {
                    ui.label(format!("id: {}", m.id));
                    ui.label(format!("name: {}", m.name));
                    ui.label(format!(
                        "category: {} · version: {} · 来源: {}",
                        if m.category.is_empty() { "-" } else { m.category.as_str() },
                        if m.version.is_empty() { "-" } else { m.version.as_str() },
                        m.source
                    ));
                    if let Some(content) = load_template_content(&m.id, m.source) {
                        let mut groups: Vec<&str> =
                            content.schema.sections.iter().map(|s| s.group.as_str()).collect();
                        groups.sort();
                        groups.dedup();
                        ui.label(format!(
                            "Groups: {} · Sections: {}",
                            groups.len(),
                            content.schema.sections.len()
                        ));
                    }
                }
                None => {
                    ui.label(egui::RichText::new("（未选择）").weak());
                }
            }

            ui.separator();
            super::modal::dialog_buttons(ui, |ui| {
                if ui.button("关闭").clicked() {
                    app.template_lib.open = false;
                }
                let enabled = detail.is_some();
                if ui.add_enabled(enabled, egui::Button::new("使用此模板新建项目")).clicked() {
                    want_use = true;
                }
            });
        });

    if !open {
        app.template_lib.open = false;
    }

    if want_use {
        if let Some(meta) = pick_meta(&app.template_lib.selected_id, &builtin_list, &local_list) {
            app.template_lib.open = false;
            app.new_project.open_from_template(&meta);
        }
    }
}

fn render_list_label(m: &TemplateMeta) -> String {
    let category = if m.category.is_empty() { "-" } else { m.category.as_str() };
    let version = if m.version.is_empty() { "-" } else { m.version.as_str() };
    format!("{}  ·  {}  ·  v{}  [id: {}]", m.name, category, version, m.id)
}

fn pick_meta(id: &str, builtin: &[TemplateMeta], local: &[TemplateMeta]) -> Option<TemplateMeta> {
    builtin
        .iter()
        .chain(local.iter())
        .find(|m| m.id == id)
        .cloned()
}

fn load_template_content(
    id: &str,
    source: &str,
) -> Option<tbl_core::template::TemplateContent> {
    match source {
        "builtin" => BuiltinTemplates::new().load_by_id(id),
        "local" => LocalTemplates::new(default_local_dir()).load_by_id(id),
        _ => None,
    }
}

// ──────── 新建项目弹窗 ────────

pub fn render_new_project_dialog(ctx: &egui::Context, app: &mut TblApp) {
    if !app.new_project.open {
        return;
    }

    let mut open = true;
    let mut do_create = false;

    // 按 mode 取窗口标题 + 介绍行 + id/name 默认值
    let (title, intro_line, default_id, default_name, allow_open_after) = match &app.new_project.mode {
        NewProjectMode::Empty => (
            "新建空项目",
            String::new(),
            String::new(),
            String::new(),
            true,
        ),
        NewProjectMode::FromTemplate { template_id, template_source, template_display } => (
            "从模板新建项目",
            format!("基于模板：{} ({})", template_display, template_source),
            template_id.clone(),
            template_display.clone(),
            true,
        ),
        NewProjectMode::Clone { source_project_id, source_display } => (
            "复制项目",
            format!("源项目：{}", source_display),
            format!("{}_copy", source_project_id),
            format!("{}_copy", source_display),
            false, // 克隆模式：必须保存才落地，"立即打开"灰态
        ),
    };

    // 自动预填 id（首次进入时），方便顺手回车创建
    if !app.new_project.id_prefilled && app.new_project.project_id.is_empty() {
        app.new_project.project_id = default_id.clone();
        app.new_project.id_prefilled = true;
    }
    if app.new_project.project_name.is_empty() {
        app.new_project.project_name = default_name.clone();
    }

    // 列表中已存在的 project id（含 closed 的，避免重复）
    let mut existing_ids: Vec<String> = app.engine.available_projects.iter()
        .map(|a| a.id.clone()).collect();
    for p in tbl_core::project::list_projects(&app.engine.workdir) {
        if !existing_ids.iter().any(|id| id == &p.id) {
            existing_ids.push(p.id);
        }
    }

    let id_err = validate_project_id(&app.new_project.project_id, &existing_ids);
    let name_err = if app.new_project.project_name.trim().is_empty() {
        Some("项目名不能为空".to_string())
    } else {
        None
    };

    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .default_width(420.0)
        .open(&mut open)
        .show(ctx, |ui| {
            if !intro_line.is_empty() {
                ui.label(intro_line);
                ui.separator();
            }

            ui.horizontal(|ui| {
                ui.label("项目 ID:");
                ui.add(
                    egui::TextEdit::singleline(&mut app.new_project.project_id)
                        .desired_width(180.0)
                        .hint_text("[a-z0-9_-]{1,32}"),
                );
            });
            if let Some(ref msg) = id_err {
                ui.label(
                    egui::RichText::new(msg)
                        .color(egui::Color32::from_rgb(220, 50, 50))
                        .size(11.0),
                );
            }

            ui.horizontal(|ui| {
                ui.label("项目名:");
                ui.add(
                    egui::TextEdit::singleline(&mut app.new_project.project_name)
                        .desired_width(220.0)
                        .hint_text("可中文，仅用于显示"),
                );
            });
            if let Some(ref msg) = name_err {
                ui.label(
                    egui::RichText::new(msg)
                        .color(egui::Color32::from_rgb(220, 50, 50))
                        .size(11.0),
                );
            }

            ui.separator();
            ui.label(format!("目录预览: projects/{}/", app.new_project.project_id));
            ui.add_enabled_ui(allow_open_after, |ui| {
                ui.checkbox(&mut app.new_project.open_after, "立即打开新项目");
            });
            if !allow_open_after {
                ui.label(
                    egui::RichText::new("（克隆项目需保存后才能打开）")
                        .weak()
                        .size(11.0),
                );
            }

            ui.separator();
            super::modal::dialog_buttons(ui, |ui| {
                if ui.button("取消").clicked() {
                    app.new_project.close();
                }
                let can = id_err.is_none() && name_err.is_none();
                if ui.add_enabled(can, egui::Button::new("创建")).clicked() {
                    do_create = true;
                }
            });
        });

    if !open {
        app.new_project.close();
    }

    if do_create {
        do_create_project(app);
    }
}

fn validate_project_id(id: &str, existing: &[String]) -> Option<String> {
    if id.is_empty() {
        return Some("Project ID 不能为空".to_string());
    }
    if !is_valid_metadata_id(id) {
        return Some("ID 仅允许小写字母 / 数字 / _ / -，长度 1..=32".to_string());
    }
    if existing.iter().any(|e| e == id) {
        return Some(format!("ID 已存在: {}", id));
    }
    None
}

fn do_create_project(app: &mut TblApp) {
    use tbl_core::model::ProjectConfig;
    use tbl_core::project::upsert_project_config_section;
    use tbl_core::tblschema::TblSchema;

    let mode = app.new_project.mode.clone();
    let project_id = app.new_project.project_id.clone();
    let display_name = app.new_project.project_name.clone();
    let open_after = app.new_project.open_after;
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let result: Result<String, String> = match &mode {
        NewProjectMode::Empty => {
            let mut schema = TblSchema::default();
            schema.meta.id = project_id.clone();
            schema.meta.name = display_name.clone();
            schema.meta.created_at = now.clone();
            app.engine.create_project_from_schema(schema)
        }
        NewProjectMode::FromTemplate { template_id, template_source, .. } => {
            let content = match load_template_content(template_id, template_source) {
                Some(c) => c,
                None => {
                    app.log(format!("[新建项目] 模板未找到: {}", template_id));
                    return;
                }
            };
            let mut schema = content.schema;
            schema.meta.id = project_id.clone();
            schema.meta.name = display_name.clone();
            schema.meta.created_at = now.clone();
            schema.meta.source_template = content.meta.id.clone();
            schema.meta.source_template_version = content.meta.version.clone();
            app.engine.create_project_from_schema(schema)
        }
        NewProjectMode::Clone { source_project_id, .. } => {
            app.engine
                .clone_project_in_memory(source_project_id, &project_id, &display_name)
                .ok_or_else(|| format!("克隆失败: 源项目未打开 {}", source_project_id))
        }
    };

    let new_id = match result {
        Ok(id) => id,
        Err(e) => {
            app.log(format!("[新建项目] 失败: {}", e));
            return;
        }
    };

    let is_clone = matches!(mode, NewProjectMode::Clone { .. });

    if is_clone {
        // 克隆：内存中创建，set_active 让它在 UI 立刻可见，未保存不写 last_project
        app.engine.set_active_by_id(&new_id);
        app.log(format!(
            "[新建项目] 已克隆 {} （内存中，需保存才落地）",
            new_id
        ));
        app.new_project.close();
        return;
    }

    if open_after {
        let workdir = app.engine.workdir.clone();
        let config_path = workdir.join(tbl_core::CONFIG_FILE);
        let original = std::fs::read_to_string(&config_path).unwrap_or_default();
        let fallback_cfg;
        let cur = if let Some(active) = app.engine.active() {
            &active.config.project
        } else if let Some(p) = app.engine.projects.first() {
            &p.config.project
        } else {
            fallback_cfg = ProjectConfig {
                last_project: String::new(),
                opened_projects: Vec::new(),
                project_sort: String::new(),
                project_order: Vec::new(),
                config_dir: "config".to_string(),
                cache_dir: ".tbl-cache".to_string(),
            };
            &fallback_cfg
        };
        let new_project_cfg = ProjectConfig {
            last_project: new_id.clone(),
            opened_projects: cur.opened_projects.clone(),
            project_sort: cur.project_sort.clone(),
            project_order: cur.project_order.clone(),
            config_dir: cur.config_dir.clone(),
            cache_dir: cur.cache_dir.clone(),
        };
        let updated = upsert_project_config_section(&original, &new_project_cfg);
        if let Err(e) = std::fs::write(&config_path, updated) {
            app.log(format!("[新建项目] 写入 tbl-tool.toml 失败: {}", e));
        }
    }

    app.log(format!(
        "[新建项目] 已创建 {} ({})",
        new_id,
        if open_after { "打开中..." } else { "未打开" }
    ));

    app.new_project.close();

    if open_after {
        app.reload();
    }
}
