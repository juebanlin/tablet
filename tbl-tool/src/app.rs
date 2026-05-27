use std::collections::HashSet;
use eframe::egui;
use crate::model::*;
use crate::ui;

pub struct TblApp {
    pub project: Project,
    pub selected: Option<SelectedNode>,
    pub pending_action: Option<PendingAction>,
    pub input_name: String,
    pub logs: Vec<String>,
    pub edit_state: EditState,
    pub context_col: Option<usize>,
    pub context_row: Option<usize>,
    pub context_pos: egui::Pos2,
    pub auto_commit_on_blur: bool,
    pub tree_filter: TreeFilter,
    pub tree_filter_show_full_group: bool,
    pub tree_expanded: HashSet<String>,
    pub tree_context: Option<TreeContext>,
    theme_applied: bool,
}

#[derive(Clone, Debug)]
pub enum TreeContext {
    Group(String),
    Node { group: String, name: String, is_table: bool },
    Blank,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum TreeFilter {
    #[default]
    All,
    New,
    Modified,
    Deleted,
    Changed,
}

#[derive(Clone, Debug, Default)]
pub struct EditState {
    pub selected: Selection,
    pub editing: Option<CellPos>,
    pub edit_buffer: String,
    pub edit_pos: Option<egui::Pos2>,
    pub drag_start: Option<(usize, usize)>,
    pub commit_pending: bool,
    pub hover_cell: Option<(usize, usize)>,
    pub formula_buffer: String,
    pub formula_committed: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Selection {
    #[default]
    None,
    Cell(usize, usize),
    CellRange { start: (usize, usize), end: (usize, usize) },
    Row(usize),
    Rows(usize, usize),
    Col(usize),
}

#[derive(Clone, Debug, PartialEq)]
pub struct CellPos {
    pub row: usize, // usize::MAX means header row
    pub col: usize,
    pub header_row: Option<usize>, // 0=desc,1=export,2=type,3=field
}

#[derive(Clone, Debug)]
pub enum SelectedNode {
    Table { group: String, name: String },
    Constant { group: String, name: String },
}

#[derive(Clone, Debug)]
pub enum PendingAction {
    NewGroup,
    NewTable { group: String },
    NewConstant { group: String },
    DeleteGroup { group: String },
    DeleteNode { group: String, name: String },
    RenameGroup { old_name: String },
    RenameNode { group: String, old_name: String },
    CopyNode { group: String, name: String, is_table: bool },
}

impl TblApp {
    pub fn new(project: Project) -> Self {
        let group_count = project.groups.len();
        let auto_commit = project.config.ui.as_ref().map_or(true, |u| u.auto_commit_on_blur);
        let expanded: HashSet<String> = project.groups.iter().map(|g| g.name.clone()).collect();
        let mut app = Self {
            project,
            selected: None,
            pending_action: None,
            input_name: String::new(),
            logs: Vec::new(),
            edit_state: EditState::default(),
            context_col: None,
            context_row: None,
            context_pos: egui::Pos2::ZERO,
            auto_commit_on_blur: auto_commit,
            tree_filter: TreeFilter::All,
            tree_filter_show_full_group: false,
            tree_expanded: expanded,
            tree_context: None,
            theme_applied: false,
        };
        app.log(format!("已加载 {} 个 Group", group_count));
        app
    }

    fn apply_theme(&self, ctx: &egui::Context) {
        let mut visuals = egui::Visuals::light();
        visuals.window_rounding = egui::Rounding::ZERO;
        visuals.window_shadow = egui::epaint::Shadow::NONE;
        visuals.popup_shadow = egui::epaint::Shadow::NONE;
        visuals.widgets.noninteractive.rounding = egui::Rounding::ZERO;
        visuals.widgets.inactive.rounding = egui::Rounding::ZERO;
        visuals.widgets.hovered.rounding = egui::Rounding::ZERO;
        visuals.widgets.active.rounding = egui::Rounding::ZERO;
        visuals.widgets.open.rounding = egui::Rounding::ZERO;
        visuals.menu_rounding = egui::Rounding::ZERO;
        ctx.set_visuals(visuals);

        let mut style = (*ctx.style()).clone();
        style.spacing.button_padding = egui::vec2(4.0, 2.0);
        style.spacing.item_spacing = egui::vec2(6.0, 4.0);
        ctx.set_style(style);
    }

    pub fn find_table(&self, group: &str, name: &str) -> Option<&Table> {
        self.project.groups.iter()
            .find(|g| g.name == group)?
            .tables.iter()
            .find(|t| t.name == name)
    }

    pub fn find_constant(&self, group: &str, name: &str) -> Option<&Constant> {
        self.project.groups.iter()
            .find(|g| g.name == group)?
            .constants.iter()
            .find(|c| c.name == name)
    }

    pub fn log(&mut self, msg: String) {
        let now = chrono::Local::now().format("%H:%M:%S").to_string();
        self.logs.push(format!("{} {}", now, msg));
    }

    pub fn mark_table_dirty(&mut self, group: &str, name: &str) {
        if let Some(g) = self.project.groups.iter_mut().find(|g| g.name == group) {
            if let Some(t) = g.tables.iter_mut().find(|t| t.name == name) {
                t.update_dirty();
            }
        }
    }

    pub fn mark_constant_dirty(&mut self, group: &str, name: &str) {
        if let Some(g) = self.project.groups.iter_mut().find(|g| g.name == group) {
            if let Some(c) = g.constants.iter_mut().find(|c| c.name == name) {
                c.update_dirty();
            }
        }
    }

    fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        for group in &self.project.groups {
            for table in &group.tables {
                if table.deleted { continue; }
                let index_col = table.schema.fields.iter().position(|f| f.name == table.schema.index);
                if let Some(idx) = index_col {
                    let mut seen_ids = std::collections::HashSet::new();
                    for (i, row) in table.records.iter().enumerate() {
                        let id = row.get(idx).map(|s| s.as_str()).unwrap_or("");
                        if id.is_empty() { continue; }
                        if id.parse::<i64>().is_err() {
                            errors.push(format!("[验证] {}/{} 第{}行: ID \"{}\" 必须是数字", group.name, table.name, i + 1, id));
                        }
                        if !seen_ids.insert(id.to_string()) {
                            errors.push(format!("[验证] {}/{} 第{}行: ID \"{}\" 重复", group.name, table.name, i + 1, id));
                        }
                    }
                }
            }
            for constant in &group.constants {
                if constant.deleted { continue; }
                let mut seen_names = std::collections::HashSet::new();
                for (i, entry) in constant.entries.iter().enumerate() {
                    if entry.name.is_empty() { continue; }
                    if entry.name.contains(' ') {
                        errors.push(format!("[验证] {}/{} 第{}行: name \"{}\" 不能含空格", group.name, constant.name, i + 1, entry.name));
                    }
                    if !is_valid_identifier(&entry.name) {
                        errors.push(format!("[验证] {}/{} 第{}行: name \"{}\" 不是合法标识符", group.name, constant.name, i + 1, entry.name));
                    }
                    if is_java_keyword(&entry.name) || is_lua_keyword(&entry.name) {
                        errors.push(format!("[验证] {}/{} 第{}行: name \"{}\" 是语言关键字", group.name, constant.name, i + 1, entry.name));
                    }
                    if !seen_names.insert(&entry.name) {
                        errors.push(format!("[验证] {}/{} 第{}行: name \"{}\" 重复", group.name, constant.name, i + 1, entry.name));
                    }
                }
            }
        }
        errors
    }

    pub fn save_all(&mut self) {
        let errors = self.validate();
        if !errors.is_empty() {
            for e in &errors { self.logs.push(e.clone()); }
            self.logs.push(format!("[保存失败] 共 {} 个验证错误", errors.len()));
            return;
        }

        use crate::core::tbl;
        let mut count = 0;
        let mut deleted = 0;
        for group in &mut self.project.groups {
            if group.is_new {
                let _ = std::fs::create_dir_all(&group.dir);
                group.is_new = false;
            }
            for table in &mut group.tables {
                if table.deleted {
                    if !table.original.is_empty() {
                        let _ = std::fs::remove_file(&table.path);
                    }
                    deleted += 1;
                } else if table.dirty {
                    let content = tbl::serialize_table(table);
                    if std::fs::write(&table.path, &content).is_ok() {
                        table.original = content;
                        table.dirty = false;
                        count += 1;
                    }
                }
            }
            for constant in &mut group.constants {
                if constant.deleted {
                    if !constant.original.is_empty() {
                        let _ = std::fs::remove_file(&constant.path);
                    }
                    deleted += 1;
                } else if constant.dirty {
                    let content = tbl::serialize_constant(constant);
                    if std::fs::write(&constant.path, &content).is_ok() {
                        constant.original = content;
                        constant.dirty = false;
                        count += 1;
                    }
                }
            }
            group.tables.retain(|t| !t.deleted);
            group.constants.retain(|c| !c.deleted);
            if group.tables.is_empty() && group.constants.is_empty() && !group.is_new && group.dir.is_dir() {
                let _ = std::fs::remove_dir_all(&group.dir);
            }
        }
        self.project.groups.retain(|g| !g.tables.is_empty() || !g.constants.is_empty());
        if count > 0 || deleted > 0 {
            self.log(format!("已保存 {} 个文件, 删除 {} 个", count, deleted));
        } else {
            self.log("无修改需要保存".to_string());
        }
    }

    pub fn reload(&mut self) {
        match crate::core::project::load_project(&self.project.workdir) {
            Ok(p) => {
                self.project = p;
                self.selected = None;
                self.edit_state = EditState::default();
                self.log(format!("重新加载完成，共 {} 个 Group", self.project.groups.len()));
            }
            Err(e) => self.log(format!("加载失败: {}", e)),
        }
    }

    pub fn commit_edit(&mut self, group: &str, name: &str, row: usize, col: usize) {
        let val = self.edit_state.edit_buffer.clone();
        if let Some(g) = self.project.groups.iter_mut().find(|g| g.name == group) {
            if let Some(t) = g.tables.iter_mut().find(|t| t.name == name) {
                if !val.is_empty() {
                    let cols = t.schema.fields.len();
                    while t.records.len() <= row {
                        t.records.push(vec![String::new(); cols]);
                    }
                }
                if let Some(record) = t.records.get_mut(row) {
                    while record.len() <= col { record.push(String::new()); }
                    record[col] = val;
                }
                t.update_dirty();
            }
        }
        self.edit_state.editing = None;
    }

    pub fn commit_header_edit(&mut self, group: &str, name: &str, header_row: usize, col: usize) {
        let mut keyword_err = None;
        if let Some(g) = self.project.groups.iter_mut().find(|g| g.name == group) {
            if let Some(t) = g.tables.iter_mut().find(|t| t.name == name) {
                if let Some(field) = t.schema.fields.get_mut(col) {
                    let val = self.edit_state.edit_buffer.clone();
                    match header_row {
                        0 => field.desc = val,
                        1 => field.export = crate::model::Export::from_str(&val),
                        2 => field.tbl_type = val,
                        3 => {
                            let v = val.trim().to_string();
                            if !is_reserved_keyword(&v) {
                                field.name = v;
                            } else {
                                keyword_err = Some(val);
                            }
                        }
                        _ => {}
                    }
                }
                t.update_dirty();
            }
        }
        if let Some(kw) = keyword_err {
            self.log(format!("字段名 '{}' 是保留关键字，不允许使用", kw));
        }
        self.edit_state.editing = None;
    }

    pub fn insert_row(&mut self, group: &str, name: &str, at: usize) {
        if let Some(g) = self.project.groups.iter_mut().find(|g| g.name == group) {
            if let Some(t) = g.tables.iter_mut().find(|t| t.name == name) {
                let cols = t.schema.fields.len();
                let row = vec![String::new(); cols];
                let at = at.min(t.records.len());
                t.records.insert(at, row);
                t.update_dirty();
            }
        }
    }

    pub fn delete_row(&mut self, group: &str, name: &str, at: usize) {
        if let Some(g) = self.project.groups.iter_mut().find(|g| g.name == group) {
            if let Some(t) = g.tables.iter_mut().find(|t| t.name == name) {
                if at < t.records.len() {
                    t.records.remove(at);
                    t.update_dirty();
                }
            }
        }
    }

    pub fn insert_column(&mut self, group: &str, name: &str, at: usize) {
        if let Some(g) = self.project.groups.iter_mut().find(|g| g.name == group) {
            if let Some(t) = g.tables.iter_mut().find(|t| t.name == name) {
                let at = at.min(t.schema.fields.len());
                t.schema.fields.insert(at, crate::model::FieldDef {
                    name: format!("field{}", t.schema.fields.len()),
                    desc: "新字段".to_string(),
                    tbl_type: "str".to_string(),
                    export: crate::model::Export::ClientServer,
                });
                for record in &mut t.records {
                    record.insert(at.min(record.len()), String::new());
                }
                t.update_dirty();
            }
        }
    }

    pub fn delete_column(&mut self, group: &str, name: &str, at: usize) {
        if let Some(g) = self.project.groups.iter_mut().find(|g| g.name == group) {
            if let Some(t) = g.tables.iter_mut().find(|t| t.name == name) {
                if at < t.schema.fields.len() {
                    t.schema.fields.remove(at);
                    for record in &mut t.records {
                        if at < record.len() { record.remove(at); }
                    }
                    t.update_dirty();
                }
            }
        }
    }

    pub fn set_cell_value(&mut self, group: &str, name: &str, row: usize, col: usize, val: &str, source: &crate::ui::grid_model::GridSource) {
        use crate::ui::grid_model::GridSource;
        match source {
            GridSource::Table => {
                if let Some(g) = self.project.groups.iter_mut().find(|g| g.name == group) {
                    if let Some(t) = g.tables.iter_mut().find(|t| t.name == name) {
                        if !val.is_empty() {
                            let cols = t.schema.fields.len();
                            while t.records.len() <= row { t.records.push(vec![String::new(); cols]); }
                        }
                        if let Some(record) = t.records.get_mut(row) {
                            while record.len() <= col { record.push(String::new()); }
                            record[col] = val.to_string();
                        }
                        t.update_dirty();
                    }
                }
            }
            GridSource::Constant => {
                if let Some(g) = self.project.groups.iter_mut().find(|g| g.name == group) {
                    if let Some(c) = g.constants.iter_mut().find(|c| c.name == name) {
                        if !val.is_empty() {
                            while c.entries.len() <= row {
                                c.entries.push(crate::model::ConstEntry {
                                    name: String::new(), tbl_type: "str".to_string(),
                                    value: String::new(), export: crate::model::Export::ClientServer, desc: String::new(),
                                });
                            }
                        }
                        if let Some(entry) = c.entries.get_mut(row) {
                            match col {
                                0 => entry.name = val.trim().replace(' ', ""),
                                2 => entry.value = val.to_string(),
                                4 => entry.desc = val.to_string(),
                                _ => {}
                            }
                        }
                        c.update_dirty();
                    }
                }
            }
        }
    }

    pub fn paste_data(&mut self, group: &str, name: &str, start_row: usize, start_col: usize, text: &str, source: &crate::ui::grid_model::GridSource) {
        use crate::ui::grid_model::GridSource;
        let lines: Vec<&str> = text.lines().collect();
        if lines.is_empty() { return; }

        if let Some(g) = self.project.groups.iter_mut().find(|g| g.name == group) {
            match source {
                GridSource::Table => {
                    if let Some(t) = g.tables.iter_mut().find(|t| t.name == name) {
                        let cols = t.schema.fields.len();
                        for (i, line) in lines.iter().enumerate() {
                            let row = start_row + i;
                            while t.records.len() <= row {
                                t.records.push(vec![String::new(); cols]);
                            }
                            let cells: Vec<&str> = line.split('\t').collect();
                            for (j, cell) in cells.iter().enumerate() {
                                let col = start_col + j;
                                if col < cols {
                                    let record = &mut t.records[row];
                                    while record.len() <= col { record.push(String::new()); }
                                    record[col] = cell.to_string();
                                }
                            }
                        }
                        t.update_dirty();
                    }
                }
                GridSource::Constant => {
                    if let Some(c) = g.constants.iter_mut().find(|c| c.name == name) {
                        for (i, line) in lines.iter().enumerate() {
                            let row = start_row + i;
                            while c.entries.len() <= row {
                                c.entries.push(crate::model::ConstEntry {
                                    name: String::new(), tbl_type: "str".to_string(),
                                    value: String::new(), export: crate::model::Export::ClientServer, desc: String::new(),
                                });
                            }
                            let cells: Vec<&str> = line.split('\t').collect();
                            for (j, cell) in cells.iter().enumerate() {
                                let col = start_col + j;
                                let entry = &mut c.entries[row];
                                match col {
                                    0 => entry.name = cell.to_string(),
                                    1 => entry.tbl_type = cell.to_string(),
                                    2 => entry.value = cell.to_string(),
                                    3 => entry.export = if cell.is_empty() { crate::model::Export::Unselected } else { crate::model::Export::from_str(cell) },
                                    4 => entry.desc = cell.to_string(),
                                    _ => {}
                                }
                            }
                        }
                        c.update_dirty();
                    }
                }
            }
        }
        self.log(format!("粘贴 {}行 数据", lines.len()));
    }

    pub fn copy_selection(&self, group: &str, name: &str) -> String {
        let table = match self.find_table(group, name) {
            Some(t) => t,
            None => return String::new(),
        };
        match &self.edit_state.selected {
            Selection::Cell(r, c) => {
                table.records.get(*r).and_then(|row| row.get(*c)).cloned().unwrap_or_default()
            }
            Selection::CellRange { start, end } => {
                let (r0, r1) = if start.0 <= end.0 { (start.0, end.0) } else { (end.0, start.0) };
                let (c0, c1) = if start.1 <= end.1 { (start.1, end.1) } else { (end.1, start.1) };
                let mut lines = Vec::new();
                for r in r0..=r1 {
                    let mut cells = Vec::new();
                    for c in c0..=c1 {
                        cells.push(table.records.get(r).and_then(|row| row.get(c)).cloned().unwrap_or_default());
                    }
                    lines.push(cells.join("\t"));
                }
                lines.join("\n")
            }
            Selection::Row(r) => {
                table.records.get(*r).map(|row| row.join("\t")).unwrap_or_default()
            }
            Selection::Rows(s, e) => {
                let mut lines = Vec::new();
                for r in *s..=*e {
                    if let Some(row) = table.records.get(r) {
                        lines.push(row.join("\t"));
                    }
                }
                lines.join("\n")
            }
            Selection::Col(c) => {
                let mut lines = Vec::new();
                for row in &table.records {
                    lines.push(row.get(*c).cloned().unwrap_or_default());
                }
                lines.join("\n")
            }
            Selection::None => String::new(),
        }
    }

    pub fn delete_rows(&mut self, group: &str, name: &str, start: usize, end: usize) {
        if let Some(g) = self.project.groups.iter_mut().find(|g| g.name == group) {
            if let Some(t) = g.tables.iter_mut().find(|t| t.name == name) {
                let end = end.min(t.records.len());
                if start < end {
                    t.records.drain(start..end);
                    t.update_dirty();
                    self.log(format!("已删除 {} 行", end - start));
                }
            }
        }
    }

    pub fn grid_commit(&mut self, group: &str, name: &str, editing: &CellPos, source: &crate::ui::grid_model::GridSource) {
        use crate::ui::grid_model::GridSource;
        let val = self.edit_state.edit_buffer.clone();

        match source {
            GridSource::Table => {
                if let Some(hrow) = editing.header_row {
                    self.commit_header_edit(group, name, hrow, editing.col);
                } else {
                    self.commit_edit(group, name, editing.row, editing.col);
                }
            }
            GridSource::Constant => {
                if editing.header_row.is_some() {
                    self.edit_state.editing = None;
                    return;
                }
                if let Some(g) = self.project.groups.iter_mut().find(|g| g.name == group) {
                    if let Some(c) = g.constants.iter_mut().find(|c| c.name == name) {
                        if !val.is_empty() {
                            while c.entries.len() <= editing.row {
                                c.entries.push(crate::model::ConstEntry {
                                    name: String::new(), tbl_type: "str".to_string(),
                                    value: String::new(), export: crate::model::Export::ClientServer, desc: String::new(),
                                });
                            }
                        }
                        if let Some(entry) = c.entries.get_mut(editing.row) {
                            match editing.col {
                                0 => entry.name = val,
                                1 => entry.tbl_type = val,
                                2 => entry.value = val,
                                3 => entry.export = crate::model::Export::from_str(&val),
                                4 => entry.desc = val,
                                _ => {}
                            }
                        }
                        c.update_dirty();
                    }
                }
                self.edit_state.editing = None;
            }
        }
    }

    pub fn delete_selected(&mut self, group: &str, name: &str, grid: &crate::ui::grid_model::GridData) {
        use crate::ui::grid_model::GridSource;
        match &grid.source {
            GridSource::Table => {
                if let Some(g) = self.project.groups.iter_mut().find(|g| g.name == group) {
                    if let Some(t) = g.tables.iter_mut().find(|t| t.name == name) {
                        clear_cells(&self.edit_state.selected, &mut t.records);
                        t.update_dirty();
                    }
                }
            }
            GridSource::Constant => {
                if let Some(g) = self.project.groups.iter_mut().find(|g| g.name == group) {
                    if let Some(c) = g.constants.iter_mut().find(|c| c.name == name) {
                        let sel = &self.edit_state.selected;
                        let clear_entry_cell = |entry: &mut crate::model::ConstEntry, col: usize| {
                            match col {
                                0 => entry.name.clear(),
                                1 => entry.tbl_type.clear(),
                                2 => entry.value.clear(),
                                3 => entry.export = crate::model::Export::Unselected,
                                4 => entry.desc.clear(),
                                _ => {}
                            }
                        };
                        match sel {
                            Selection::Cell(r, col) => {
                                if let Some(entry) = c.entries.get_mut(*r) { clear_entry_cell(entry, *col); }
                            }
                            Selection::CellRange { start, end } => {
                                let (r0, r1) = (start.0.min(end.0), start.0.max(end.0));
                                let (c0, c1) = (start.1.min(end.1), start.1.max(end.1));
                                for r in r0..=r1 {
                                    if let Some(entry) = c.entries.get_mut(r) {
                                        for col in c0..=c1 { clear_entry_cell(entry, col); }
                                    }
                                }
                            }
                            Selection::Row(r) => {
                                if let Some(entry) = c.entries.get_mut(*r) {
                                    for col in 0..5 { clear_entry_cell(entry, col); }
                                }
                            }
                            Selection::Rows(s, e) => {
                                for r in *s..=*e {
                                    if let Some(entry) = c.entries.get_mut(r) {
                                        for col in 0..5 { clear_entry_cell(entry, col); }
                                    }
                                }
                            }
                            Selection::Col(col) => {
                                for entry in c.entries.iter_mut() { clear_entry_cell(entry, *col); }
                            }
                            Selection::None => {}
                        }
                        c.update_dirty();
                    }
                }
            }
        }
        self.log("已删除选中内容".to_string());
    }

    pub fn generate_test_config(&mut self) {
        let config_dir = self.project.workdir.join(&self.project.config.project.config_dir);

        let hero_dir = config_dir.join("hero");
        let _ = std::fs::create_dir_all(&hero_dir);
        let _ = std::fs::write(hero_dir.join("HeroBase.tbl"),
            "#!tbl v2\n#mode table\n#index id\n#desc 英雄ID|名称|血量|技能组\n#type int|str|int|IntArray\n#export 前后端|前后端|服务器|前后端\n#field id|name|hp|skills\n---\n1001|战士|100|1;2;3\n1002|法师|80|4;5\n1003|弓手|90|6;7;8\n");
        let _ = std::fs::write(hero_dir.join("HeroConst.tbl"),
            "#!tbl v2\n#mode constant\n---\nmax_level|int|60||英雄最大等级\nunlock_cost|int|100||解锁费用\n");

        let global_dir = config_dir.join("global");
        let _ = std::fs::create_dir_all(&global_dir);
        let _ = std::fs::write(global_dir.join("GlobalConst.tbl"),
            "#!tbl v2\n#mode constant\n---\nmax_level|int|100||最大等级\nstart_pos|IntPair|5,10||出生坐标\nserver_name|str|test1||服务器名称\n");

        self.log("已生成测试配置文件".to_string());
        self.reload();
    }

    pub fn clear_all_config(&mut self) {
        let config_dir = self.project.workdir.join(&self.project.config.project.config_dir);
        if config_dir.is_dir() {
            let _ = std::fs::remove_dir_all(&config_dir);
            let _ = std::fs::create_dir_all(&config_dir);
        }
        self.log("已清空所有配置文件".to_string());
        self.reload();
    }
}

impl eframe::App for TblApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.theme_applied {
            self.apply_theme(ctx);
            self.theme_applied = true;
        }
        egui::SidePanel::left("tree_panel")
            .default_width(200.0)
            .min_width(80.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui::tree::render(ui, self);
            });

        egui::TopBottomPanel::top("toolbar_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("生成测试数据").clicked() {
                    self.generate_test_config();
                }
                if ui.button("清空配置").clicked() {
                    self.clear_all_config();
                }
                ui.separator();
                if ui.button("用Excel打开").clicked() {
                    self.log("Excel 编辑功能待实现".to_string());
                }
                if ui.button("保存").clicked() {
                    self.save_all();
                }
                if ui.button("重新加载").clicked() {
                    self.reload();
                }
            });
        });

        egui::TopBottomPanel::bottom("log_panel")
            .default_height(120.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.label("日志");
                ui.separator();
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for line in &self.logs {
                            ui.label(line);
                        }
                    });
            });

        egui::TopBottomPanel::bottom("status_panel")
            .exact_height(20.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let text = ui::grid::format_selection(&self.edit_state.selected, self.edit_state.hover_cell);
                    ui.label(egui::RichText::new(text).size(11.0));
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui::detail::render(ui, self);
        });

        self.show_input_dialog(ctx);
    }
}

impl TblApp {
    fn show_input_dialog(&mut self, ctx: &egui::Context) {
        let action = match &self.pending_action {
            Some(a) => a.clone(),
            None => return,
        };

        // 删除操作直接确认，不需要输入名称
        match &action {
            PendingAction::DeleteGroup { group } => {
                let group = group.clone();
                let mut open = true;
                egui::Window::new("确认删除")
                    .collapsible(false).resizable(false).open(&mut open)
                    .show(ctx, |ui| {
                        ui.label(format!("确定删除 Group \"{}\" 及其所有内容？", group));
                        ui.horizontal(|ui| {
                            if ui.button("确定").clicked() {
                                self.delete_group(&group);
                                self.pending_action = None;
                            }
                            if ui.button("取消").clicked() {
                                self.pending_action = None;
                            }
                        });
                    });
                if !open { self.pending_action = None; }
                return;
            }
            PendingAction::DeleteNode { group, name } => {
                let (group, name) = (group.clone(), name.clone());
                let mut open = true;
                egui::Window::new("确认删除")
                    .collapsible(false).resizable(false).open(&mut open)
                    .show(ctx, |ui| {
                        ui.label(format!("确定删除 \"{}/{}\"？", group, name));
                        ui.horizontal(|ui| {
                            if ui.button("确定").clicked() {
                                self.delete_node(&group, &name);
                                self.pending_action = None;
                            }
                            if ui.button("取消").clicked() {
                                self.pending_action = None;
                            }
                        });
                    });
                if !open { self.pending_action = None; }
                return;
            }
            PendingAction::CopyNode { group, name, is_table } => {
                let (group, name, is_table) = (group.clone(), name.clone(), *is_table);
                self.copy_node(&group, &name, is_table);
                self.pending_action = None;
                return;
            }
            _ => {}
        }

        let title = match &action {
            PendingAction::NewGroup => "新建 Group",
            PendingAction::NewTable { .. } => "新建 Table",
            PendingAction::NewConstant { .. } => "新建 Constant",
            PendingAction::RenameGroup { .. } => "重命名 Group",
            PendingAction::RenameNode { .. } => "重命名",
            _ => return,
        };

        let mut open = true;
        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("名称:");
                    ui.text_edit_singleline(&mut self.input_name);
                });
                ui.horizontal(|ui| {
                    if ui.button("确定").clicked() && !self.input_name.is_empty() {
                        self.execute_action(&action);
                        self.pending_action = None;
                        self.input_name.clear();
                    }
                    if ui.button("取消").clicked() {
                        self.pending_action = None;
                        self.input_name.clear();
                    }
                });
            });

        if !open {
            self.pending_action = None;
            self.input_name.clear();
        }
    }

    fn delete_group(&mut self, group_name: &str) {
        if let Some(g) = self.project.groups.iter().find(|g| g.name == group_name) {
            if g.is_new {
                self.project.groups.retain(|g| g.name != group_name);
                self.log(format!("已移除新建 Group: {}", group_name));
            } else {
                if let Some(g) = self.project.groups.iter_mut().find(|g| g.name == group_name) {
                    for t in &mut g.tables { t.deleted = true; }
                    for c in &mut g.constants { c.deleted = true; }
                }
                self.log(format!("已标记删除 Group: {}", group_name));
            }
        }
        self.selected = None;
    }

    fn delete_node(&mut self, group_name: &str, node_name: &str) {
        if let Some(g) = self.project.groups.iter_mut().find(|g| g.name == group_name) {
            if let Some(t) = g.tables.iter_mut().find(|t| t.name == node_name) {
                t.deleted = true;
            }
            if let Some(c) = g.constants.iter_mut().find(|c| c.name == node_name) {
                c.deleted = true;
            }
        }
        self.selected = None;
        self.log(format!("已标记删除: {}/{}", group_name, node_name));
    }

    fn copy_node(&mut self, group_name: &str, node_name: &str, is_table: bool) {
        if let Some(g) = self.project.groups.iter_mut().find(|g| g.name == group_name) {
            let new_name = format!("{}_copy", node_name);
            let dst = g.dir.join(format!("{}.tbl", new_name));
            if is_table {
                if let Some(t) = g.tables.iter().find(|t| t.name == node_name).cloned() {
                    let mut copy = t;
                    copy.name = new_name.clone();
                    copy.path = dst;
                    copy.dirty = true;
                    copy.original = String::new();
                    g.tables.push(copy);
                }
            } else {
                if let Some(c) = g.constants.iter().find(|c| c.name == node_name).cloned() {
                    let mut copy = c;
                    copy.name = new_name.clone();
                    copy.path = dst;
                    copy.dirty = true;
                    copy.original = String::new();
                    g.constants.push(copy);
                }
            }
            self.log(format!("已复制: {}/{} → {}", group_name, node_name, new_name));
        }
    }

    fn execute_action(&mut self, action: &PendingAction) {
        let config_dir = self.project.workdir.join(&self.project.config.project.config_dir);

        match action {
            PendingAction::NewGroup => {
                let dir = config_dir.join(&self.input_name);
                self.tree_expanded.insert(self.input_name.clone());
                self.project.groups.push(Group {
                    name: self.input_name.clone(),
                    dir,
                    tables: Vec::new(),
                    constants: Vec::new(),
                    is_new: true,
                });
                self.log(format!("新建 Group: {}", self.input_name));
            }
            PendingAction::NewTable { group } => {
                if let Some(g) = self.project.groups.iter_mut().find(|g| g.name == *group) {
                    let path = g.dir.join(format!("{}.tbl", &self.input_name));
                    g.tables.push(Table {
                        name: self.input_name.clone(),
                        path: path.clone(),
                        schema: TableSchema {
                            fields: vec![FieldDef {
                                name: "id".to_string(),
                                desc: "ID".to_string(),
                                tbl_type: "int".to_string(),
                                export: Export::ClientServer,
                            }],
                            index: "id".to_string(),
                        },
                        records: Vec::new(),
                        dirty: true,
                        deleted: false,
                        original: String::new(),
                    });
                    self.log(format!("新建 Table: {}/{}", group, self.input_name));
                }
            }
            PendingAction::NewConstant { group } => {
                if let Some(g) = self.project.groups.iter_mut().find(|g| g.name == *group) {
                    let path = g.dir.join(format!("{}.tbl", &self.input_name));
                    g.constants.push(Constant {
                        name: self.input_name.clone(),
                        path: path.clone(),
                        entries: Vec::new(),
                        dirty: true,
                        deleted: false,
                        original: String::new(),
                    });
                    self.log(format!("新建 Constant: {}/{}", group, self.input_name));
                }
            }
            PendingAction::RenameGroup { old_name } => {
                let old_dir = config_dir.join(old_name);
                let new_dir = config_dir.join(&self.input_name);
                let _ = std::fs::rename(&old_dir, &new_dir);
                if let Some(g) = self.project.groups.iter_mut().find(|g| g.name == *old_name) {
                    g.name = self.input_name.clone();
                    g.dir = new_dir;
                }
                self.log(format!("重命名 Group: {} → {}", old_name, self.input_name));
            }
            PendingAction::RenameNode { group, old_name } => {
                if let Some(g) = self.project.groups.iter_mut().find(|g| g.name == *group) {
                    let old_path = g.dir.join(format!("{}.tbl", old_name));
                    let new_path = g.dir.join(format!("{}.tbl", &self.input_name));
                    let _ = std::fs::rename(&old_path, &new_path);
                    if let Some(t) = g.tables.iter_mut().find(|t| t.name == *old_name) {
                        t.name = self.input_name.clone();
                        t.path = new_path.clone();
                    }
                    if let Some(c) = g.constants.iter_mut().find(|c| c.name == *old_name) {
                        c.name = self.input_name.clone();
                        c.path = new_path;
                    }
                }
                self.log(format!("重命名: {}/{} → {}", group, old_name, self.input_name));
            }
            _ => {}
        }
    }
}

pub fn is_reserved_keyword(name: &str) -> bool {
    const JAVA_KEYWORDS: &[&str] = &[
        "abstract","assert","boolean","break","byte","case","catch","char","class",
        "const","continue","default","do","double","else","enum","extends","final",
        "finally","float","for","goto","if","implements","import","instanceof","int",
        "interface","long","native","new","package","private","protected","public",
        "return","short","static","strictfp","super","switch","synchronized","this",
        "throw","throws","transient","try","void","volatile","while",
    ];
    const LUA_KEYWORDS: &[&str] = &[
        "and","break","do","else","elseif","end","false","for","function","goto",
        "if","in","local","nil","not","or","repeat","return","then","true","until","while",
    ];
    JAVA_KEYWORDS.contains(&name) || LUA_KEYWORDS.contains(&name)
}

fn clear_cells(sel: &Selection, data: &mut Vec<Vec<String>>) {
    match sel {
        Selection::Cell(r, c) => {
            if let Some(row) = data.get_mut(*r) { if let Some(cell) = row.get_mut(*c) { cell.clear(); } }
        }
        Selection::CellRange { start, end } => {
            let (r0, r1) = (start.0.min(end.0), start.0.max(end.0));
            let (c0, c1) = (start.1.min(end.1), start.1.max(end.1));
            for r in r0..=r1 { if let Some(row) = data.get_mut(r) { for c in c0..=c1 { if let Some(cell) = row.get_mut(c) { cell.clear(); } } } }
        }
        Selection::Row(r) => { if let Some(row) = data.get_mut(*r) { for cell in row.iter_mut() { cell.clear(); } } }
        Selection::Rows(s, e) => { for r in *s..=*e { if let Some(row) = data.get_mut(r) { for cell in row.iter_mut() { cell.clear(); } } } }
        Selection::Col(c) => { for row in data.iter_mut() { if let Some(cell) = row.get_mut(*c) { cell.clear(); } } }
        Selection::None => {}
    }
}

fn is_valid_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn is_java_keyword(s: &str) -> bool {
    matches!(s,
        "abstract" | "assert" | "boolean" | "break" | "byte" | "case" | "catch" |
        "char" | "class" | "const" | "continue" | "default" | "do" | "double" |
        "else" | "enum" | "extends" | "final" | "finally" | "float" | "for" |
        "goto" | "if" | "implements" | "import" | "instanceof" | "int" |
        "interface" | "long" | "native" | "new" | "package" | "private" |
        "protected" | "public" | "return" | "short" | "static" | "strictfp" |
        "super" | "switch" | "synchronized" | "this" | "throw" | "throws" |
        "transient" | "try" | "void" | "volatile" | "while" |
        "true" | "false" | "null"
    )
}

fn is_lua_keyword(s: &str) -> bool {
    matches!(s,
        "and" | "break" | "do" | "else" | "elseif" | "end" | "false" | "for" |
        "function" | "goto" | "if" | "in" | "local" | "nil" | "not" | "or" |
        "repeat" | "return" | "then" | "true" | "until" | "while"
    )
}
