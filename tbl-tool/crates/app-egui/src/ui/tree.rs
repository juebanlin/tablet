use eframe::egui;
use std::collections::HashSet;
use crate::app::{TblApp, SelectedNode, PendingAction, RenameProjectStage, TreeFilter, TreeContext};
use tbl_core::ops::{AvailableProject, NodeClipboard, NodeKind};
use tbl_core::name_matches;

/// TreeSection 大区渲染：4 段子区域（标题 / 顶部功能区 / 搜索过滤区 / 节点列表）。
/// 三层结构：project（indent=0）/ group（indent=1）/ node（indent=2）。
/// 对应 docs/04-UI设计.md §2。
pub fn render(ui: &mut egui::Ui, app: &mut TblApp) {
    // ─── 1. 标题栏 ───
    ui.heading("配置树");
    ui.separator();

    // ─── 2. 顶部功能区：模板库 + 项目排序 ───
    ui.horizontal(|ui| {
        if ui.button("模板库").clicked() {
            app.template_lib.open = true;
        }
        ui.label("排序:");
        let label = match app.project_sort.as_str() {
            "name" => "名称",
            "open" => "已打开",
            "created" => "创建时间",
            "manual" => "手动",
            _ => "ID",
        };
        let mut new_sort: Option<&str> = None;
        egui::ComboBox::from_id_source("project_sort")
            .selected_text(label)
            .show_ui(ui, |ui| {
                if ui.selectable_label(app.project_sort == "id", "ID").clicked() { new_sort = Some("id"); }
                if ui.selectable_label(app.project_sort == "name", "名称").clicked() { new_sort = Some("name"); }
                if ui.selectable_label(app.project_sort == "open", "已打开").clicked() { new_sort = Some("open"); }
                if ui.selectable_label(app.project_sort == "created", "创建时间").clicked() { new_sort = Some("created"); }
                if ui.selectable_label(app.project_sort == "manual", "手动").clicked() { new_sort = Some("manual"); }
            });
        if let Some(s) = new_sort {
            if app.project_sort != s {
                app.project_sort = s.to_string();
                persist_workspace(app);
            }
        }
    });
    ui.separator();

    // ─── 3. 功能区：搜索 / 过滤 / 完整组 ───
    ui.horizontal(|ui| {
        ui.label("搜索:");
        ui.add(egui::TextEdit::singleline(&mut app.tree_search)
            .desired_width(ui.available_width().min(140.0))
            .hint_text("名称 / 拼音首字母"));
    });
    ui.horizontal(|ui| {
        ui.label("过滤:");
        let label = match app.tree_filter {
            TreeFilter::All => "全部",
            TreeFilter::New => "新增",
            TreeFilter::Modified => "修改",
            TreeFilter::Deleted => "删除",
            TreeFilter::Changed => "改动",
        };
        egui::ComboBox::from_id_source("tree_filter")
            .selected_text(label)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut app.tree_filter, TreeFilter::All, "全部");
                ui.selectable_value(&mut app.tree_filter, TreeFilter::Changed, "改动");
                ui.separator();
                ui.selectable_value(&mut app.tree_filter, TreeFilter::New, "新增");
                ui.selectable_value(&mut app.tree_filter, TreeFilter::Modified, "修改");
                ui.selectable_value(&mut app.tree_filter, TreeFilter::Deleted, "删除");
            });
    });
    ui.checkbox(&mut app.tree_filter_show_full_group, "完整组");
    ui.separator();

    // ─── 4. 节点列表 ───
    egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
        // 按当前 sort 把 available_projects 排序后列出（含 closed）。
        let opened_set: HashSet<String> = app.engine.opened_ids().into_iter().collect();
        let sorted = sorted_available(
            app.engine.available(),
            &opened_set,
            &app.project_sort,
            &app.project_order,
        );
        // 已打开 project 的 (id, name, groups) snapshot；用于三层渲染（避免 borrow 冲突）
        let opened_snap: std::collections::HashMap<String, (String, Vec<tbl_core::model::Group>)> =
            app.engine.projects.iter()
                .map(|p| (p.instance_meta.id.clone(),
                          (p.instance_meta.name.clone(), p.groups.clone())))
                .collect();
        let filter = app.tree_filter.clone();
        let full_group = app.tree_filter_show_full_group;
        let search = app.tree_search.clone();

        for ap in &sorted {
            let pid = &ap.id;
            let is_open = opened_set.contains(pid);
            // ── closed project：灰态展示，不参与搜索过滤，永远可见 ──
            if !is_open {
                let row = ui.horizontal(|ui| {
                    ui.add_space(14.0); // 占位三角宽度（closed 不显示三角，靠双击/右键打开）
                    let label_text = format!("📦 {}", ap.name);
                    let selected = matches!(&app.selected, Some(SelectedNode::Project { project_id }) if project_id == pid);
                    let r = ui.selectable_label(
                        selected,
                        egui::RichText::new(&label_text).weak(),
                    );
                    r
                });
                let resp = row.inner;
                let rect = row.response.rect;
                if resp.clicked() {
                    // closed project 单击仅选中，不打开（与统一规则一致）
                    app.selected = Some(SelectedNode::Project { project_id: pid.clone() });
                }
                if resp.double_clicked() {
                    let pid_owned = pid.clone();
                    if open_project_with_persist(app, &pid_owned) {
                        app.engine.set_active_by_id(&pid_owned);
                        app.selected = Some(SelectedNode::Project { project_id: pid_owned.clone() });
                        app.project_expanded.insert(pid_owned.clone());
                        if let Some(p) = app.engine.find_project(&pid_owned) {
                            for g in &p.groups {
                                app.tree_expanded.insert((pid_owned.clone(), g.name.clone()));
                            }
                        }
                    }
                }
                if check_secondary_click(ui, rect) {
                    app.tree_context = Some(TreeContext::Project(pid.clone()));
                    app.context_pos = ui.input(|i| i.pointer.interact_pos().unwrap_or_default());
                }
                continue;
            }

            // ── opened project：从 snapshot 拿到 name + groups，三层渲染 ──
            let (pname, groups) = match opened_snap.get(pid) {
                Some(v) => v.clone(),
                None => continue,
            };
            // 计算每个 group 是否要显示
            let mut group_views: Vec<GroupView> = Vec::with_capacity(groups.len());
            for group in &groups {
                let table_hits: Vec<bool> = group.tables.iter().map(|t| {
                    passes_filter(&filter, t.deleted, t.original.is_empty(), t.dirty)
                        && name_matches(&t.name, &search)
                }).collect();
                let const_hits: Vec<bool> = group.constants.iter().map(|c| {
                    passes_filter(&filter, c.deleted, c.original.is_empty(), c.dirty)
                        && name_matches(&c.name, &search)
                }).collect();
                let enum_hits: Vec<bool> = group.enums.iter().map(|e| {
                    passes_filter(&filter, e.deleted, e.original.is_empty(), e.dirty)
                        && name_matches(&e.name, &search)
                }).collect();
                let any_child_hit = table_hits.iter().any(|b| *b)
                    || const_hits.iter().any(|b| *b)
                    || enum_hits.iter().any(|b| *b);
                let group_filter_pass = filter == TreeFilter::All
                    || group.tables.iter().any(|t| passes_filter(&filter, t.deleted, t.original.is_empty(), t.dirty))
                    || group.constants.iter().any(|c| passes_filter(&filter, c.deleted, c.original.is_empty(), c.dirty))
                    || group.enums.iter().any(|e| passes_filter(&filter, e.deleted, e.original.is_empty(), e.dirty));
                let group_self_hit = group_filter_pass && name_matches(&group.name, &search);
                let show = group_self_hit || any_child_hit;
                group_views.push(GroupView { table_hits, const_hits, enum_hits, show });
            }
            let project_self_hit = name_matches(&pname, &search) || name_matches(pid, &search);
            let project_show = project_self_hit || group_views.iter().any(|g| g.show);
            if !project_show { continue; }

            // 项目根节点状态聚合：跨所有 group/table/constant/enum
            let mut all_deleted_self = false;
            let mut all_deleted = true;
            let mut has_dirty = false;
            let mut has_new = false;
            for g in &groups {
                if !g.tables.is_empty() || !g.constants.is_empty() || !g.enums.is_empty() {
                    all_deleted_self = true;
                }
                if g.is_new { has_new = true; }
                for t in &g.tables {
                    if !t.deleted { all_deleted = false; }
                    if t.dirty && !t.deleted { has_dirty = true; }
                    if t.original.is_empty() && !t.deleted { has_new = true; }
                }
                for c in &g.constants {
                    if !c.deleted { all_deleted = false; }
                    if c.dirty && !c.deleted { has_dirty = true; }
                    if c.original.is_empty() && !c.deleted { has_new = true; }
                }
                for e in &g.enums {
                    if !e.deleted { all_deleted = false; }
                    if e.dirty && !e.deleted { has_dirty = true; }
                    if e.original.is_empty() && !e.deleted { has_new = true; }
                }
            }
            let project_deleted = all_deleted_self && all_deleted;
            let project_dirty = has_dirty && !project_deleted;

            let p_expanded = app.project_expanded.contains(pid);
            let arrow = if p_expanded { "▼" } else { "▶" };
            let is_active = app.engine.active_project_id() == Some(pid.as_str());

            let row = ui.horizontal(|ui| {
                if ui.small_button(arrow).clicked() {
                    toggle_project_expanded(app, pid);
                }
                let label_text = format!("📦 {}{}", pname, if is_active { " ★" } else { "" });
                let selected = matches!(&app.selected, Some(SelectedNode::Project { project_id }) if project_id == pid);
                let r = ui.selectable_label(selected, &label_text);
                render_marker(ui, project_deleted, has_new && !project_dirty && !project_deleted, project_dirty);
                if app.engine.has_project_error(pid) {
                    ui.label(egui::RichText::new("!").color(egui::Color32::from_rgb(220, 50, 50)).strong());
                }
                r
            });
            let project_resp = row.inner;
            let project_rect = row.response.rect;
            if project_resp.clicked() {
                app.engine.set_active_by_id(pid);
                app.selected = Some(SelectedNode::Project { project_id: pid.clone() });
            }
            if project_resp.double_clicked() {
                toggle_project_expanded(app, pid);
            }
            if check_secondary_click(ui, project_rect) {
                app.tree_context = Some(TreeContext::Project(pid.clone()));
                app.context_pos = ui.input(|i| i.pointer.interact_pos().unwrap_or_default());
            }

            if !p_expanded { continue; }

            // ── group → node 两层 ──
            for (gi, group) in groups.iter().enumerate() {
                let view = &group_views[gi];
                if !view.show { continue; }

                let expanded = app.tree_expanded.contains(&(pid.clone(), group.name.clone()));
                let arrow = if expanded { "▼" } else { "▶" };

                let all_deleted_self = !group.tables.is_empty() || !group.constants.is_empty() || !group.enums.is_empty();
                let all_deleted = all_deleted_self
                    && group.tables.iter().all(|t| t.deleted)
                    && group.constants.iter().all(|c| c.deleted)
                    && group.enums.iter().all(|e| e.deleted);
                let has_dirty = group.tables.iter().any(|t| t.dirty && !t.deleted)
                    || group.constants.iter().any(|c| c.dirty && !c.deleted)
                    || group.enums.iter().any(|e| e.dirty && !e.deleted);
                let group_deleted = all_deleted && !group.is_new;
                let group_is_new = group.is_new;
                let group_dirty = has_dirty && !group_is_new && !group_deleted;

                let row = ui.horizontal(|ui| {
                    ui.add_space(14.0);
                    if ui.small_button(arrow).clicked() {
                        toggle_group_expanded(app, pid, &group.name);
                    }
                    let group_label = format!("📁 {}", group.name);
                    let g_selected = matches!(&app.selected, Some(SelectedNode::Group { project_id, group: g })
                        if project_id == pid && g == &group.name);
                    let r = ui.selectable_label(g_selected, &group_label);
                    render_marker(ui, group_deleted, group_is_new, group_dirty);
                    if app.engine.has_group_error(pid, &group.name) {
                        ui.label(egui::RichText::new("!").color(egui::Color32::from_rgb(220, 50, 50)).strong());
                    }
                    r
                });
                let label_resp = row.inner;
                let row_rect = row.response.rect;
                if label_resp.clicked() {
                    app.engine.set_active_by_id(pid);
                    app.selected = Some(SelectedNode::Group {
                        project_id: pid.clone(), group: group.name.clone(),
                    });
                }
                if label_resp.double_clicked() {
                    toggle_group_expanded(app, pid, &group.name);
                }
                if check_secondary_click(ui, row_rect) {
                    app.tree_context = Some(TreeContext::Group { project_id: pid.clone(), name: group.name.clone() });
                    app.context_pos = ui.input(|i| i.pointer.interact_pos().unwrap_or_default());
                }

                if !expanded { continue; }

                for (idx, table) in group.tables.iter().enumerate() {
                    let show = if full_group { !table.deleted } else { view.table_hits[idx] };
                    if !show { continue; }
                    let selected = matches!(&app.selected, Some(SelectedNode::Table { project_id, group: g, name: n })
                        if project_id == pid && g == &group.name && n == &table.name);
                    let row = ui.horizontal(|ui| {
                        ui.add_space(36.0);
                        let r = ui.selectable_label(selected, format!("📊 {}", table.name));
                        render_marker(ui, table.deleted, table.original.is_empty(), table.dirty);
                        if app.engine.has_node_error(pid, &group.name, &table.name) {
                            ui.label(egui::RichText::new("!").color(egui::Color32::from_rgb(220, 50, 50)).strong());
                        }
                        r
                    });
                    if row.inner.clicked() && !table.deleted {
                        app.engine.set_active_by_id(pid);
                        app.selected = Some(SelectedNode::Table {
                            project_id: pid.clone(), group: group.name.clone(), name: table.name.clone(),
                        });
                    }
                    if check_secondary_click(ui, row.response.rect) {
                        app.tree_context = Some(TreeContext::Node {
                            project_id: pid.clone(), group: group.name.clone(),
                            name: table.name.clone(), kind: NodeKind::Table,
                        });
                        app.context_pos = ui.input(|i| i.pointer.interact_pos().unwrap_or_default());
                    }
                }
                for (idx, constant) in group.constants.iter().enumerate() {
                    let show = if full_group { !constant.deleted } else { view.const_hits[idx] };
                    if !show { continue; }
                    let selected = matches!(&app.selected, Some(SelectedNode::Constant { project_id, group: g, name: n })
                        if project_id == pid && g == &group.name && n == &constant.name);
                    let row = ui.horizontal(|ui| {
                        ui.add_space(36.0);
                        let r = ui.selectable_label(selected, format!("📋 {}", constant.name));
                        render_marker(ui, constant.deleted, constant.original.is_empty(), constant.dirty);
                        if app.engine.has_node_error(pid, &group.name, &constant.name) {
                            ui.label(egui::RichText::new("!").color(egui::Color32::from_rgb(220, 50, 50)).strong());
                        }
                        r
                    });
                    if row.inner.clicked() && !constant.deleted {
                        app.engine.set_active_by_id(pid);
                        app.selected = Some(SelectedNode::Constant {
                            project_id: pid.clone(), group: group.name.clone(), name: constant.name.clone(),
                        });
                    }
                    if check_secondary_click(ui, row.response.rect) {
                        app.tree_context = Some(TreeContext::Node {
                            project_id: pid.clone(), group: group.name.clone(),
                            name: constant.name.clone(), kind: NodeKind::Constant,
                        });
                        app.context_pos = ui.input(|i| i.pointer.interact_pos().unwrap_or_default());
                    }
                }
                for (idx, enum_def) in group.enums.iter().enumerate() {
                    let show = if full_group { !enum_def.deleted } else { view.enum_hits[idx] };
                    if !show { continue; }
                    let selected = matches!(&app.selected, Some(SelectedNode::Enum { project_id, group: g, name: n })
                        if project_id == pid && g == &group.name && n == &enum_def.name);
                    let row = ui.horizontal(|ui| {
                        ui.add_space(36.0);
                        let r = ui.selectable_label(selected, format!("🔢 {}", enum_def.name));
                        render_marker(ui, enum_def.deleted, enum_def.original.is_empty(), enum_def.dirty);
                        if app.engine.has_node_error(pid, &group.name, &enum_def.name) {
                            ui.label(egui::RichText::new("!").color(egui::Color32::from_rgb(220, 50, 50)).strong());
                        }
                        r
                    });
                    if row.inner.clicked() && !enum_def.deleted {
                        app.engine.set_active_by_id(pid);
                        app.selected = Some(SelectedNode::Enum {
                            project_id: pid.clone(), group: group.name.clone(), name: enum_def.name.clone(),
                        });
                    }
                    if check_secondary_click(ui, row.response.rect) {
                        app.tree_context = Some(TreeContext::Node {
                            project_id: pid.clone(), group: group.name.clone(),
                            name: enum_def.name.clone(), kind: NodeKind::Enum,
                        });
                        app.context_pos = ui.input(|i| i.pointer.interact_pos().unwrap_or_default());
                    }
                }
            }
        }

        let remaining = ui.available_rect_before_wrap();
        if remaining.height() > 0.0 {
            ui.allocate_rect(remaining, egui::Sense::hover());
        }
    });

    if app.tree_context.is_none() && ui.input(|i| i.pointer.secondary_clicked()) {
        if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
            if ui.max_rect().contains(pos) {
                app.tree_context = Some(TreeContext::Blank);
                app.context_pos = pos;
            }
        }
    }

    render_tree_context(ui, app);
}

struct GroupView {
    table_hits: Vec<bool>,
    const_hits: Vec<bool>,
    enum_hits: Vec<bool>,
    show: bool,
}

fn check_secondary_click(ui: &egui::Ui, rect: egui::Rect) -> bool {
    ui.input(|i| {
        i.pointer.secondary_clicked()
            && i.pointer.interact_pos().map_or(false, |p| rect.contains(p))
    })
}

fn toggle_project_expanded(app: &mut TblApp, pid: &str) {
    if !app.project_expanded.remove(pid) {
        app.project_expanded.insert(pid.to_string());
    }
}

fn toggle_group_expanded(app: &mut TblApp, pid: &str, name: &str) {
    let key = (pid.to_string(), name.to_string());
    if !app.tree_expanded.remove(&key) {
        app.tree_expanded.insert(key);
    }
}

fn render_tree_context(ui: &mut egui::Ui, app: &mut TblApp) {
    let ctx = match &app.tree_context { Some(c) => c.clone(), None => return };

    egui::Area::new(egui::Id::new("tree_ctx_menu"))
        .fixed_pos(app.context_pos)
        .order(egui::Order::Foreground)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(120.0);
                match &ctx {
                    TreeContext::Blank => {
                        if ui.button("新建 Group").clicked() {
                            // 落点：active project；全关时不响应
                            if let Some(pid) = app.engine.active_project_id().map(str::to_string) {
                                app.pending_action = Some(PendingAction::NewGroup { project_id: pid });
                            }
                            app.tree_context = None;
                        }
                    }
                    TreeContext::Project(pid) => {
                        let is_opened = app.engine.is_opened(pid);
                        if !is_opened {
                            // closed project：仅显示 打开 / 在文件管理器打开 / 重命名 / 删除
                            if ui.button("打开 Project").clicked() {
                                let pid_owned = pid.clone();
                                if open_project_with_persist(app, &pid_owned) {
                                    app.engine.set_active_by_id(&pid_owned);
                                    app.selected = Some(SelectedNode::Project { project_id: pid_owned.clone() });
                                    app.project_expanded.insert(pid_owned.clone());
                                    if let Some(p) = app.engine.find_project(&pid_owned) {
                                        for g in &p.groups {
                                            app.tree_expanded.insert((pid_owned.clone(), g.name.clone()));
                                        }
                                    }
                                }
                                app.tree_context = None;
                            }
                            ui.separator();
                            if ui.button("在文件管理器打开").clicked() {
                                if let Some(ap) = app.engine.available().iter().find(|a| &a.id == pid) {
                                    let _ = open::that(&ap.root);
                                }
                                app.tree_context = None;
                            }
                            if ui.button("重命名 Project...").clicked() {
                                // 重命名 closed project：先打开再重命名（rename 流程操作的是 opened project 的 root + 文件）
                                let pid_owned = pid.clone();
                                if open_project_with_persist(app, &pid_owned) {
                                    app.engine.set_active_by_id(&pid_owned);
                                    app.project_expanded.insert(pid_owned.clone());
                                }
                                app.pending_action = Some(PendingAction::RenameProject {
                                    old_id: pid_owned.clone(),
                                    stage: RenameProjectStage::EnterId,
                                });
                                app.input_name = pid_owned;
                                app.tree_context = None;
                            }
                            if ui.button("删除 Project...").clicked() {
                                app.pending_action = Some(PendingAction::DeleteProject { project_id: pid.clone() });
                                app.tree_context = None;
                            }
                        } else {
                            if ui.button("保存此 Project").clicked() {
                                app.engine.save_project(pid);
                                app.tree_context = None;
                            }
                            if ui.button("导出此 Project（JSON）").clicked() {
                                let _ = app.engine.export_project(pid, tbl_core::export::export_all_json, "JSON");
                                app.tree_context = None;
                            }
                            if ui.button("导出此 Project（XML）").clicked() {
                                let _ = app.engine.export_project(pid, tbl_core::export::export_all_xml, "XML");
                                app.tree_context = None;
                            }
                            ui.separator();
                            if ui.button("新建 Group").clicked() {
                                app.pending_action = Some(PendingAction::NewGroup { project_id: pid.clone() });
                                app.tree_context = None;
                            }
                            {
                                let cb = app.engine.node_clipboard.as_ref();
                                let cb_label = cb.map(NodeClipboard::label).unwrap_or_default();
                                let has_group_cb = cb.map_or(false, NodeClipboard::is_group);
                                let paste_group_label = if has_group_cb {
                                    format!("粘贴 Group（{}）", cb_label)
                                } else {
                                    "粘贴 Group".to_string()
                                };
                                if ui.add_enabled(has_group_cb, egui::Button::new(paste_group_label)).clicked() {
                                    let pid_owned = pid.clone();
                                    if let Some(new_group) = app.engine.paste_group_to(&pid_owned) {
                                        app.project_expanded.insert(pid_owned.clone());
                                        app.tree_expanded.insert((pid_owned, new_group));
                                    }
                                    app.tree_context = None;
                                }
                            }
                            if ui.button("重命名 Project...").clicked() {
                                app.pending_action = Some(PendingAction::RenameProject {
                                    old_id: pid.clone(),
                                    stage: RenameProjectStage::EnterId,
                                });
                                app.input_name = pid.clone();
                                app.tree_context = None;
                            }
                            if ui.button("删除 Project...").clicked() {
                                app.pending_action = Some(PendingAction::DeleteProject { project_id: pid.clone() });
                                app.tree_context = None;
                            }
                            ui.separator();
                            if ui.button("关闭 Project").clicked() {
                                let pid_owned = pid.clone();
                                if app.engine.is_project_dirty(&pid_owned) {
                                    // 有未保存改动 → 走 ConfirmDialog
                                    app.pending_action = Some(PendingAction::CloseDirtyProject {
                                        project_id: pid_owned,
                                    });
                                } else {
                                    close_project_with_persist(app, &pid_owned);
                                }
                                app.tree_context = None;
                            }
                            ui.separator();
                            if ui.button("在文件管理器打开").clicked() {
                                if let Some(p) = app.engine.find_project(pid) {
                                    let _ = open::that(&p.project_root);
                                }
                                app.tree_context = None;
                            }
                        }
                    }
                    TreeContext::Group { project_id, name } => {
                        if ui.button("新建 Table").clicked() {
                            app.pending_action = Some(PendingAction::NewTable {
                                project_id: project_id.clone(), group: name.clone(),
                            });
                            app.tree_context = None;
                        }
                        if ui.button("新建 Constant").clicked() {
                            app.pending_action = Some(PendingAction::NewConstant {
                                project_id: project_id.clone(), group: name.clone(),
                            });
                            app.tree_context = None;
                        }
                        if ui.button("新建 Enum").clicked() {
                            app.pending_action = Some(PendingAction::NewEnum {
                                project_id: project_id.clone(), group: name.clone(),
                            });
                            app.tree_context = None;
                        }
                        ui.separator();
                        let cb = app.engine.node_clipboard.as_ref();
                        let cb_label = cb.map(NodeClipboard::label).unwrap_or_default();
                        let has_node_cb = cb.map_or(false, NodeClipboard::is_node);
                        let has_group_cb = cb.map_or(false, NodeClipboard::is_group);
                        if ui.button("复制 Group（含全部内容）").clicked() {
                            app.engine.clipboard_copy_group(project_id, name);
                            app.tree_context = None;
                        }
                        let paste_node_label = if has_node_cb {
                            format!("粘贴节点（{}）", cb_label)
                        } else {
                            "粘贴节点".to_string()
                        };
                        if ui.add_enabled(has_node_cb, egui::Button::new(paste_node_label)).clicked() {
                            let pid_owned = project_id.clone();
                            let group_owned = name.clone();
                            app.engine.paste_node_to(&pid_owned, &group_owned);
                            app.tree_expanded.insert((pid_owned, group_owned));
                            app.tree_context = None;
                        }
                        let paste_group_label = if has_group_cb {
                            format!("粘贴 Group（{}）", cb_label)
                        } else {
                            "粘贴 Group".to_string()
                        };
                        if ui.add_enabled(has_group_cb, egui::Button::new(paste_group_label)).clicked() {
                            let pid_owned = project_id.clone();
                            if let Some(new_group) = app.engine.paste_group_to(&pid_owned) {
                                app.project_expanded.insert(pid_owned.clone());
                                app.tree_expanded.insert((pid_owned, new_group));
                            }
                            app.tree_context = None;
                        }
                        ui.separator();
                        if ui.button("重命名").clicked() {
                            app.pending_action = Some(PendingAction::RenameGroup {
                                project_id: project_id.clone(), old_name: name.clone(),
                            });
                            app.input_name = name.clone();
                            app.tree_context = None;
                        }
                        if ui.button("删除").clicked() {
                            app.pending_action = Some(PendingAction::DeleteGroup {
                                project_id: project_id.clone(), group: name.clone(),
                            });
                            app.tree_context = None;
                        }
                    }
                    TreeContext::Node { project_id, group, name, kind } => {
                        let cb = app.engine.node_clipboard.as_ref();
                        let cb_label = cb.map(NodeClipboard::label).unwrap_or_default();
                        let has_node_cb = cb.map_or(false, NodeClipboard::is_node);
                        if ui.button("复制").clicked() {
                            app.engine.clipboard_copy_node(project_id, group, name, *kind);
                            app.tree_context = None;
                        }
                        let paste_label = if has_node_cb {
                            format!("粘贴节点（{}）", cb_label)
                        } else {
                            "粘贴节点".to_string()
                        };
                        if ui.add_enabled(has_node_cb, egui::Button::new(paste_label)).clicked() {
                            let pid_owned = project_id.clone();
                            let group_owned = group.clone();
                            app.engine.paste_node_to(&pid_owned, &group_owned);
                            app.tree_expanded.insert((pid_owned, group_owned));
                            app.tree_context = None;
                        }
                        ui.separator();
                        if ui.button("重命名").clicked() {
                            app.pending_action = Some(PendingAction::RenameNode {
                                project_id: project_id.clone(), group: group.clone(), old_name: name.clone(),
                            });
                            app.input_name = name.clone();
                            app.tree_context = None;
                        }
                        if ui.button("删除").clicked() {
                            app.pending_action = Some(PendingAction::DeleteNode {
                                project_id: project_id.clone(), group: group.clone(), name: name.clone(),
                            });
                            app.tree_context = None;
                        }
                    }
                }
            });
        });

    if ui.input(|i| i.pointer.primary_clicked() || i.key_pressed(egui::Key::Escape)) {
        app.tree_context = None;
    }
}

fn passes_filter(filter: &TreeFilter, deleted: bool, is_new: bool, dirty: bool) -> bool {
    match filter {
        TreeFilter::All => true,
        TreeFilter::New => is_new && !deleted,
        TreeFilter::Modified => dirty && !is_new && !deleted,
        TreeFilter::Deleted => deleted,
        TreeFilter::Changed => deleted || dirty || is_new,
    }
}

fn render_marker(ui: &mut egui::Ui, deleted: bool, is_new: bool, dirty: bool) {
    let (text, color) = if deleted {
        ("-", egui::Color32::from_rgb(220, 50, 50))
    } else if is_new {
        ("+", egui::Color32::from_rgb(40, 180, 40))
    } else if dirty {
        ("*", egui::Color32::from_rgb(200, 170, 0))
    } else {
        return;
    };
    ui.label(egui::RichText::new(text).color(color).strong());
}

/// 把 available_projects 按当前 sort 排序；closed 与 opened 一同进入。
/// "id"     ：字典序（默认）
/// "name"   ：display name 字典序
/// "open"   ：已打开优先 → id 字典序
/// "created"：created_at 字典序
/// "manual" ：跟随 project_order；缺席的排到末尾保持 id 序
fn sorted_available(
    available: &[AvailableProject],
    opened: &HashSet<String>,
    sort: &str,
    manual: &[String],
) -> Vec<AvailableProject> {
    let mut v: Vec<AvailableProject> = available.to_vec();
    match sort {
        "name" => v.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id))),
        "open" => v.sort_by(|a, b| {
            let ao = opened.contains(&a.id);
            let bo = opened.contains(&b.id);
            bo.cmp(&ao).then(a.id.cmp(&b.id))
        }),
        "created" => v.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id))),
        "manual" => {
            let pos = |id: &str| manual.iter().position(|x| x == id).unwrap_or(usize::MAX);
            v.sort_by(|a, b| pos(&a.id).cmp(&pos(&b.id)).then(a.id.cmp(&b.id)));
        }
        _ => v.sort_by(|a, b| a.id.cmp(&b.id)),
    }
    v
}

/// 把当前 workspace 状态落盘到 `<workdir>/tbl-tool.toml`；失败仅 log。
fn persist_workspace(app: &mut TblApp) {
    if let Err(e) = tbl_core::project::persist_workspace_state(
        &app.engine, &app.project_sort, &app.project_order,
    ) {
        app.engine.log(format!("[workspace] 持久化失败: {}", e));
    }
}

/// 打开一个 closed project，成功后 persist。失败 log。
/// 返回 true 表示真的开了一个新的。
fn open_project_with_persist(app: &mut TblApp, pid: &str) -> bool {
    match app.engine.open_project(pid) {
        Ok(true) => {
            persist_workspace(app);
            true
        }
        Ok(false) => false, // 已打开
        Err(e) => {
            app.engine.log(format!("[workspace] 打开 {} 失败: {}", pid, e));
            false
        }
    }
}

/// 关闭一个 opened project，成功后清掉相关 UI 态 + persist。
fn close_project_with_persist(app: &mut TblApp, pid: &str) {
    // close 前如果是 active selected 节点，清空 selected
    if matches!(&app.selected, Some(s) if s.project_id() == pid) {
        app.selected = None;
    }
    if app.engine.close_project(pid) {
        app.tree_expanded.retain(|(p, _)| p != pid);
        app.project_expanded.remove(pid);
        persist_workspace(app);
    }
}
