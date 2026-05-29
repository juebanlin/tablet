use eframe::egui;
use crate::app::CellPos;
use tbl_core::types::{BaseType, Paradigm, TblType, SeparatorsSection};
use crate::ui::grid_model::GridSource;

#[derive(Clone, Debug)]
pub struct TypeSelectorState {
    pub open: bool,
    pub paradigm: Paradigm,
    pub params: Vec<BaseType>,
    pub editing_cell: Option<CellPos>,
    pub editing_group: String,
    pub editing_name: String,
    pub editing_source: GridSource,
}

impl Default for TypeSelectorState {
    fn default() -> Self {
        Self {
            open: false,
            paradigm: Paradigm::Base,
            params: vec![BaseType::Int],
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
        if let Some(t) = TblType::parse(current_type) {
            self.paradigm = t.paradigm;
            self.params = t.params;
        } else {
            self.paradigm = Paradigm::Base;
            self.params = vec![BaseType::Int];
        }
    }

    fn sync_params(&mut self) {
        let count = self.paradigm.param_slots().len();
        self.params.resize(count, BaseType::Int);
    }

    fn current_type(&self) -> TblType {
        TblType {
            paradigm: self.paradigm.clone(),
            params: self.params.clone(),
            ref_name: None,
        }
    }
}

pub fn render_type_selector(ctx: &egui::Context, state: &mut TypeSelectorState, sep: &SeparatorsSection) -> Option<String> {
    if !state.open { return None; }

    let mut result: Option<String> = None;
    let mut close = false;

    egui::Window::new("选择类型")
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_min_width(130.0);
                    ui.set_max_height(260.0);
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for p in Paradigm::all() {
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
                    ui.set_min_width(180.0);
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
                    let tbl_type = state.current_type();
                    ui.label(egui::RichText::new(format!("结果: {}", tbl_type.to_type_string())).size(11.0));
                    ui.label(egui::RichText::new(format!("示例: {}", tbl_type.example_with_sep(sep))).size(11.0));
                    ui.label(egui::RichText::new(format!("Java: {}", tbl_type.java_decl())).size(11.0).weak());
                    ui.label(egui::RichText::new(format!("Go:   {}", tbl_type.go_decl())).size(11.0).weak());
                    ui.label(egui::RichText::new(format!("Lua:  {}", tbl_type.lua_decl())).size(11.0).weak());

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("确定").clicked() {
                            result = Some(tbl_type.to_type_string());
                            close = true;
                        }
                        if ui.button("取消").clicked() {
                            close = true;
                        }
                    });
                });
            });
        });

    if close { state.open = false; }
    result
}
