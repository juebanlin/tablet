use std::collections::HashSet;
use crate::model::*;
use crate::validate::*;

pub struct ProjectEngine {
    pub project: Project,
    pub validation_errors: HashSet<(String, String, usize, usize)>,
    pub logs: Vec<String>,
}

#[derive(Clone, Debug)]
pub enum ProjectAction {
    NewGroup { name: String },
    NewTable { group: String, name: String },
    NewConstant { group: String, name: String },
    RenameGroup { old_name: String, new_name: String },
    RenameNode { group: String, old_name: String, new_name: String },
}

impl ProjectEngine {
    pub fn new(project: Project) -> Self {
        Self {
            project,
            validation_errors: HashSet::new(),
            logs: Vec::new(),
        }
    }

    pub fn log(&mut self, msg: String) {
        let now = chrono::Local::now().format("%H:%M:%S").to_string();
        self.logs.push(format!("{} {}", now, msg));
    }

    pub fn logs(&self) -> &[String] {
        &self.logs
    }

    pub fn find_table(&self, group: &str, name: &str) -> Option<&Table> {
        self.project.groups.iter()
            .find(|g| g.name == group)?
            .tables.iter()
            .find(|t| t.name == name)
    }

    pub fn find_table_mut(&mut self, group: &str, name: &str) -> Option<&mut Table> {
        self.project.groups.iter_mut()
            .find(|g| g.name == group)?
            .tables.iter_mut()
            .find(|t| t.name == name)
    }

    pub fn find_constant(&self, group: &str, name: &str) -> Option<&Constant> {
        self.project.groups.iter()
            .find(|g| g.name == group)?
            .constants.iter()
            .find(|c| c.name == name)
    }

    pub fn find_constant_mut(&mut self, group: &str, name: &str) -> Option<&mut Constant> {
        self.project.groups.iter_mut()
            .find(|g| g.name == group)?
            .constants.iter_mut()
            .find(|c| c.name == name)
    }

    pub fn mark_table_dirty(&mut self, group: &str, name: &str) {
        if let Some(t) = self.find_table_mut(group, name) {
            t.update_dirty();
        }
    }

    pub fn mark_constant_dirty(&mut self, group: &str, name: &str) {
        if let Some(c) = self.find_constant_mut(group, name) {
            c.update_dirty();
        }
    }

    pub fn set_table_cell(&mut self, group: &str, name: &str, row: usize, col: usize, val: &str) {
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

    pub fn set_constant_cell(&mut self, group: &str, name: &str, row: usize, col: usize, val: &str) {
        if let Some(g) = self.project.groups.iter_mut().find(|g| g.name == group) {
            if let Some(c) = g.constants.iter_mut().find(|c| c.name == name) {
                if !val.is_empty() {
                    while c.entries.len() <= row {
                        c.entries.push(ConstEntry {
                            name: String::new(), tbl_type: "str".to_string(),
                            value: String::new(), export: Export::ClientServer, desc: String::new(),
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

    pub fn commit_table_cell(&mut self, group: &str, name: &str, row: usize, col: usize, val: String) {
        if let Some(g) = self.project.groups.iter_mut().find(|g| g.name == group) {
            if let Some(t) = g.tables.iter_mut().find(|t| t.name == name) {
                if !val.is_empty() {
                    let cols = t.schema.fields.len();
                    while t.records.len() <= row { t.records.push(vec![String::new(); cols]); }
                }
                if let Some(record) = t.records.get_mut(row) {
                    while record.len() <= col { record.push(String::new()); }
                    record[col] = val;
                }
                t.update_dirty();
            }
        }
    }

    pub fn commit_constant_cell(&mut self, group: &str, name: &str, row: usize, col: usize, val: String) {
        if let Some(g) = self.project.groups.iter_mut().find(|g| g.name == group) {
            if let Some(c) = g.constants.iter_mut().find(|c| c.name == name) {
                if !val.is_empty() {
                    while c.entries.len() <= row {
                        c.entries.push(ConstEntry {
                            name: String::new(), tbl_type: "str".to_string(),
                            value: String::new(), export: Export::ClientServer, desc: String::new(),
                        });
                    }
                }
                if let Some(entry) = c.entries.get_mut(row) {
                    match col {
                        0 => entry.name = val,
                        1 => entry.tbl_type = val,
                        2 => entry.value = val,
                        3 => entry.export = Export::from_str(&val),
                        4 => entry.desc = val,
                        _ => {}
                    }
                }
                c.update_dirty();
            }
        }
    }

    pub fn commit_header_edit(&mut self, group: &str, name: &str, header_row: usize, col: usize, val: String) {
        let mut keyword_err = None;
        if let Some(g) = self.project.groups.iter_mut().find(|g| g.name == group) {
            if let Some(t) = g.tables.iter_mut().find(|t| t.name == name) {
                if let Some(field) = t.schema.fields.get_mut(col) {
                    match header_row {
                        0 => field.desc = val,
                        1 => field.export = Export::from_str(&val),
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

    pub fn insert_column(&mut self, group: &str, name: &str, at: usize) {
        if let Some(g) = self.project.groups.iter_mut().find(|g| g.name == group) {
            if let Some(t) = g.tables.iter_mut().find(|t| t.name == name) {
                let at = at.min(t.schema.fields.len());
                t.schema.fields.insert(at, FieldDef {
                    name: format!("field{}", t.schema.fields.len()),
                    desc: "新字段".to_string(),
                    tbl_type: "str".to_string(),
                    export: Export::ClientServer,
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

    pub fn paste_table_data(&mut self, group: &str, name: &str, start_row: usize, start_col: usize, text: &str) {
        let lines: Vec<&str> = text.lines().collect();
        if lines.is_empty() { return; }
        if let Some(g) = self.project.groups.iter_mut().find(|g| g.name == group) {
            if let Some(t) = g.tables.iter_mut().find(|t| t.name == name) {
                let cols = t.schema.fields.len();
                for (i, line) in lines.iter().enumerate() {
                    let row = start_row + i;
                    while t.records.len() <= row { t.records.push(vec![String::new(); cols]); }
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
        self.log(format!("粘贴 {}行 数据", lines.len()));
    }

    pub fn paste_constant_data(&mut self, group: &str, name: &str, start_row: usize, start_col: usize, text: &str) {
        let lines: Vec<&str> = text.lines().collect();
        if lines.is_empty() { return; }
        if let Some(g) = self.project.groups.iter_mut().find(|g| g.name == group) {
            if let Some(c) = g.constants.iter_mut().find(|c| c.name == name) {
                for (i, line) in lines.iter().enumerate() {
                    let row = start_row + i;
                    while c.entries.len() <= row {
                        c.entries.push(ConstEntry {
                            name: String::new(), tbl_type: "str".to_string(),
                            value: String::new(), export: Export::ClientServer, desc: String::new(),
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
                            3 => entry.export = if cell.is_empty() { Export::Unselected } else { Export::from_str(cell) },
                            4 => entry.desc = cell.to_string(),
                            _ => {}
                        }
                    }
                }
                c.update_dirty();
            }
        }
        self.log(format!("粘贴 {}行 数据", lines.len()));
    }

    pub fn clear_table_cells(&mut self, group: &str, name: &str, cells: &[(usize, usize)]) {
        if let Some(g) = self.project.groups.iter_mut().find(|g| g.name == group) {
            if let Some(t) = g.tables.iter_mut().find(|t| t.name == name) {
                for &(row, col) in cells {
                    if let Some(record) = t.records.get_mut(row) {
                        if let Some(cell) = record.get_mut(col) { cell.clear(); }
                    }
                }
                t.update_dirty();
            }
        }
    }

    pub fn clear_constant_cells(&mut self, group: &str, name: &str, cells: &[(usize, usize)]) {
        if let Some(g) = self.project.groups.iter_mut().find(|g| g.name == group) {
            if let Some(c) = g.constants.iter_mut().find(|c| c.name == name) {
                for &(row, col) in cells {
                    if let Some(entry) = c.entries.get_mut(row) {
                        match col {
                            0 => entry.name.clear(),
                            1 => entry.tbl_type.clear(),
                            2 => entry.value.clear(),
                            3 => entry.export = Export::Unselected,
                            4 => entry.desc.clear(),
                            _ => {}
                        }
                    }
                }
                c.update_dirty();
            }
        }
    }

    // --- PLACEHOLDER_SAVE ---

    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        let sep = &self.project.config.separators;
        for group in &self.project.groups {
            for table in &group.tables {
                if table.deleted { continue; }
                let index_col = table.schema.fields.iter().position(|f| f.name == table.schema.index);
                let mut seen_ids = std::collections::HashSet::new();
                for row in 0..table.records.len() {
                    for (col, msg) in validate_table_row(table, row, sep) {
                        let val = table.records[row].get(col).map(|s| s.as_str()).unwrap_or("");
                        let pos = format!("{}{}", col_letter(col), row + 1);
                        errors.push(format!("[验证] {}/{} {}: \"{}\" {}", group.name, table.name, pos, val, msg));
                    }
                    if let Some(idx) = index_col {
                        let id = table.records[row].get(idx).map(|s| s.as_str()).unwrap_or("");
                        if !id.is_empty() && !seen_ids.insert(id.to_string()) {
                            let pos = format!("{}{}", col_letter(idx), row + 1);
                            errors.push(format!("[验证] {}/{} {}: ID \"{}\" 重复", group.name, table.name, pos, id));
                        }
                    }
                }
            }
            for constant in &group.constants {
                if constant.deleted { continue; }
                let mut seen_names = std::collections::HashSet::new();
                for row in 0..constant.entries.len() {
                    for (col, msg) in validate_constant_row(constant, row, sep) {
                        let val = match col { 0 => &constant.entries[row].name, 2 => &constant.entries[row].value, _ => "" };
                        let pos = format!("{}{}", col_letter(col), row + 1);
                        errors.push(format!("[验证] {}/{} {}: \"{}\" {}", group.name, constant.name, pos, val, msg));
                    }
                    let n = &constant.entries[row].name;
                    if !n.is_empty() && !seen_names.insert(n.clone()) {
                        let pos = format!("A{}", row + 1);
                        errors.push(format!("[验证] {}/{} {}: name \"{}\" 重复", group.name, constant.name, pos, n));
                    }
                }
            }
        }
        errors
    }

    pub fn revalidate(&mut self, group: &str, name: &str) {
        self.validation_errors.retain(|(g, n, _, _)| g != group || n != name);
        let sep = self.project.config.separators.clone();
        if let Some(g) = self.project.groups.iter().find(|g| g.name == group) {
            if let Some(table) = g.tables.iter().find(|t| t.name == name) {
                let mut seen_ids = std::collections::HashSet::new();
                let index_col = table.schema.fields.iter().position(|f| f.name == table.schema.index);
                for row in 0..table.records.len() {
                    for (col, _msg) in validate_table_row(table, row, &sep) {
                        self.validation_errors.insert((group.to_string(), name.to_string(), row, col));
                    }
                    if let Some(idx) = index_col {
                        let id = table.records[row].get(idx).map(|s| s.as_str()).unwrap_or("");
                        if !id.is_empty() && !seen_ids.insert(id.to_string()) {
                            self.validation_errors.insert((group.to_string(), name.to_string(), row, idx));
                        }
                    }
                }
            }
            if let Some(constant) = g.constants.iter().find(|c| c.name == name) {
                let mut seen_names = std::collections::HashSet::new();
                for row in 0..constant.entries.len() {
                    for (col, _msg) in validate_constant_row(constant, row, &sep) {
                        self.validation_errors.insert((group.to_string(), name.to_string(), row, col));
                    }
                    let n = &constant.entries[row].name;
                    if !n.is_empty() && !seen_names.insert(n.clone()) {
                        self.validation_errors.insert((group.to_string(), name.to_string(), row, 0));
                    }
                }
            }
        }
    }

    pub fn revalidate_all(&mut self) {
        self.validation_errors.clear();
        let groups: Vec<_> = self.project.groups.iter()
            .map(|g| (g.name.clone(), g.tables.iter().map(|t| t.name.clone()).collect::<Vec<_>>(), g.constants.iter().map(|c| c.name.clone()).collect::<Vec<_>>()))
            .collect();
        for (gname, tables, constants) in &groups {
            for tname in tables { self.revalidate(gname, tname); }
            for cname in constants { self.revalidate(gname, cname); }
        }
    }

    // --- PLACEHOLDER_SAVE2 ---

    pub fn save_all(&mut self) {
        self.revalidate_all();
        if !self.validation_errors.is_empty() {
            let errors = self.validate();
            for e in &errors { self.logs.push(e.clone()); }
            self.logs.push(format!("[保存失败] 共 {} 个验证错误", self.validation_errors.len()));
            return;
        }

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
                    let content = crate::tbl::serialize_table(table);
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
                    let content = crate::tbl::serialize_constant(constant);
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
        match crate::project::load_project(&self.project.workdir) {
            Ok(p) => {
                self.project = p;
                self.log(format!("重新加载完成，共 {} 个 Group", self.project.groups.len()));
            }
            Err(e) => self.log(format!("加载失败: {}", e)),
        }
    }

    pub fn delete_group(&mut self, group_name: &str) {
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
    }

    pub fn delete_node(&mut self, group_name: &str, node_name: &str) {
        if let Some(g) = self.project.groups.iter_mut().find(|g| g.name == group_name) {
            if let Some(t) = g.tables.iter_mut().find(|t| t.name == node_name) {
                t.deleted = true;
            }
            if let Some(c) = g.constants.iter_mut().find(|c| c.name == node_name) {
                c.deleted = true;
            }
        }
        self.log(format!("已标记删除: {}/{}", group_name, node_name));
    }

    pub fn copy_node(&mut self, group_name: &str, node_name: &str, is_table: bool) {
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

    // --- PLACEHOLDER_ACTIONS ---

    pub fn execute_action(&mut self, action: &ProjectAction) {
        let config_dir = self.project.workdir.join(&self.project.config.project.config_dir);
        match action {
            ProjectAction::NewGroup { name } => {
                let dir = config_dir.join(name);
                self.project.groups.push(Group {
                    name: name.clone(),
                    dir,
                    tables: Vec::new(),
                    constants: Vec::new(),
                    is_new: true,
                });
                self.log(format!("新建 Group: {}", name));
            }
            ProjectAction::NewTable { group, name } => {
                if let Some(g) = self.project.groups.iter_mut().find(|g| g.name == *group) {
                    let path = g.dir.join(format!("{}.tbl", name));
                    g.tables.push(Table {
                        name: name.clone(),
                        path,
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
                    self.log(format!("新建 Table: {}/{}", group, name));
                }
            }
            ProjectAction::NewConstant { group, name } => {
                if let Some(g) = self.project.groups.iter_mut().find(|g| g.name == *group) {
                    let path = g.dir.join(format!("{}.tbl", name));
                    g.constants.push(Constant {
                        name: name.clone(),
                        path,
                        entries: Vec::new(),
                        dirty: true,
                        deleted: false,
                        original: String::new(),
                    });
                    self.log(format!("新建 Constant: {}/{}", group, name));
                }
            }
            ProjectAction::RenameGroup { old_name, new_name } => {
                let old_dir = config_dir.join(old_name);
                let new_dir = config_dir.join(new_name);
                let _ = std::fs::rename(&old_dir, &new_dir);
                if let Some(g) = self.project.groups.iter_mut().find(|g| g.name == *old_name) {
                    g.name = new_name.clone();
                    g.dir = new_dir;
                }
                self.log(format!("重命名 Group: {} → {}", old_name, new_name));
            }
            ProjectAction::RenameNode { group, old_name, new_name } => {
                if let Some(g) = self.project.groups.iter_mut().find(|g| g.name == *group) {
                    let old_path = g.dir.join(format!("{}.tbl", old_name));
                    let new_path = g.dir.join(format!("{}.tbl", new_name));
                    let _ = std::fs::rename(&old_path, &new_path);
                    if let Some(t) = g.tables.iter_mut().find(|t| t.name == *old_name) {
                        t.name = new_name.clone();
                        t.path = new_path.clone();
                    }
                    if let Some(c) = g.constants.iter_mut().find(|c| c.name == *old_name) {
                        c.name = new_name.clone();
                        c.path = new_path;
                    }
                }
                self.log(format!("重命名: {}/{} → {}", group, old_name, new_name));
            }
        }
    }

    pub fn generate_test_config(&mut self) {
        let config_dir = self.project.workdir.join(&self.project.config.project.config_dir);
        let opts = crate::test_util::TestGenOptions::full();
        crate::test_util::generate_test_config(&config_dir, &opts);
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

    pub fn validate_group_name(&self, name: &str) -> Option<String> {
        if name.is_empty() { return Some("名称不能为空".to_string()); }
        if !is_valid_group_name(name) { return Some("组名只能包含中英文数字下划线".to_string()); }
        let lower = name.to_lowercase();
        if self.project.groups.iter().any(|g| g.name.to_lowercase() == lower) {
            return Some("组名重复（忽略大小写）".to_string());
        }
        None
    }

    pub fn validate_group_name_rename(&self, name: &str, old_name: &str) -> Option<String> {
        if name.is_empty() { return Some("名称不能为空".to_string()); }
        if !is_valid_group_name(name) { return Some("组名只能包含中英文数字下划线".to_string()); }
        let lower = name.to_lowercase();
        if self.project.groups.iter().any(|g| g.name.to_lowercase() == lower && g.name != old_name) {
            return Some("组名重复（忽略大小写）".to_string());
        }
        None
    }

    pub fn validate_node_name(&self, name: &str) -> Option<String> {
        if name.is_empty() { return Some("名称不能为空".to_string()); }
        if !is_valid_node_name(name) { return Some("配置项名必须符合Java类名规则(大写开头,英文数字下划线)".to_string()); }
        let lower = name.to_lowercase();
        for g in &self.project.groups {
            for t in &g.tables {
                if !t.deleted && t.name.to_lowercase() == lower { return Some("配置项名重复（忽略大小写）".to_string()); }
            }
            for c in &g.constants {
                if !c.deleted && c.name.to_lowercase() == lower { return Some("配置项名重复（忽略大小写）".to_string()); }
            }
        }
        None
    }

    pub fn validate_node_name_rename(&self, name: &str, old_name: &str) -> Option<String> {
        if name.is_empty() { return Some("名称不能为空".to_string()); }
        if !is_valid_node_name(name) { return Some("配置项名必须符合Java类名规则(大写开头,英文数字下划线)".to_string()); }
        let lower = name.to_lowercase();
        for g in &self.project.groups {
            for t in &g.tables {
                if !t.deleted && t.name.to_lowercase() == lower && t.name != old_name { return Some("配置项名重复（忽略大小写）".to_string()); }
            }
            for c in &g.constants {
                if !c.deleted && c.name.to_lowercase() == lower && c.name != old_name { return Some("配置项名重复（忽略大小写）".to_string()); }
            }
        }
        None
    }

    pub fn export_json(&mut self) -> anyhow::Result<Vec<String>> {
        let result = crate::export::export_all_json(&self.project)?;
        self.log(format!("导出 JSON 数据文件 {} 个", result.len()));
        Ok(result)
    }

    pub fn export_xml(&mut self) -> anyhow::Result<Vec<String>> {
        let result = crate::export::export_all_xml(&self.project)?;
        self.log(format!("导出 XML 数据文件 {} 个", result.len()));
        Ok(result)
    }

    pub fn export_java(&mut self) -> anyhow::Result<Vec<String>> {
        let result = crate::export::export_all_java(&self.project)?;
        self.log(format!("导出 Java 模板类 {} 个", result.len()));
        Ok(result)
    }

    pub fn export_lua(&mut self) -> anyhow::Result<Vec<String>> {
        let result = crate::export::export_all_lua(&self.project)?;
        self.log(format!("导出 Lua 文件 {} 个", result.len()));
        Ok(result)
    }
}
