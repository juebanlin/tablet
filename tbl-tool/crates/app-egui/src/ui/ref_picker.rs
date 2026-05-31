use eframe::egui;
use crate::app::CellPos;
use crate::ui::grid_model::GridSource;
use tbl_core::model::{Group, Export};

/// 引用值选择弹窗：单元格类型为 @Xxx 时弹出，列出被引用项的条目供选择。
/// 选中后写入 id（数据文件层永远存 id）。
///
/// Table 引用列展示策略由 [`RefDisplayStrategy`] 控制；Enum 引用不受影响（永远 id/name/desc）。
#[derive(Clone, Debug)]
pub struct RefPickerState {
    pub open: bool,
    /// 被引用项名称（@HeroBase / @HeroType 中的 HeroBase / HeroType）
    pub ref_name: String,
    pub search: String,
    pub selected_id: String,
    /// 手动输入框：可与列表选中行双向同步，确认时优先取此值（不要求一定命中列表）
    pub manual_value: String,
    /// 列展示策略：每次打开重置回项目配置默认；本次会话内可临时切换
    pub strategy: RefDisplayStrategy,
    /// 编辑上下文（同 type_selector）
    pub editing_cell: Option<CellPos>,
    pub editing_group: String,
    pub editing_name: String,
    pub editing_source: GridSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefDisplayStrategy {
    /// id + 最多 2 个非引用、非复合、export != "-" 的辅助列
    Auto,
    /// schema 全部字段（除 export = "-" 不导出列）
    Full,
}

impl RefDisplayStrategy {
    pub fn from_config(s: &str) -> Self {
        match s {
            "full" => Self::Full,
            _ => Self::Auto,
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            Self::Auto => "智能",
            Self::Full => "全部",
        }
    }
}

impl Default for RefPickerState {
    fn default() -> Self {
        Self {
            open: false,
            ref_name: String::new(),
            search: String::new(),
            selected_id: String::new(),
            manual_value: String::new(),
            strategy: RefDisplayStrategy::Auto,
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
        default_strategy: RefDisplayStrategy,
    ) {
        self.open = true;
        self.ref_name = ref_name.to_string();
        self.selected_id = current_value.to_string();
        self.manual_value = current_value.to_string();
        self.strategy = default_strategy;
        self.search.clear();
        self.editing_cell = Some(cell);
        self.editing_group = group.to_string();
        self.editing_name = name.to_string();
        self.editing_source = source.clone();
    }
}

/// 引用条目：通用列结构。第 0 列固定为 id，后续列按策略推导。
#[derive(Clone, Debug)]
struct RefRow {
    id: String,
    /// 与 [`RefTarget::headers`] 等长（不含 id 那列）；Enum 时为 [name, desc]，Table 时按策略变化
    extras: Vec<String>,
}

#[derive(Clone, Debug)]
struct RefTarget {
    kind: RefTargetKind,
    /// 不含 id 列的辅助列表头（id 永远是第一列）
    headers: Vec<String>,
    rows: Vec<RefRow>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum RefTargetKind {
    Table,
    Enum,
}

/// auto 策略：第一个 id 之后的字段池里挑前 N 个"辅助识别列"。
/// 跳过：被删/不导出（export=-）/类型以 @ 开头的引用列/类型含 < 的复合列（List/Map 等）。
fn pick_auto_extras(table: &tbl_core::model::Table) -> Vec<usize> {
    const MAX_EXTRAS: usize = 2;
    let mut picked = Vec::new();
    for (idx, f) in table.schema.fields.iter().enumerate() {
        if f.name == "id" { continue; }
        if matches!(f.export, Export::None) { continue; }
        let t = f.tbl_type.trim();
        if t.starts_with('@') { continue; }
        if t.contains('<') { continue; }
        picked.push(idx);
        if picked.len() >= MAX_EXTRAS { break; }
    }
    picked
}

/// full 策略：除 id 外所有 export != "-" 的列
fn pick_full_extras(table: &tbl_core::model::Table) -> Vec<usize> {
    table.schema.fields.iter().enumerate()
        .filter(|(_, f)| f.name != "id" && !matches!(f.export, Export::None))
        .map(|(idx, _)| idx)
        .collect()
}

fn collect_target(groups: &[Group], ref_name: &str, strategy: RefDisplayStrategy) -> Option<RefTarget> {
    for g in groups {
        for t in &g.tables {
            if t.deleted || t.name != ref_name { continue; }
            let id_idx = match t.schema.fields.iter().position(|f| f.name == "id") {
                Some(i) => i,
                None => return Some(RefTarget {
                    kind: RefTargetKind::Table,
                    headers: vec![],
                    rows: vec![],
                }),
            };
            let extras_idx = match strategy {
                RefDisplayStrategy::Auto => pick_auto_extras(t),
                RefDisplayStrategy::Full => pick_full_extras(t),
            };
            let headers: Vec<String> = extras_idx.iter()
                .map(|&i| t.schema.fields[i].name.clone())
                .collect();
            let rows: Vec<RefRow> = t.records.iter()
                .filter_map(|row| {
                    let id = row.get(id_idx).cloned().unwrap_or_default();
                    if id.is_empty() { return None; }
                    let extras: Vec<String> = extras_idx.iter()
                        .map(|&i| row.get(i).cloned().unwrap_or_default())
                        .collect();
                    Some(RefRow { id, extras })
                })
                .collect();
            return Some(RefTarget { kind: RefTargetKind::Table, headers, rows });
        }
        for e in &g.enums {
            if e.deleted || e.name != ref_name { continue; }
            let rows: Vec<RefRow> = e.entries.iter()
                .filter(|en| !en.id.is_empty())
                .map(|en| RefRow {
                    id: en.id.clone(),
                    extras: vec![en.name.clone(), en.desc.clone()],
                })
                .collect();
            return Some(RefTarget {
                kind: RefTargetKind::Enum,
                headers: vec!["name".to_string(), "desc".to_string()],
                rows,
            });
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

    if super::modal::modal_scrim(ctx, "ref_picker") {
        state.open = false;
        return None;
    }

    let mut result: Option<String> = None;
    let mut close = false;

    let target = collect_target(groups, &state.ref_name, state.strategy);

    let title = format!("选择引用: @{}", state.ref_name);
    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            // ── 顶部：手动输入框（始终可见，与列表选中双向同步）──
            ui.horizontal(|ui| {
                ui.label("当前值:");
                let resp = ui.add(egui::TextEdit::singleline(&mut state.manual_value)
                    .desired_width(220.0)
                    .hint_text("可直接输入，或在下方列表选择"));
                if resp.changed() {
                    state.selected_id = state.manual_value.clone();
                }
            });

            match &target {
                None => {
                    ui.colored_label(
                        egui::Color32::from_rgb(220, 50, 50),
                        format!("⚠️ 引用的配置项 \"{}\" 不存在", state.ref_name),
                    );
                }
                Some(t) => {
                    ui.horizontal(|ui| {
                        let kind_label = match t.kind {
                            RefTargetKind::Table => "📊 表引用",
                            RefTargetKind::Enum => "🔢 枚举引用",
                        };
                        ui.label(egui::RichText::new(kind_label).size(11.0).weak());
                        // Table 才显示列展示切换；Enum 永远固定 id/name/desc
                        if matches!(t.kind, RefTargetKind::Table) {
                            ui.separator();
                            ui.label(egui::RichText::new("列展示:").size(11.0));
                            ui.selectable_value(&mut state.strategy, RefDisplayStrategy::Auto, "智能");
                            ui.selectable_value(&mut state.strategy, RefDisplayStrategy::Full, "全部");
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("搜索:");
                        ui.add(egui::TextEdit::singleline(&mut state.search).desired_width(260.0));
                    });
                    ui.separator();

                    let search = state.search.to_lowercase();
                    let filtered: Vec<&RefRow> = t.rows.iter().filter(|r| {
                        if search.is_empty() { return true; }
                        if r.id.to_lowercase().contains(&search) { return true; }
                        r.extras.iter().any(|e| e.to_lowercase().contains(&search))
                    }).collect();

                    // 动态列宽：id 列 80px，每个 extra 列 140px
                    let id_w = 80.0;
                    let extra_w = 140.0;
                    let row_h = 22.0;
                    let total_w = id_w + extra_w * t.headers.len() as f32;
                    ui.set_min_width((total_w + 40.0).max(360.0));

                    // 表头
                    let (header_rect, _) = ui.allocate_exact_size(
                        egui::vec2(total_w, row_h),
                        egui::Sense::hover(),
                    );
                    let painter = ui.painter();
                    let header_bg = ui.visuals().widgets.noninteractive.bg_fill;
                    painter.rect_filled(header_rect, 0.0, header_bg);
                    let font = egui::FontId::proportional(11.0);
                    let strong_color = ui.visuals().strong_text_color();
                    painter.text(
                        header_rect.left_center() + egui::vec2(6.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        "id",
                        font.clone(),
                        strong_color,
                    );
                    for (i, h) in t.headers.iter().enumerate() {
                        let x = id_w + extra_w * i as f32 + 6.0;
                        painter.text(
                            header_rect.left_center() + egui::vec2(x, 0.0),
                            egui::Align2::LEFT_CENTER,
                            h,
                            font.clone(),
                            strong_color,
                        );
                    }
                    let grid_color = ui.visuals().widgets.noninteractive.bg_stroke.color;
                    let stroke = egui::Stroke::new(1.0, grid_color);
                    painter.line_segment([header_rect.left_bottom(), header_rect.right_bottom()], stroke);
                    for i in 0..=t.headers.len() {
                        let x = header_rect.left() + id_w + extra_w * i as f32 - extra_w;
                        if i == 0 {
                            let xx = header_rect.left() + id_w;
                            painter.line_segment([egui::pos2(xx, header_rect.top()), egui::pos2(xx, header_rect.bottom())], stroke);
                        } else {
                            let xx = header_rect.left() + id_w + extra_w * i as f32;
                            if xx < header_rect.right() {
                                painter.line_segment([egui::pos2(xx, header_rect.top()), egui::pos2(xx, header_rect.bottom())], stroke);
                            }
                        }
                        let _ = x; // keep variable used
                    }

                    let mut double_confirm = false;
                    egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
                        if filtered.is_empty() {
                            ui.label(egui::RichText::new("（无匹配条目）").weak());
                        }
                        for row in &filtered {
                            let selected = state.selected_id == row.id;
                            let (rect, resp) = ui.allocate_exact_size(
                                egui::vec2(total_w, row_h),
                                egui::Sense::click(),
                            );
                            let painter = ui.painter();

                            let bg = if selected {
                                ui.visuals().selection.bg_fill
                            } else if resp.hovered() {
                                ui.visuals().widgets.hovered.bg_fill
                            } else {
                                egui::Color32::TRANSPARENT
                            };
                            if bg != egui::Color32::TRANSPARENT {
                                painter.rect_filled(rect, 0.0, bg);
                            }

                            let text_color = if selected {
                                ui.visuals().selection.stroke.color
                            } else {
                                ui.visuals().text_color()
                            };
                            let text_font = egui::FontId::proportional(13.0);
                            painter.text(
                                rect.left_center() + egui::vec2(6.0, 0.0),
                                egui::Align2::LEFT_CENTER,
                                &row.id,
                                text_font.clone(),
                                text_color,
                            );
                            for (i, val) in row.extras.iter().enumerate() {
                                let x = id_w + extra_w * i as f32 + 6.0;
                                painter.text(
                                    rect.left_center() + egui::vec2(x, 0.0),
                                    egui::Align2::LEFT_CENTER,
                                    val,
                                    text_font.clone(),
                                    text_color,
                                );
                            }

                            // 列分隔 + 行底
                            let xx = rect.left() + id_w;
                            painter.line_segment([egui::pos2(xx, rect.top()), egui::pos2(xx, rect.bottom())], stroke);
                            for i in 1..=t.headers.len() {
                                let xx = rect.left() + id_w + extra_w * i as f32;
                                if xx < rect.right() {
                                    painter.line_segment([egui::pos2(xx, rect.top()), egui::pos2(xx, rect.bottom())], stroke);
                                }
                            }
                            painter.line_segment([rect.left_bottom(), rect.right_bottom()], stroke);

                            if resp.clicked() {
                                state.selected_id = row.id.clone();
                                state.manual_value = row.id.clone();
                            }
                            if resp.double_clicked() {
                                state.selected_id = row.id.clone();
                                state.manual_value = row.id.clone();
                                double_confirm = true;
                            }
                        }
                    });
                    if double_confirm {
                        result = Some(state.selected_id.clone());
                        close = true;
                    }

                    ui.add_space(4.0);
                    ui.separator();
                    if state.selected_id.is_empty() {
                        ui.label(egui::RichText::new("当前选择: （未选择）").size(11.0).weak());
                    } else {
                        let preview = t.rows.iter().find(|r| r.id == state.selected_id)
                            .map(|r| {
                                let nm = r.extras.first().cloned().unwrap_or_default();
                                if nm.is_empty() { r.id.clone() } else { format!("{} ({})", r.id, nm) }
                            })
                            .unwrap_or_else(|| format!("{} ⚠️ 不在列表中（手动输入）", state.selected_id));
                        ui.label(egui::RichText::new(format!("当前选择: {}", preview)).size(11.0));
                    }
                }
            }

            ui.add_space(6.0);
            super::modal::dialog_buttons(ui, |ui| {
                if ui.button("取消").clicked() {
                    close = true;
                }
                // 确定按钮：永远启用（手动输入无须命中列表）
                if ui.button("确定").clicked() {
                    // 优先以手动输入框为准，落空时回退到 selected_id
                    let val = if !state.manual_value.is_empty() {
                        state.manual_value.clone()
                    } else {
                        state.selected_id.clone()
                    };
                    result = Some(val);
                    close = true;
                }
                if ui.button("清空").clicked() {
                    result = Some(String::new());
                    close = true;
                }
            });
        });

    if close { state.open = false; }
    result
}
