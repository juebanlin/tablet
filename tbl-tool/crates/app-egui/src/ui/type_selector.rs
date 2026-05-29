use eframe::egui;
use crate::app::CellPos;
use tbl_core::types::{BaseType, Paradigm, TblType, SeparatorsSection};
use tbl_core::model::Group;
use crate::ui::grid_model::GridSource;

#[derive(Clone, Debug, PartialEq)]
pub enum SelectorTab {
    Data,
    Reference,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RefFilter {
    All,
    Table,
    Enum,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RefKind {
    Table,
    Enum,
}

#[derive(Clone, Debug)]
pub struct TypeSelectorState {
    pub open: bool,
    pub tab: SelectorTab,
    // Data tab state
    pub paradigm: Paradigm,
    pub params: Vec<BaseType>,
    // Reference tab state
    pub ref_name: String,
    pub ref_filter: RefFilter,
    pub ref_search: String,
    // Editing context
    pub editing_cell: Option<CellPos>,
    pub editing_group: String,
    pub editing_name: String,
    pub editing_source: GridSource,
}

impl Default for TypeSelectorState {
    fn default() -> Self {
        Self {
            open: false,
            tab: SelectorTab::Data,
            paradigm: Paradigm::Base,
            params: vec![BaseType::Int],
            ref_name: String::new(),
            ref_filter: RefFilter::All,
            ref_search: String::new(),
            editing_cell: None,
            editing_group: String::new(),
            editing_name: String::new(),
            editing_source: GridSource::Table,
        }
    }
}

impl TypeSelectorState {
    pub fn open_with(&mut self, current_type: &str, cell: CellPos, group: &str, name: &str, source: &GridSource) {
        self.open = true;
        self.editing_cell = Some(cell);
        self.editing_group = group.to_string();
        self.editing_name = name.to_string();
        self.editing_source = source.clone();
        self.ref_search.clear();
        self.ref_filter = RefFilter::All;
        if let Some(t) = TblType::parse(current_type) {
            if t.paradigm == Paradigm::Ref {
                self.tab = SelectorTab::Reference;
                self.ref_name = t.ref_name.clone().unwrap_or_default();
                self.paradigm = Paradigm::Base;
                self.params = vec![BaseType::Int];
            } else {
                self.tab = SelectorTab::Data;
                self.paradigm = t.paradigm;
                self.params = t.params;
                self.ref_name.clear();
            }
        } else {
            self.tab = SelectorTab::Data;
            self.paradigm = Paradigm::Base;
            self.params = vec![BaseType::Int];
            self.ref_name.clear();
        }
    }

    fn sync_params(&mut self) {
        let count = self.paradigm.param_slots().len();
        self.params.resize(count, BaseType::Int);
    }

    fn data_type(&self) -> TblType {
        TblType {
            paradigm: self.paradigm.clone(),
            params: self.params.clone(),
            ref_name: None,
        }
    }

    fn ref_type(&self) -> Option<TblType> {
        if self.ref_name.is_empty() { return None; }
        Some(TblType::new_ref(self.ref_name.clone()))
    }
}

/// 收集项目内所有可被引用的项（table + enum，排除已删除）
fn collect_ref_targets(groups: &[Group]) -> Vec<(String, RefKind)> {
    let mut out = Vec::new();
    for g in groups {
        for t in &g.tables {
            if !t.deleted { out.push((t.name.clone(), RefKind::Table)); }
        }
        for e in &g.enums {
            if !e.deleted { out.push((e.name.clone(), RefKind::Enum)); }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

pub fn render_type_selector(
    ctx: &egui::Context,
    state: &mut TypeSelectorState,
    sep: &SeparatorsSection,
    groups: &[Group],
) -> Option<String> {
    if !state.open { return None; }

    let mut result: Option<String> = None;
    let mut close = false;

    // constant 表禁用引用 tab
    let ref_disabled = matches!(state.editing_source, GridSource::Constant);
    if ref_disabled && state.tab == SelectorTab::Reference {
        state.tab = SelectorTab::Data;
    }

    egui::Window::new("选择类型")
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            // Tabs
            ui.horizontal(|ui| {
                ui.selectable_value(&mut state.tab, SelectorTab::Data, "数据类型");
                ui.add_enabled_ui(!ref_disabled, |ui| {
                    ui.selectable_value(&mut state.tab, SelectorTab::Reference, "引用类型");
                });
                if ref_disabled {
                    ui.label(egui::RichText::new("（constant 不允许引用）").size(11.0).weak());
                }
            });
            ui.separator();

            match state.tab {
                SelectorTab::Data => render_data_tab(ui, state, sep),
                SelectorTab::Reference => render_ref_tab(ui, state, groups),
            }

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let confirm_enabled = match state.tab {
                    SelectorTab::Data => true,
                    SelectorTab::Reference => !state.ref_name.is_empty(),
                };
                if ui.add_enabled(confirm_enabled, egui::Button::new("确定")).clicked() {
                    let s = match state.tab {
                        SelectorTab::Data => state.data_type().to_type_string(),
                        SelectorTab::Reference => state.ref_type()
                            .map(|t| t.to_type_string())
                            .unwrap_or_default(),
                    };
                    if !s.is_empty() {
                        result = Some(s);
                        close = true;
                    }
                }
                if ui.button("取消").clicked() {
                    close = true;
                }
            });
        });

    if close { state.open = false; }
    result
}

fn render_data_tab(ui: &mut egui::Ui, state: &mut TypeSelectorState, sep: &SeparatorsSection) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_min_width(160.0);
            ui.set_max_height(280.0);
            egui::ScrollArea::vertical().show(ui, |ui| {
                for p in Paradigm::all_data() {
                    let selected = &state.paradigm == p;
                    if ui.selectable_label(selected, p.label()).clicked() {
                        state.paradigm = p.clone();
                        state.sync_params();
                    }
                }
            });
        });

        ui.separator();

        ui.vertical(|ui| {
            ui.set_min_width(200.0);
            ui.label(egui::RichText::new("参数").size(11.0).strong());
            ui.separator();

            let slots = state.paradigm.param_slots();
            state.params.resize(slots.len(), BaseType::Int);

            for (i, slot) in slots.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("{}:", slot.label));
                    let options = if slot.is_map_key { BaseType::map_key_types() } else { BaseType::all() };
                    egui::ComboBox::from_id_source(format!("type_param_{}", i))
                        .selected_text(state.params[i].name())
                        .show_ui(ui, |ui| {
                            for &opt in options {
                                ui.selectable_value(&mut state.params[i], opt, opt.name());
                            }
                        });
                });
            }

            ui.add_space(8.0);
            ui.separator();
            let tbl_type = state.data_type();
            ui.label(egui::RichText::new(format!("结果: {}", tbl_type.to_type_string())).size(11.0));
            ui.label(egui::RichText::new(format!("示例: {}", tbl_type.example_with_sep(sep))).size(11.0));
            ui.label(egui::RichText::new(format!("Java: {}", tbl_type.java_decl())).size(11.0).weak());
            ui.label(egui::RichText::new(format!("Go:   {}", tbl_type.go_decl())).size(11.0).weak());
            ui.label(egui::RichText::new(format!("Lua:  {}", tbl_type.lua_decl())).size(11.0).weak());
        });
    });
}

fn render_ref_tab(ui: &mut egui::Ui, state: &mut TypeSelectorState, groups: &[Group]) {
    let targets = collect_ref_targets(groups);

    ui.horizontal(|ui| {
        ui.label("过滤:");
        ui.selectable_value(&mut state.ref_filter, RefFilter::All, "全部");
        ui.selectable_value(&mut state.ref_filter, RefFilter::Table, "📊 表");
        ui.selectable_value(&mut state.ref_filter, RefFilter::Enum, "🔢 枚举");
    });
    ui.horizontal(|ui| {
        ui.label("搜索:");
        ui.add(egui::TextEdit::singleline(&mut state.ref_search).desired_width(220.0));
    });

    ui.separator();

    let search = state.ref_search.to_lowercase();
    let filtered: Vec<&(String, RefKind)> = targets.iter().filter(|(name, kind)| {
        let kind_ok = match state.ref_filter {
            RefFilter::All => true,
            RefFilter::Table => *kind == RefKind::Table,
            RefFilter::Enum => *kind == RefKind::Enum,
        };
        let search_ok = search.is_empty() || name.to_lowercase().contains(&search);
        kind_ok && search_ok
    }).collect();

    ui.set_min_width(380.0);
    egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
        if filtered.is_empty() {
            ui.label(egui::RichText::new("（项目中没有可引用的项）").weak());
        }
        for (name, kind) in &filtered {
            let icon = match kind { RefKind::Table => "📊", RefKind::Enum => "🔢" };
            let label = format!("{} {}", icon, name);
            let selected = state.ref_name == **name;
            if ui.selectable_label(selected, label).clicked() {
                state.ref_name = name.clone();
            }
        }
    });

    ui.add_space(8.0);
    ui.separator();
    if state.ref_name.is_empty() {
        ui.label(egui::RichText::new("结果: （未选择）").size(11.0).weak());
    } else {
        // 找到 kind，渲染语言级声明
        let kind = targets.iter().find(|(n, _)| n == &state.ref_name).map(|(_, k)| k.clone());
        ui.label(egui::RichText::new(format!("结果: @{}", state.ref_name)).size(11.0));
        match kind {
            Some(RefKind::Table) => {
                ui.label(egui::RichText::new("Java: int  Go: int32  Lua: number").size(11.0).weak());
            }
            Some(RefKind::Enum) => {
                let cls = format!("{}Enum", state.ref_name);
                ui.label(egui::RichText::new(format!("Java: {}  Go: {}  Lua: number", cls, cls)).size(11.0).weak());
            }
            None => {
                ui.label(egui::RichText::new("⚠️ 选中项不存在")
                    .color(egui::Color32::from_rgb(220, 50, 50)).size(11.0));
            }
        }
    }
}
