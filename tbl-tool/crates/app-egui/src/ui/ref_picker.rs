use eframe::egui;
use crate::app::CellPos;
use crate::ui::grid_model::GridSource;
use tbl_core::model::Group;

/// 引用值选择弹窗：单元格类型为 @Xxx 时弹出，列出被引用项的条目供选择。
/// 选中后写入 id（数据文件层永远存 id）。
#[derive(Clone, Debug)]
pub struct RefPickerState {
    pub open: bool,
    /// 被引用项名称（@HeroBase / @HeroType 中的 HeroBase / HeroType）
    pub ref_name: String,
    pub search: String,
    pub selected_id: String,
    /// 编辑上下文（同 type_selector）
    pub editing_cell: Option<CellPos>,
    pub editing_group: String,
    pub editing_name: String,
    pub editing_source: GridSource,
}

impl Default for RefPickerState {
    fn default() -> Self {
        Self {
            open: false,
            ref_name: String::new(),
            search: String::new(),
            selected_id: String::new(),
            editing_cell: None,
            editing_group: String::new(),
            editing_name: String::new(),
            editing_source: GridSource::Table,
        }
    }
}

impl RefPickerState {
    pub fn open_with(
        &mut self,
        ref_name: &str,
        current_value: &str,
        cell: CellPos,
        group: &str,
        name: &str,
        source: &GridSource,
    ) {
        self.open = true;
        self.ref_name = ref_name.to_string();
        self.selected_id = current_value.to_string();
        self.search.clear();
        self.editing_cell = Some(cell);
        self.editing_group = group.to_string();
        self.editing_name = name.to_string();
        self.editing_source = source.clone();
    }
}

/// 引用条目：(id, name, desc)
#[derive(Clone, Debug)]
struct RefRow {
    id: String,
    name: String,
    desc: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum RefTargetKind {
    Table,
    Enum,
}

fn collect_target_rows(groups: &[Group], ref_name: &str) -> Option<(RefTargetKind, Vec<RefRow>)> {
    for g in groups {
        for t in &g.tables {
            if t.deleted || t.name != ref_name { continue; }
            let id_idx = t.schema.fields.iter().position(|f| f.name == "id");
            let name_idx = t.schema.fields.iter().position(|f| f.name == "name");
            let mut rows = Vec::new();
            if let Some(idx) = id_idx {
                for row in &t.records {
                    let id = row.get(idx).cloned().unwrap_or_default();
                    if id.is_empty() { continue; }
                    let nm = name_idx.and_then(|i| row.get(i).cloned()).unwrap_or_default();
                    rows.push(RefRow { id, name: nm, desc: String::new() });
                }
            }
            return Some((RefTargetKind::Table, rows));
        }
        for e in &g.enums {
            if e.deleted || e.name != ref_name { continue; }
            let rows: Vec<RefRow> = e.entries.iter()
                .filter(|en| !en.id.is_empty())
                .map(|en| RefRow { id: en.id.clone(), name: en.name.clone(), desc: en.desc.clone() })
                .collect();
            return Some((RefTargetKind::Enum, rows));
        }
    }
    None
}

pub fn render_ref_picker(
    ctx: &egui::Context,
    state: &mut RefPickerState,
    groups: &[Group],
) -> Option<String> {
    if !state.open { return None; }

    let mut result: Option<String> = None;
    let mut close = false;

    let target = collect_target_rows(groups, &state.ref_name);

    let title = format!("选择引用: @{}", state.ref_name);
    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            match &target {
                None => {
                    ui.colored_label(
                        egui::Color32::from_rgb(220, 50, 50),
                        format!("⚠️ 引用的配置项 \"{}\" 不存在", state.ref_name),
                    );
                }
                Some((kind, rows)) => {
                    let kind_label = match kind {
                        RefTargetKind::Table => "📊 表引用",
                        RefTargetKind::Enum => "🔢 枚举引用",
                    };
                    ui.label(egui::RichText::new(kind_label).size(11.0).weak());
                    ui.horizontal(|ui| {
                        ui.label("搜索:");
                        ui.add(egui::TextEdit::singleline(&mut state.search).desired_width(260.0));
                    });
                    ui.separator();

                    let search = state.search.to_lowercase();
                    let filtered: Vec<&RefRow> = rows.iter().filter(|r| {
                        if search.is_empty() { return true; }
                        r.id.to_lowercase().contains(&search)
                            || r.name.to_lowercase().contains(&search)
                            || r.desc.to_lowercase().contains(&search)
                    }).collect();

                    ui.set_min_width(420.0);
                    egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
                        if filtered.is_empty() {
                            ui.label(egui::RichText::new("（无匹配条目）").weak());
                        }
                        egui::Grid::new("ref_picker_grid")
                            .num_columns(3)
                            .striped(true)
                            .min_col_width(80.0)
                            .show(ui, |ui| {
                                ui.label(egui::RichText::new("id").size(11.0).strong());
                                ui.label(egui::RichText::new("name").size(11.0).strong());
                                let desc_header = match kind {
                                    RefTargetKind::Enum => "desc",
                                    RefTargetKind::Table => "",
                                };
                                ui.label(egui::RichText::new(desc_header).size(11.0).strong());
                                ui.end_row();

                                for row in &filtered {
                                    let selected = state.selected_id == row.id;
                                    let label = format!("{}", row.id);
                                    if ui.selectable_label(selected, label).clicked() {
                                        state.selected_id = row.id.clone();
                                    }
                                    if ui.selectable_label(selected, &row.name).clicked() {
                                        state.selected_id = row.id.clone();
                                    }
                                    ui.label(&row.desc);
                                    ui.end_row();
                                }
                            });
                    });

                    ui.add_space(4.0);
                    ui.separator();
                    if state.selected_id.is_empty() {
                        ui.label(egui::RichText::new("当前选择: （未选择）").size(11.0).weak());
                    } else {
                        let preview = rows.iter().find(|r| r.id == state.selected_id)
                            .map(|r| if r.name.is_empty() { r.id.clone() } else { format!("{} ({})", r.id, r.name) })
                            .unwrap_or_else(|| format!("{} ⚠️ 不存在", state.selected_id));
                        ui.label(egui::RichText::new(format!("当前选择: {}", preview)).size(11.0));
                    }
                }
            }

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let confirm_enabled = target.is_some();
                if ui.add_enabled(confirm_enabled, egui::Button::new("确定")).clicked() {
                    result = Some(state.selected_id.clone());
                    close = true;
                }
                if ui.button("清空").clicked() {
                    result = Some(String::new());
                    close = true;
                }
                if ui.button("取消").clicked() {
                    close = true;
                }
            });
        });

    if close { state.open = false; }
    result
}
