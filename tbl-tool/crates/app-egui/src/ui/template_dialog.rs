// 模板库 + 新建项目对话框（@docs/04-UI设计.md §6.6 / §6.7、@08 S15-E2）。
//
// - 模板库：浏览 BuiltinTemplates / LocalTemplates，列表选中后点击"使用此模板"
//   关此弹窗 → 弹"新建项目"。
// - 新建项目：复用 CLI run_new_project 的 5 步落地逻辑（实例化 + 写 meta + 可选切换）。

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

#[derive(Default)]
pub struct NewProjectState {
    pub open: bool,
    pub template_id: String,
    pub template_source: String, // "builtin" / "local"，仅展示
    pub template_display: String, // 顶部 "基于模板：xxx" 行
    pub project_id: String,
    pub project_name: String,
    pub switch_after: bool,
    /// 是否已经按 template 自动填过 id 默认值（避免覆盖用户手输）
    pub id_prefilled: bool,
}

impl NewProjectState {
    pub fn open_with(&mut self, meta: &TemplateMeta) {
        self.open = true;
        self.template_id = meta.id.clone();
        self.template_source = meta.source.to_string();
        self.template_display = if meta.name.is_empty() {
            meta.id.clone()
        } else {
            meta.name.clone()
        };
        self.project_id.clear();
        self.project_name.clear();
        self.switch_after = true;
        self.id_prefilled = false;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.template_id.clear();
        self.template_source.clear();
        self.template_display.clear();
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
            app.new_project.open_with(&meta);
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

    // 自动用 template id 预填 project id（首次进入时），方便顺手回车创建
    if !app.new_project.id_prefilled && app.new_project.project_id.is_empty() {
        app.new_project.project_id = app.new_project.template_id.clone();
        app.new_project.id_prefilled = true;
    }
    if app.new_project.project_name.is_empty() {
        app.new_project.project_name = app.new_project.template_display.clone();
    }

    let existing_ids: Vec<String> = tbl_core::project::list_projects(&app.engine.workdir)
        .into_iter()
        .map(|p| p.id)
        .collect();

    let id_err = validate_project_id(&app.new_project.project_id, &existing_ids);
    let name_err = if app.new_project.project_name.trim().is_empty() {
        Some("项目名不能为空".to_string())
    } else {
        None
    };

    egui::Window::new("新建项目")
        .collapsible(false)
        .resizable(false)
        .default_width(420.0)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label(format!(
                "基于模板：{}  ({})",
                app.new_project.template_display, app.new_project.template_source
            ));
            ui.separator();

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
            ui.checkbox(&mut app.new_project.switch_after, "立即切换到新项目");

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
    use tbl_core::model::{ProjectConfig, ProjectInstanceMeta};
    use tbl_core::project::{
        upsert_project_config_section, write_project_meta, PROJECTS_DIR,
    };
    use tbl_core::template::instantiate_template;

    let workdir = app.engine.workdir.clone();
    let project_id = app.new_project.project_id.clone();
    let display_name = app.new_project.project_name.clone();
    let template_id = app.new_project.template_id.clone();
    let template_source = app.new_project.template_source.clone();
    let switch_after = app.new_project.switch_after;

    let content = match template_source.as_str() {
        "local" => LocalTemplates::new(default_local_dir()).load_by_id(&template_id),
        _ => BuiltinTemplates::new().load_by_id(&template_id),
    };
    let content = match content {
        Some(c) => c,
        None => {
            app.log(format!("[新建项目] 模板未找到: {}", template_id));
            return;
        }
    };

    let projects_dir = workdir.join(PROJECTS_DIR);
    if let Err(e) = std::fs::create_dir_all(&projects_dir) {
        app.log(format!("[新建项目] 创建 projects/ 失败: {}", e));
        return;
    }
    let project_root = projects_dir.join(&project_id);
    if project_root.exists() {
        app.log(format!("[新建项目] 目录已存在: {}", project_root.display()));
        return;
    }

    if let Err(e) = instantiate_template(&content.schema, &project_root) {
        app.log(format!("[新建项目] 实例化模板失败: {}", e));
        let _ = std::fs::remove_dir_all(&project_root);
        return;
    }

    let meta = ProjectInstanceMeta {
        id: project_id.clone(),
        name: display_name.clone(),
        created_at: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        source_template: content.meta.id.clone(),
        source_template_version: content.meta.version.clone(),
    };
    if let Err(e) = write_project_meta(&project_root, &meta) {
        app.log(format!("[新建项目] 写入 project.toml 失败: {}", e));
        return;
    }

    if switch_after {
        let config_path = workdir.join(tbl_core::CONFIG_FILE);
        let original = std::fs::read_to_string(&config_path).unwrap_or_default();
        // active 缺失时回落 projects[0]，再回落硬编码默认（让"全关时新建项目"也能写出 toml）
        let fallback_cfg;
        let cur = if let Some(active) = app.engine.active() {
            &active.config.project
        } else if let Some(p) = app.engine.projects.first() {
            &p.config.project
        } else {
            fallback_cfg = ProjectConfig {
                name: "my-game".to_string(),
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
            name: cur.name.clone(),
            last_project: project_id.clone(),
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
        project_root.display(),
        if switch_after { "切换中..." } else { "未切换" }
    ));

    app.new_project.close();

    if switch_after {
        app.reload();
    }
}
