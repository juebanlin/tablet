use std::collections::HashSet;
use std::path::PathBuf;
use crate::model::*;
use crate::validate::*;

/// 把一条 ValidationError 格式化为日志行（统一 B 格式：`位置:[内容] -> 原因`）：
/// - Table 表头：`[验证] group/node 表头第N行X列:[field] -> message`
///   N = 1 desc / 2 export / 3 type / 4 field（与 UI 表头从上到下一致）
/// - Constant/Enum 表头：`[验证] group/node 表头X列:[field] -> message`
/// - 数据行：`[验证] group/node X<row>:[value] -> message`，value 过长截断为 `xxx...`
fn format_validation_log(group: &str, node: &str, err: &ValidationError) -> String {
    if err.is_schema() {
        let col = col_letter(err.col);
        let field = if err.field.is_empty() { "?".to_string() } else { err.field.clone() };
        match err.header_row {
            Some(hr) => {
                let n = hr as usize;
                format!("[验证] {}/{} 表头第{}行{}列:[{}] -> {}", group, node, n, col, field, err.message)
            }
            None => {
                format!("[验证] {}/{} 表头{}列:[{}] -> {}", group, node, col, field, err.message)
            }
        }
    } else {
        let pos = format!("{}{}", col_letter(err.col), err.row + 1);
        let display = truncate_display(&err.value, 16);
        format!("[验证] {}/{} {}:[{}] -> {}", group, node, pos, display, err.message)
    }
}

/// 截断显示用值：超过 max 个 char 时保留前 max 个 + `...`，否则原样返回。
fn truncate_display(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max { return s.to_string(); }
    let mut out: String = s.chars().take(max).collect();
    out.push_str("...");
    out
}

/// 在 existing 集合里找一个不冲突的名字：先试 `<base>_copy`，再 `_copy2`、`_copy3` …
/// 直到 1000 次还没找到就返回 `<base>_copy_<timestamp_ms>` 兜底。
fn dedupe_name<'a, I: IntoIterator<Item = &'a str>>(base: &str, existing: I) -> String {
    let set: std::collections::HashSet<&str> = existing.into_iter().collect();
    let first = format!("{}_copy", base);
    if !set.contains(first.as_str()) { return first; }
    for n in 2..1000 {
        let candidate = format!("{}_copy{}", base, n);
        if !set.contains(candidate.as_str()) { return candidate; }
    }
    format!("{}_copy_{}", base, chrono::Local::now().timestamp_millis())
}

/// 同步 schema.sections 与 groups 的结构（仅结构骨架，不动 schema.meta）。
/// 任何 NewGroup / RenameGroup / DeleteGroup / NewTable / RenameNode / DeleteNode
/// / paste_node_to / paste_group_to 后都要调一次，保证 save 时 project.tblschema
/// 与磁盘 .tbl 一致。
fn sync_schema_from_groups(project: &mut Project) {
    // sync 永远不带 preset：project.tblschema 是「结构骨架」，行数据归项目里的 .tbl
    let mut s = crate::tblschema::schema_from_project(&project.groups, false);
    s.meta = project.schema.meta.clone();
    project.schema = s;
    project.schema_dirty = true;
}

/// 保存单个 Project 的所有 dirty / deleted 节点；返回 (保存数, 删除数)。
/// 抽出来给 save_all（active 一份）和 save_all_projects（遍历）共用。
///
/// 项目级落盘（root 目录创建 / .tblschema 写入）算 1 次保存动作，
/// 让"空项目（仅 meta，无 group）"首次保存的日志为"已保存 1 个文件"而不是"无修改"。
fn save_project_files(project: &mut Project) -> (usize, usize) {
    let mut count = 0;
    let mut deleted = 0;
    // 第一步：克隆出来但未落盘的项目，先建 root 目录 + 让 schema 一并落地
    if project.root_pending_create {
        if let Err(e) = std::fs::create_dir_all(&project.project_root) {
            eprintln!("warn: create project root failed: {} ({})", project.project_root.display(), e);
            return (0, 0);
        }
        project.root_pending_create = false;
        project.schema_dirty = true;
    }
    if project.schema_dirty {
        let txt = crate::tblschema::serialize_tblschema(&project.schema);
        let path = project.project_root.join(crate::project::PROJECT_SCHEMA_FILE);
        if std::fs::write(&path, txt).is_ok() {
            project.schema_dirty = false;
            count += 1;
        }
    }
    for group in &mut project.groups {
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
        for enum_def in &mut group.enums {
            if enum_def.deleted {
                if !enum_def.original.is_empty() {
                    let _ = std::fs::remove_file(&enum_def.path);
                }
                deleted += 1;
            } else if enum_def.dirty {
                let content = crate::tbl::serialize_enum(enum_def);
                if std::fs::write(&enum_def.path, &content).is_ok() {
                    enum_def.original = content;
                    enum_def.dirty = false;
                    count += 1;
                }
            }
        }
        group.tables.retain(|t| !t.deleted);
        group.constants.retain(|c| !c.deleted);
        group.enums.retain(|e| !e.deleted);
        if group.tables.is_empty() && group.constants.is_empty() && group.enums.is_empty() && !group.is_new && group.dir.is_dir() {
            let _ = std::fs::remove_dir_all(&group.dir);
        }
    }
    project.groups.retain(|g| !g.tables.is_empty() || !g.constants.is_empty() || !g.enums.is_empty());
    (count, deleted)
}

pub struct ProjectEngine {
    /// 顶层 workdir（@04.2.0 多 Project 同时管理）。所有 project 共用。
    pub workdir: PathBuf,
    /// 扫描 `<workdir>/projects/` 得到的全部 project 元数据（包含未打开的）。
    /// 启动 + rename / delete project 时维护；UI 树根据它渲染所有 project。
    pub available_projects: Vec<AvailableProject>,
    /// 已打开的 Project，按打开顺序保留（首次启动 = `[project] opened_projects` 决定）。
    pub projects: Vec<Project>,
    /// active project 在 `projects` 里的 idx；全部关闭时为 None。
    /// 所有"按 (group, name) 定位节点"的接口隐式作用在 active project 上，要求 UI 在 active=None 时不要触发。
    active_idx: Option<usize>,
    /// 全部 Project 的验证错误集，key=(project_id, group, name, row, col)。
    /// `revalidate_all` 扫描所有 Project 重建；UI 在树上做 `!` 聚合时按 project_id 过滤。
    pub validation_errors: HashSet<(String, String, String, usize, usize)>,
    /// 进程内剪贴板：tree 面板复制粘贴的载体。不持久化，关闭/退出随 engine drop。
    pub node_clipboard: Option<NodeClipboard>,
    /// 启动时从 workspace `tbl-tool.toml [separators]` 读到的"程序级默认分隔符"；
    /// 仅用作「新建空项目」时 schema.separators 的初值（从模板/文件复制时取 source.separators）。
    /// 已加载项目的 separators 各自走 schema，运行期不再读这里。
    pub default_separators: crate::types::SeparatorsSection,
    pub logs: Vec<String>,
}

/// 扫描得到的 project 元数据（不持有数据）。
#[derive(Clone, Debug)]
pub struct AvailableProject {
    pub id: String,
    /// 取自 `project.toml` 的 `[project].name`；空回落 id。
    pub name: String,
    /// `<workdir>/projects/<id>/`
    pub root: PathBuf,
    /// 取自 `project.tblschema` 的 `# @meta created_at`；用于 sort=created。
    pub created_at: String,
}

impl AvailableProject {
    /// 从 `Project` 的 schema.meta 派生（已加载场景）。
    pub fn from_project(p: &Project) -> Self {
        Self {
            id: p.schema.meta.id.clone(),
            name: if p.schema.meta.name.is_empty() {
                p.schema.meta.id.clone()
            } else {
                p.schema.meta.name.clone()
            },
            root: p.project_root.clone(),
            created_at: p.schema.meta.created_at.clone(),
        }
    }

    /// 从 `list_projects` 扫描结果派生（未加载场景）。
    /// `created_at` 需要再读一次 project.toml；为简化，本 helper 留空，
    /// `rescan_available` 内部如果需要 created_at 自行额外读。
    pub fn from_list_entry(e: crate::project::ProjectListEntry) -> Self {
        Self {
            id: e.id,
            name: e.name,
            root: e.root,
            created_at: String::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NodeKind {
    Table,
    Constant,
    Enum,
}

/// 跨 project 复制粘贴的节点载荷（深拷贝）。
#[derive(Clone, Debug)]
pub enum NodeBody {
    Table(Table),
    Constant(Constant),
    Enum(EnumDef),
}

/// 进程内剪贴板：tree 面板的复制粘贴载体（与 OS 剪贴板互不干扰）。
/// Grid 单元格 TSV 复制走 arboard，二者完全隔离。
#[derive(Clone, Debug)]
pub enum NodeClipboard {
    /// 单节点（Table/Constant/Enum），含完整 schema/records/entries。
    Node {
        source_project: String,
        source_group: String,
        body: NodeBody,
    },
    /// 整组：组内全部 table/constant/enum 都已深拷贝。
    Group {
        source_project: String,
        snapshot: Group,
    },
}

impl NodeClipboard {
    pub fn label(&self) -> String {
        match self {
            NodeClipboard::Node { source_project, source_group, body } => {
                let (kind, name) = match body {
                    NodeBody::Table(t) => ("Table", t.name.as_str()),
                    NodeBody::Constant(c) => ("Constant", c.name.as_str()),
                    NodeBody::Enum(e) => ("Enum", e.name.as_str()),
                };
                format!("{} {}（{}/{}）", kind, name, source_project, source_group)
            }
            NodeClipboard::Group { source_project, snapshot } => format!(
                "Group {}（{}，T{}+C{}+E{}）",
                snapshot.name,
                source_project,
                snapshot.tables.len(),
                snapshot.constants.len(),
                snapshot.enums.len(),
            ),
        }
    }

    pub fn is_node(&self) -> bool { matches!(self, NodeClipboard::Node { .. }) }
    pub fn is_group(&self) -> bool { matches!(self, NodeClipboard::Group { .. }) }
}

#[derive(Clone, Debug)]
pub enum ProjectAction {
    NewGroup    { project_id: String, name: String },
    NewTable    { project_id: String, group: String, name: String },
    NewConstant { project_id: String, group: String, name: String },
    NewEnum     { project_id: String, group: String, name: String },
    RenameGroup { project_id: String, old_name: String, new_name: String },
    RenameNode  { project_id: String, group: String, old_name: String, new_name: String },
    /// 重命名 project：迁移 `projects/<old_id>/` → `projects/<new_id>/`，写 project.toml 新 name + id。
    RenameProject { old_id: String, new_id: String, new_name: String },
    /// 删除 project：rm -rf `projects/<id>/`、从内存 `Vec<Project>` 移除。
    /// 至少要保留一个 project；删最后一个会失败（log 报错）。
    DeleteProject { project_id: String },
}

impl ProjectEngine {
    /// 老接口：单 Project 构造（兼容 fixture / CLI）。
    /// available_projects 仅含该 project；workdir 取自该 project。
    pub fn new(project: Project) -> Self {
        let workdir = project.workdir.clone();
        let available = vec![AvailableProject::from_project(&project)];
        Self {
            workdir,
            available_projects: available,
            projects: vec![project],
            active_idx: Some(0),
            validation_errors: HashSet::new(),
            node_clipboard: None,
            default_separators: crate::types::SeparatorsSection::default(),
            logs: Vec::new(),
        }
    }

    /// 多 Project 构造（兼容 #195 老调用点：把 projects 当作"全部"且全部 opened）。
    /// projects 必须非空；available_projects 由 projects 推导。
    pub fn new_multi(projects: Vec<Project>, last_id: Option<&str>) -> Self {
        assert!(!projects.is_empty(), "ProjectEngine::new_multi 至少要有一个 Project");
        let workdir = projects[0].workdir.clone();
        let available = projects.iter().map(AvailableProject::from_project).collect();
        let active_idx = last_id
            .and_then(|id| projects.iter().position(|p| p.schema.meta.id == id))
            .or(Some(0));
        Self {
            workdir,
            available_projects: available,
            projects,
            active_idx,
            validation_errors: HashSet::new(),
            node_clipboard: None,
            default_separators: crate::types::SeparatorsSection::default(),
            logs: Vec::new(),
        }
    }

    /// DBeaver-style 工作空间构造：available 为全部扫到的 project 元数据，
    /// opened 为已加载的（≤ available 的子集），active_id 选中其中一个 opened 的 id。
    /// 若 active_id 不在 opened 里，且 opened 非空，则 active = opened[0]；opened 空则 None。
    pub fn new_workspace(
        workdir: PathBuf,
        available: Vec<AvailableProject>,
        opened: Vec<Project>,
        active_id: Option<&str>,
    ) -> Self {
        let active_idx = if opened.is_empty() {
            None
        } else {
            active_id
                .and_then(|id| opened.iter().position(|p| p.schema.meta.id == id))
                .or(Some(0))
        };
        Self {
            workdir,
            available_projects: available,
            projects: opened,
            active_idx,
            validation_errors: HashSet::new(),
            node_clipboard: None,
            default_separators: crate::types::SeparatorsSection::default(),
            logs: Vec::new(),
        }
    }

    /// 设置「新建空项目」时使用的默认分隔符。`load_workspace` 启动时调一次。
    pub fn set_default_separators(&mut self, sep: crate::types::SeparatorsSection) {
        self.default_separators = sep;
    }

    /// active project 的不可变引用；active=None 时 panic（调用方应先 active() 取 Option）。
    pub fn project(&self) -> &Project {
        &self.projects[self.active_idx.expect("ProjectEngine::project() called with no active project")]
    }

    /// active project 的可变引用；active=None 时 panic。
    pub fn project_mut(&mut self) -> &mut Project {
        let idx = self.active_idx.expect("ProjectEngine::project_mut() called with no active project");
        &mut self.projects[idx]
    }

    /// active project 的不可变引用，可能为 None（全部关闭时）。
    pub fn active(&self) -> Option<&Project> {
        self.active_idx.map(|i| &self.projects[i])
    }

    /// active project 的可变引用，可能为 None。
    pub fn active_mut(&mut self) -> Option<&mut Project> {
        self.active_idx.map(move |i| &mut self.projects[i])
    }

    /// active project 的 id；全部关闭时返回 None。
    pub fn active_project_id(&self) -> Option<&str> {
        self.active_idx.map(|i| self.projects[i].schema.meta.id.as_str())
    }

    /// 把 active 切到指定 id；找不到则不动。返回是否切换成功。
    /// 切换不动 validation_errors —— errors 现在按 project_id 维度索引，所有 Project 的错误都常驻。
    pub fn set_active_by_id(&mut self, id: &str) -> bool {
        if let Some(idx) = self.projects.iter().position(|p| p.schema.meta.id == id) {
            self.active_idx = Some(idx);
            true
        } else {
            false
        }
    }

    /// 把 active 置空（关闭最后一个 / 关闭当前 active 时调用）。
    pub fn set_active_none(&mut self) {
        self.active_idx = None;
    }

    /// 是否已打开（在 self.projects 里）。
    pub fn is_opened(&self, project_id: &str) -> bool {
        self.projects.iter().any(|p| p.schema.meta.id == project_id)
    }

    /// 当前已打开 project ids（按 self.projects 顺序）。
    pub fn opened_ids(&self) -> Vec<String> {
        self.projects.iter().map(|p| p.schema.meta.id.clone()).collect()
    }

    /// 当前 available（顺序保持扫描时的字典序，rename / delete 时维护）。
    pub fn available(&self) -> &[AvailableProject] {
        &self.available_projects
    }

    /// 重新扫描 `<workdir>/projects/`，刷新 available_projects。
    pub fn rescan_available(&mut self) {
        self.available_projects = crate::project::list_projects(&self.workdir)
            .into_iter()
            .map(AvailableProject::from_list_entry)
            .collect();
    }

    /// 打开一个 available project：从盘加载并 append 到 self.projects；
    /// 若已打开，返回 Ok(false)；找不到返回 Err。
    /// 不修改 active；UI 通常会紧跟着 set_active_by_id。
    pub fn open_project(&mut self, project_id: &str) -> anyhow::Result<bool> {
        if self.is_opened(project_id) { return Ok(false); }
        if !self.available_projects.iter().any(|a| a.id == project_id) {
            anyhow::bail!("Project 不存在: {}", project_id);
        }
        let project = crate::project::load_specific_project(&self.workdir, project_id)?;
        self.projects.push(project);
        // 加载后立刻补上验证错误（让红框 / `!` 显示）：临时切到新打开 idx 跑 revalidate_all
        let prev = self.active_idx;
        self.active_idx = Some(self.projects.len() - 1);
        self.revalidate_all();
        self.active_idx = prev;
        Ok(true)
    }

    /// 关闭一个已打开 project：从 self.projects 移除（不动盘）。
    /// 若是 active，active_idx 切到 None。返回是否真关了一个。
    /// 不动 available_projects（关闭 ≠ 删除）。同时清掉该 pid 的 validation_errors。
    pub fn close_project(&mut self, project_id: &str) -> bool {
        let Some(idx) = self.projects.iter().position(|p| p.schema.meta.id == project_id) else {
            self.log(format!("Project 未打开: {}", project_id));
            return false;
        };
        self.projects.remove(idx);
        self.validation_errors.retain(|(p, _, _, _, _)| p != project_id);
        // active_idx 维护
        match self.active_idx {
            Some(active) if active == idx => {
                self.active_idx = None;
            }
            Some(active) if active > idx => {
                self.active_idx = Some(active - 1);
            }
            _ => {}
        }
        if self.projects.is_empty() {
            self.active_idx = None;
        }
        true
    }

    /// 用一个完整 schema（含项目身份 meta + sections 骨架）落地一个新 Project：
    /// `<workdir>/projects/<schema.meta.id>/` 目录下立即写 schema + config/<group>/<name>.tbl 空骨架，
    /// 然后加载到 self.projects 末尾，并加进 available_projects。返回新 project id 或错误信息。
    pub fn create_project_from_schema(
        &mut self,
        schema: crate::tblschema::TblSchema,
    ) -> Result<String, String> {
        if !crate::tblschema::is_valid_metadata_id(&schema.meta.id) {
            return Err(format!("project id 不合法: {}", schema.meta.id));
        }
        if self.available_projects.iter().any(|a| a.id == schema.meta.id) {
            return Err(format!("project id 已存在: {}", schema.meta.id));
        }
        let project_id = schema.meta.id.clone();
        let projects_dir = self.workdir.join(crate::project::PROJECTS_DIR);
        if let Err(e) = std::fs::create_dir_all(&projects_dir) {
            return Err(format!("创建 projects/ 失败: {}", e));
        }
        let project_root = projects_dir.join(&project_id);
        if project_root.exists() {
            return Err(format!("目录已存在: {}", project_root.display()));
        }
        if let Err(e) = crate::template::instantiate_template(&schema, &project_root) {
            let _ = std::fs::remove_dir_all(&project_root);
            return Err(format!("实例化失败: {}", e));
        }
        // 加载到 self.projects + available
        match crate::project::load_specific_project(&self.workdir, &project_id) {
            Ok(p) => {
                self.available_projects.push(AvailableProject::from_project(&p));
                self.available_projects.sort_by(|a, b| a.id.cmp(&b.id));
                self.projects.push(p);
                Ok(project_id)
            }
            Err(e) => Err(format!("加载新项目失败: {}", e)),
        }
    }

    /// 内存模式创建新 Project：与 clone_project_in_memory 行为对齐。
    /// - 不立即落盘；root_pending_create=true、schema_dirty=true
    /// - sections 展开为 group.is_new=true、节点 original="" → 树面板 `+` 标记
    /// - config 借用现有任意 opened project 的 WorkspaceConfig，否则用默认 fallback
    /// - 同时加进 available_projects（未保存前目录还不存在，但 UI 树要展示）
    /// 返回 new_id 或错误信息。
    ///
    /// `with_preset`：当 schema.meta.has_preset=true 时是否把 # @preset 行一并灌入项目。
    pub fn create_project_in_memory_with(
        &mut self,
        schema: crate::tblschema::TblSchema,
        with_preset: bool,
    ) -> Result<String, String> {
        if !crate::tblschema::is_valid_metadata_id(&schema.meta.id) {
            return Err(format!("project id 不合法: {}", schema.meta.id));
        }
        if self.available_projects.iter().any(|a| a.id == schema.meta.id) {
            return Err(format!("project id 已存在: {}", schema.meta.id));
        }
        let project_id = schema.meta.id.clone();
        let project_root = self.workdir.join(crate::project::PROJECTS_DIR).join(&project_id);

        // 借用任一已打开 project 的 config 当默认（与 workspace 共享段对齐）；都没有则走 toml 默认
        let config = self.projects.first().map(|p| p.config.clone())
            .or_else(|| toml::from_str::<WorkspaceConfig>("").ok())
            .ok_or_else(|| "无法构造默认 WorkspaceConfig".to_string())?;

        let mut new_project = Project {
            workdir: self.workdir.clone(),
            project_root: project_root.clone(),
            config,
            schema: schema.clone(),
            groups: Vec::new(),
            schema_dirty: true,
            root_pending_create: true,
        };

        // 用 apply_schema_to_project 把 sections 展开为 groups+nodes：
        // 它会建出 group.is_new=true、节点 dirty=true、original=""，正好对齐"内存模式"语义。
        let data_dir = new_project.data_dir();
        crate::tblschema::apply_schema_to_project(
            &mut new_project.groups,
            &schema.sections,
            &data_dir,
            with_preset,
        );

        let avail = AvailableProject::from_project(&new_project);
        self.available_projects.push(avail);
        self.available_projects.sort_by(|a, b| a.id.cmp(&b.id));
        self.projects.push(new_project);
        Ok(project_id)
    }

    /// 旧接口：默认按 schema.meta.has_preset 决定是否灌入预设数据。
    /// 新代码请直接调 `create_project_in_memory_with(schema, with_preset)`。
    pub fn create_project_in_memory(
        &mut self,
        schema: crate::tblschema::TblSchema,
    ) -> Result<String, String> {
        let with_preset = schema.meta.has_preset;
        self.create_project_in_memory_with(schema, with_preset)
    }

    /// 内存深拷贝克隆：把 source_project_id 的当前状态（含未保存改动）整体复制为新 project，
    /// 加进 self.projects 但**不立即落盘**（root_pending_create=true，schema_dirty=true，
    /// 全部 group is_new=true，所有节点 dirty=true / original=""）。用户保存时才会落盘。
    /// - source 必须是 opened（在 self.projects 里）
    /// - 同时把新 project 加进 available_projects（未保存前目录还不存在，但 UI 树要展示）
    /// 返回 new_id 或 None（id 冲突 / source 不存在）。
    pub fn clone_project_in_memory(
        &mut self,
        source_project_id: &str,
        new_id: &str,
        new_name: &str,
    ) -> Option<String> {
        if !crate::tblschema::is_valid_metadata_id(new_id) {
            self.log(format!("project id 不合法: {}", new_id));
            return None;
        }
        if self.available_projects.iter().any(|a| a.id == new_id) {
            self.log(format!("project id 已存在: {}", new_id));
            return None;
        }
        let source = self.find_project(source_project_id)?.clone();
        let new_root = self.workdir.join(crate::project::PROJECTS_DIR).join(new_id);
        let mut new_project = source;
        new_project.project_root = new_root.clone();
        new_project.schema.meta.id = new_id.to_string();
        new_project.schema.meta.name = new_name.to_string();
        new_project.schema.meta.created_at =
            chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        // source_template* 沿用源（克隆来源最早还是那个 template）
        new_project.schema_dirty = true;
        new_project.root_pending_create = true;

        let new_data_dir = new_project.data_dir();
        for g in &mut new_project.groups {
            g.dir = new_data_dir.join(&g.name);
            g.is_new = true;
            for t in &mut g.tables {
                t.path = g.dir.join(format!("{}.tbl", t.name));
                t.dirty = true;
                t.original = String::new();
            }
            for c in &mut g.constants {
                c.path = g.dir.join(format!("{}.tbl", c.name));
                c.dirty = true;
                c.original = String::new();
            }
            for e in &mut g.enums {
                e.path = g.dir.join(format!("{}.tbl", e.name));
                e.dirty = true;
                e.original = String::new();
            }
        }
        // 同步 schema.sections 与 groups（克隆下来的 sections 已经一致，但保险起见走一次）
        sync_schema_from_groups(&mut new_project);
        let avail = AvailableProject::from_project(&new_project);
        self.available_projects.push(avail);
        self.available_projects.sort_by(|a, b| a.id.cmp(&b.id));
        self.projects.push(new_project);
        self.log(format!(
            "已克隆 {} → {}（内存中，需保存才落地）",
            source_project_id, new_id
        ));
        Some(new_id.to_string())
    }

    /// 按 id 查 project（不可变）。
    pub fn find_project(&self, id: &str) -> Option<&Project> {
        self.projects.iter().find(|p| p.schema.meta.id == id)
    }

    /// 按 id 查 project（可变）。
    pub fn find_project_mut(&mut self, id: &str) -> Option<&mut Project> {
        self.projects.iter_mut().find(|p| p.schema.meta.id == id)
    }

    /// 当前 active Project 在指定 (group, name) 上是否有验证错误。
    /// UI 树聚合 / 单元格红框查询都走它。
    pub fn has_active_node_error(&self, group: &str, name: &str) -> bool {
        let Some(pid) = self.active_project_id() else { return false; };
        self.validation_errors.iter()
            .any(|(p, g, n, _, _)| p == pid && g == group && n == name)
    }

    /// 当前 active Project 的某 group 是否有任意验证错误。供 group 行的 `!` 标记。
    pub fn has_active_group_error(&self, group: &str) -> bool {
        let Some(pid) = self.active_project_id() else { return false; };
        self.validation_errors.iter()
            .any(|(p, g, _, _, _)| p == pid && g == group)
    }

    /// 当前 active Project 在 (group, name, row, col) 上是否有错。供单元格红框查询。
    pub fn has_active_cell_error(&self, group: &str, name: &str, row: usize, col: usize) -> bool {
        let Some(pid) = self.active_project_id() else { return false; };
        self.validation_errors.contains(
            &(pid.to_string(), group.to_string(), name.to_string(), row, col)
        )
    }

    /// 指定 project 的 (group, name) 是否有验证错误（多 Project 树面板专用）。
    pub fn has_node_error(&self, project_id: &str, group: &str, name: &str) -> bool {
        self.validation_errors.iter()
            .any(|(p, g, n, _, _)| p == project_id && g == group && n == name)
    }

    /// 指定 project 的某 group 是否有任意验证错误。
    pub fn has_group_error(&self, project_id: &str, group: &str) -> bool {
        self.validation_errors.iter()
            .any(|(p, g, _, _, _)| p == project_id && g == group)
    }

    /// 指定 project 是否有任意验证错误（multi-project 时 project 根节点 `!` 用）。
    pub fn has_project_error(&self, project_id: &str) -> bool {
        self.validation_errors.iter().any(|(p, _, _, _, _)| p == project_id)
    }

    pub fn log(&mut self, msg: String) {
        let now = chrono::Local::now().format("%H:%M:%S").to_string();
        self.logs.push(format!("{} {}", now, msg));
    }

    pub fn logs(&self) -> &[String] {
        &self.logs
    }

    pub fn find_table(&self, group: &str, name: &str) -> Option<&Table> {
        self.project().groups.iter()
            .find(|g| g.name == group)?
            .tables.iter()
            .find(|t| t.name == name)
    }

    pub fn find_table_mut(&mut self, group: &str, name: &str) -> Option<&mut Table> {
        self.project_mut().groups.iter_mut()
            .find(|g| g.name == group)?
            .tables.iter_mut()
            .find(|t| t.name == name)
    }

    pub fn find_constant(&self, group: &str, name: &str) -> Option<&Constant> {
        self.project().groups.iter()
            .find(|g| g.name == group)?
            .constants.iter()
            .find(|c| c.name == name)
    }

    pub fn find_constant_mut(&mut self, group: &str, name: &str) -> Option<&mut Constant> {
        self.project_mut().groups.iter_mut()
            .find(|g| g.name == group)?
            .constants.iter_mut()
            .find(|c| c.name == name)
    }

    pub fn find_enum(&self, group: &str, name: &str) -> Option<&EnumDef> {
        self.project().groups.iter()
            .find(|g| g.name == group)?
            .enums.iter()
            .find(|e| e.name == name)
    }

    pub fn find_enum_mut(&mut self, group: &str, name: &str) -> Option<&mut EnumDef> {
        self.project_mut().groups.iter_mut()
            .find(|g| g.name == group)?
            .enums.iter_mut()
            .find(|e| e.name == name)
    }

    pub fn mark_enum_dirty(&mut self, group: &str, name: &str) {
        if let Some(e) = self.find_enum_mut(group, name) {
            e.update_dirty();
        }
    }

    pub fn set_enum_cell(&mut self, group: &str, name: &str, row: usize, col: usize, val: &str) {
        if let Some(g) = self.project_mut().groups.iter_mut().find(|g| g.name == group) {
            if let Some(en) = g.enums.iter_mut().find(|e| e.name == name) {
                if !val.is_empty() {
                    while en.entries.len() <= row {
                        en.entries.push(EnumEntry::default());
                    }
                }
                if let Some(entry) = en.entries.get_mut(row) {
                    match col {
                        0 => entry.id = val.trim().to_string(),
                        1 => entry.name = val.trim().replace(' ', ""),
                        2 => entry.desc = val.to_string(),
                        _ => {}
                    }
                }
                en.update_dirty();
            }
        }
    }

    pub fn commit_enum_cell(&mut self, group: &str, name: &str, row: usize, col: usize, val: String) {
        if let Some(g) = self.project_mut().groups.iter_mut().find(|g| g.name == group) {
            if let Some(en) = g.enums.iter_mut().find(|e| e.name == name) {
                if !val.is_empty() {
                    while en.entries.len() <= row {
                        en.entries.push(EnumEntry::default());
                    }
                }
                if let Some(entry) = en.entries.get_mut(row) {
                    match col {
                        0 => entry.id = val,
                        1 => entry.name = val,
                        2 => entry.desc = val,
                        _ => {}
                    }
                }
                en.update_dirty();
            }
        }
    }

    pub fn clear_enum_cells(&mut self, group: &str, name: &str, cells: &[(usize, usize)]) {
        if let Some(g) = self.project_mut().groups.iter_mut().find(|g| g.name == group) {
            if let Some(en) = g.enums.iter_mut().find(|e| e.name == name) {
                for &(row, col) in cells {
                    if let Some(entry) = en.entries.get_mut(row) {
                        match col {
                            0 => entry.id.clear(),
                            1 => entry.name.clear(),
                            2 => entry.desc.clear(),
                            _ => {}
                        }
                    }
                }
                en.update_dirty();
            }
        }
    }

    pub fn paste_enum_data(&mut self, group: &str, name: &str, start_row: usize, start_col: usize, text: &str) {
        let lines: Vec<&str> = text.lines().collect();
        if lines.is_empty() { return; }
        if let Some(g) = self.project_mut().groups.iter_mut().find(|g| g.name == group) {
            if let Some(en) = g.enums.iter_mut().find(|e| e.name == name) {
                for (i, line) in lines.iter().enumerate() {
                    let row = start_row + i;
                    while en.entries.len() <= row {
                        en.entries.push(EnumEntry::default());
                    }
                    let cells: Vec<&str> = line.split('\t').collect();
                    for (j, cell) in cells.iter().enumerate() {
                        let col = start_col + j;
                        let entry = &mut en.entries[row];
                        match col {
                            0 => entry.id = cell.to_string(),
                            1 => entry.name = cell.to_string(),
                            2 => entry.desc = cell.to_string(),
                            _ => {}
                        }
                    }
                }
                en.update_dirty();
            }
        }
        self.log(format!("粘贴 {}行 数据", lines.len()));
    }

    pub fn insert_enum_row(&mut self, group: &str, name: &str, at: usize) {
        if let Some(g) = self.project_mut().groups.iter_mut().find(|g| g.name == group) {
            if let Some(en) = g.enums.iter_mut().find(|e| e.name == name) {
                let at = at.min(en.entries.len());
                en.entries.insert(at, EnumEntry::default());
                en.update_dirty();
            }
        }
    }

    pub fn delete_enum_row(&mut self, group: &str, name: &str, at: usize) {
        if let Some(g) = self.project_mut().groups.iter_mut().find(|g| g.name == group) {
            if let Some(en) = g.enums.iter_mut().find(|e| e.name == name) {
                if at < en.entries.len() {
                    en.entries.remove(at);
                    en.update_dirty();
                }
            }
        }
    }

    pub fn delete_enum_rows(&mut self, group: &str, name: &str, start: usize, end: usize) {
        if let Some(g) = self.project_mut().groups.iter_mut().find(|g| g.name == group) {
            if let Some(en) = g.enums.iter_mut().find(|e| e.name == name) {
                let end = end.min(en.entries.len());
                if start < end {
                    en.entries.drain(start..end);
                    en.update_dirty();
                    self.log(format!("已删除 {} 行", end - start));
                }
            }
        }
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
        if let Some(g) = self.project_mut().groups.iter_mut().find(|g| g.name == group) {
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
        if let Some(g) = self.project_mut().groups.iter_mut().find(|g| g.name == group) {
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
        if let Some(g) = self.project_mut().groups.iter_mut().find(|g| g.name == group) {
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
        if let Some(g) = self.project_mut().groups.iter_mut().find(|g| g.name == group) {
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

    /// 提交表头编辑。
    /// `header_row` 是 0-based UI 行号（与 `TableHeaderRow::row()` 等价）：
    ///   0=desc, 1=export, 2=type, 3=field —— 顺序与 UI 表头从上到下一致，也与 .tbl 序列化顺序一致。
    pub fn commit_header_edit(&mut self, group: &str, name: &str, header_row: usize, col: usize, val: String) {
        let mut keyword_err = None;
        if let Some(g) = self.project_mut().groups.iter_mut().find(|g| g.name == group) {
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
        if let Some(g) = self.project_mut().groups.iter_mut().find(|g| g.name == group) {
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
        if let Some(g) = self.project_mut().groups.iter_mut().find(|g| g.name == group) {
            if let Some(t) = g.tables.iter_mut().find(|t| t.name == name) {
                if at < t.records.len() {
                    t.records.remove(at);
                    t.update_dirty();
                }
            }
        }
    }

    pub fn delete_rows(&mut self, group: &str, name: &str, start: usize, end: usize) {
        if let Some(g) = self.project_mut().groups.iter_mut().find(|g| g.name == group) {
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
        if let Some(g) = self.project_mut().groups.iter_mut().find(|g| g.name == group) {
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
        if let Some(g) = self.project_mut().groups.iter_mut().find(|g| g.name == group) {
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
        if let Some(g) = self.project_mut().groups.iter_mut().find(|g| g.name == group) {
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
        if let Some(g) = self.project_mut().groups.iter_mut().find(|g| g.name == group) {
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
        if let Some(g) = self.project_mut().groups.iter_mut().find(|g| g.name == group) {
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
        if let Some(g) = self.project_mut().groups.iter_mut().find(|g| g.name == group) {
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
        let sep = &self.project().config.separators;
        let refs = RefIndex::build(&self.project().groups);
        let allow_ref = self.project().config.ui.as_ref()
            .map_or(true, |u| u.constant_ref_allowed);
        for group in &self.project().groups {
            for table in &group.tables {
                if table.deleted { continue; }
                for err in validate_table(table, sep, Some(&refs)) {
                    errors.push(format_validation_log(&group.name, &table.name, &err));
                }
            }
            for constant in &group.constants {
                if constant.deleted { continue; }
                for err in validate_constant(constant, sep, allow_ref, Some(&refs)) {
                    errors.push(format_validation_log(&group.name, &constant.name, &err));
                }
            }
            for enum_def in &group.enums {
                if enum_def.deleted { continue; }
                for err in validate_enum(enum_def) {
                    errors.push(format_validation_log(&group.name, &enum_def.name, &err));
                }
            }
        }
        errors
    }

    pub fn revalidate(&mut self, group: &str, name: &str) {
        let Some(pid) = self.active_project_id().map(str::to_string) else { return; };
        self.validation_errors.retain(|(p, g, n, _, _)| p != &pid || g != group || n != name);
        let sep = self.project().config.separators.clone();
        let refs = RefIndex::build(&self.project().groups);
        let allow_ref = self.project().config.ui.as_ref()
            .map_or(true, |u| u.constant_ref_allowed);
        let mut new_errs: Vec<(usize, usize)> = Vec::new();
        {
            let g = match self.project().groups.iter().find(|g| g.name == group) { Some(g) => g, None => return };
            if let Some(table) = g.tables.iter().find(|t| t.name == name) {
                if table.deleted { return; }
                for err in validate_table(table, &sep, Some(&refs)) {
                    new_errs.push((err.row, err.col));
                }
            }
            if let Some(constant) = g.constants.iter().find(|c| c.name == name) {
                if constant.deleted { return; }
                for err in validate_constant(constant, &sep, allow_ref, Some(&refs)) {
                    new_errs.push((err.row, err.col));
                }
            }
            if let Some(enum_def) = g.enums.iter().find(|e| e.name == name) {
                if enum_def.deleted { return; }
                for err in validate_enum(enum_def) {
                    new_errs.push((err.row, err.col));
                }
            }
        }
        for (r, c) in new_errs {
            self.validation_errors.insert((pid.clone(), group.to_string(), name.to_string(), r, c));
        }
    }

    /// 重算 active project 的所有节点错误；不影响其它 Project 的错误索引。
    pub fn revalidate_all(&mut self) {
        let Some(pid) = self.active_project_id().map(str::to_string) else { return; };
        self.validation_errors.retain(|(p, _, _, _, _)| p != &pid);
        let groups: Vec<_> = self.project().groups.iter()
            .map(|g| (
                g.name.clone(),
                g.tables.iter().map(|t| t.name.clone()).collect::<Vec<_>>(),
                g.constants.iter().map(|c| c.name.clone()).collect::<Vec<_>>(),
                g.enums.iter().map(|e| e.name.clone()).collect::<Vec<_>>(),
            ))
            .collect();
        for (gname, tables, constants, enums) in &groups {
            for tname in tables { self.revalidate(gname, tname); }
            for cname in constants { self.revalidate(gname, cname); }
            for ename in enums { self.revalidate(gname, ename); }
        }
    }

    /// 全 Project 重算：扫描每个 Project 的所有节点。启动加载、切换 active、批量验证用。
    /// 临时切换 active_idx 以复用现有 active-scope 逻辑，最后恢复。
    pub fn revalidate_all_projects(&mut self) {
        self.validation_errors.clear();
        let original = self.active_idx;
        for idx in 0..self.projects.len() {
            self.active_idx = Some(idx);
            self.revalidate_all();
        }
        self.active_idx = original;
    }

    // --- PLACEHOLDER_SAVE2 ---

    /// 保存 active Project 的所有改动（右键"保存此 project"或单 Project 兼容路径）。
    /// 内部 revalidate_all 仅作用 active；存在验证错误就放弃保存并打日志。
    /// 全部关闭时（无 active）直接 no-op。
    pub fn save_all(&mut self) {
        if self.active_idx.is_none() {
            self.log("没有打开的 Project，无需保存".to_string());
            return;
        }
        self.revalidate_all();
        let pid = self.active_project_id().expect("active checked").to_string();
        let active_errs = self.validation_errors.iter()
            .filter(|(p, _, _, _, _)| p == &pid)
            .count();
        if active_errs > 0 {
            let errors = self.validate();
            for e in &errors { self.logs.push(e.clone()); }
            self.logs.push(format!("[保存失败] 共 {} 个验证错误", active_errs));
            return;
        }
        let (count, deleted) = save_project_files(self.project_mut());
        if count > 0 || deleted > 0 {
            self.log(format!("已保存 {} 个文件, 删除 {} 个", count, deleted));
        } else {
            self.log("无修改需要保存".to_string());
        }
    }

    /// 保存全部 Project（工具栏"保存"默认走这里）。
    /// 全 Project 重算后若任一 Project 有验证错误则中止；否则逐个写盘。
    pub fn save_all_projects(&mut self) {
        self.revalidate_all_projects();
        if !self.validation_errors.is_empty() {
            let errors = self.validate();
            for e in &errors { self.logs.push(e.clone()); }
            self.logs.push(format!("[保存失败] 共 {} 个验证错误", self.validation_errors.len()));
            return;
        }
        let mut total_count = 0;
        let mut total_deleted = 0;
        for project in &mut self.projects {
            let (c, d) = save_project_files(project);
            total_count += c;
            total_deleted += d;
        }
        if total_count > 0 || total_deleted > 0 {
            self.log(format!("已保存 {} 个文件, 删除 {} 个（{} 个 Project）",
                total_count, total_deleted, self.projects.len()));
        } else {
            self.log("无修改需要保存".to_string());
        }
    }

    pub fn reload(&mut self) {
        let workdir = self.workdir.clone();
        let last_id = self.active_project_id().map(str::to_string);
        match crate::project::load_workspace(&workdir) {
            Ok(mut new_engine) => {
                // 维持原 active：若原 active 在新 opened 列表里则保留，否则用新 active
                if let Some(pid) = last_id.as_deref() {
                    new_engine.set_active_by_id(pid);
                }
                let pcount = new_engine.projects.len();
                let group_total: usize = new_engine.projects.iter().map(|p| p.groups.len()).sum();
                self.workdir = new_engine.workdir;
                self.available_projects = new_engine.available_projects;
                self.projects = new_engine.projects;
                self.active_idx = new_engine.active_idx;
                self.validation_errors.clear();
                self.log(format!("重新加载完成，共 {} 个 Project / {} 个 Group", pcount, group_total));
            }
            Err(e) => self.log(format!("加载失败: {}", e)),
        }
    }

    /// 当前 active Project 是否有未保存改动；`is_dirty_any` 跨全部 Project。
    /// 全部关闭时返回 false。
    pub fn is_dirty(&self) -> bool {
        let Some(p) = self.active() else { return false; };
        if p.root_pending_create || p.schema_dirty { return true; }
        for g in &p.groups {
            if g.is_new { return true; }
            for t in &g.tables { if t.dirty || t.deleted { return true; } }
            for c in &g.constants { if c.dirty || c.deleted { return true; } }
            for e in &g.enums { if e.dirty || e.deleted { return true; } }
        }
        false
    }

    /// 任一 Project 有未保存改动。退出工具 / 全保存判空用。
    pub fn is_dirty_any(&self) -> bool {
        for p in &self.projects {
            if p.root_pending_create || p.schema_dirty { return true; }
            for g in &p.groups {
                if g.is_new { return true; }
                for t in &g.tables { if t.dirty || t.deleted { return true; } }
                for c in &g.constants { if c.dirty || c.deleted { return true; } }
                for e in &g.enums { if e.dirty || e.deleted { return true; } }
            }
        }
        false
    }

    pub fn delete_group(&mut self, group_name: &str) {
        if let Some(g) = self.project().groups.iter().find(|g| g.name == group_name) {
            if g.is_new {
                self.project_mut().groups.retain(|g| g.name != group_name);
                self.log(format!("已移除新建 Group: {}", group_name));
            } else {
                if let Some(g) = self.project_mut().groups.iter_mut().find(|g| g.name == group_name) {
                    for t in &mut g.tables { t.deleted = true; }
                    for c in &mut g.constants { c.deleted = true; }
                    for e in &mut g.enums { e.deleted = true; }
                }
                self.log(format!("已标记删除 Group: {}", group_name));
            }
        }
        sync_schema_from_groups(self.project_mut());
        // 删除/标删后，该 group 下所有节点的 validation_errors 不应再被 UI 聚合
        let Some(pid) = self.active_project_id().map(str::to_string) else { return; };
        self.validation_errors.retain(|(p, g, _, _, _)| !(p == &pid && g == group_name));
    }

    pub fn delete_node(&mut self, group_name: &str, node_name: &str) {
        if let Some(g) = self.project_mut().groups.iter_mut().find(|g| g.name == group_name) {
            if let Some(t) = g.tables.iter_mut().find(|t| t.name == node_name) {
                t.deleted = true;
            }
            if let Some(c) = g.constants.iter_mut().find(|c| c.name == node_name) {
                c.deleted = true;
            }
            if let Some(e) = g.enums.iter_mut().find(|e| e.name == node_name) {
                e.deleted = true;
            }
        }
        sync_schema_from_groups(self.project_mut());
        let Some(pid) = self.active_project_id().map(str::to_string) else { return; };
        self.validation_errors.retain(|(p, g, n, _, _)| !(p == &pid && g == group_name && n == node_name));
        self.log(format!("已标记删除: {}/{}", group_name, node_name));
    }

    /// 把指定节点深拷贝到剪贴板（不改源数据，不动 dirty 标）。
    /// 找不到节点 → 仅 log warn，剪贴板不变。
    pub fn clipboard_copy_node(
        &mut self,
        project_id: &str,
        group: &str,
        node_name: &str,
        kind: NodeKind,
    ) {
        let Some(p) = self.find_project(project_id) else {
            self.log(format!("Project 不存在: {}", project_id));
            return;
        };
        let Some(g) = p.groups.iter().find(|g| g.name == group) else {
            self.log(format!("Group 不存在: {}/{}", project_id, group));
            return;
        };
        let body = match kind {
            NodeKind::Table => g.tables.iter().find(|t| t.name == node_name).cloned().map(NodeBody::Table),
            NodeKind::Constant => g.constants.iter().find(|c| c.name == node_name).cloned().map(NodeBody::Constant),
            NodeKind::Enum => g.enums.iter().find(|e| e.name == node_name).cloned().map(NodeBody::Enum),
        };
        let Some(body) = body else {
            self.log(format!("节点不存在: {}/{}/{}", project_id, group, node_name));
            return;
        };
        let kind_label = match &body {
            NodeBody::Table(_) => "Table",
            NodeBody::Constant(_) => "Constant",
            NodeBody::Enum(_) => "Enum",
        };
        self.node_clipboard = Some(NodeClipboard::Node {
            source_project: project_id.to_string(),
            source_group: group.to_string(),
            body,
        });
        self.log(format!("已复制 {} 到剪贴板: {}/{}/{}", kind_label, project_id, group, node_name));
    }

    /// 整组深拷贝到剪贴板（含组内全部 table/constant/enum）。
    pub fn clipboard_copy_group(&mut self, project_id: &str, group: &str) {
        let Some(p) = self.find_project(project_id) else {
            self.log(format!("Project 不存在: {}", project_id));
            return;
        };
        let Some(g) = p.groups.iter().find(|g| g.name == group) else {
            self.log(format!("Group 不存在: {}/{}", project_id, group));
            return;
        };
        let snapshot = g.clone();
        let summary = format!(
            "T{}+C{}+E{}",
            snapshot.tables.len(),
            snapshot.constants.len(),
            snapshot.enums.len(),
        );
        self.node_clipboard = Some(NodeClipboard::Group {
            source_project: project_id.to_string(),
            snapshot,
        });
        self.log(format!("已复制 Group 到剪贴板: {}/{}（{}）", project_id, group, summary));
    }

    /// 粘贴单节点到目标 (project, group)。
    /// - 名字冲突走 `_copy` / `_copy2` …
    /// - path 重写到 `<target_group.dir>/<final_name>.tbl`
    /// - dirty=true，original=""，is_new 在 Group 层面已独立标识，节点本身不需要
    /// - 末尾对该节点跑一次 revalidate（active 状态下生效；非 active 暂不需要红框）
    /// 返回新节点名（成功时）。
    pub fn paste_node_to(&mut self, target_project: &str, target_group: &str) -> Option<String> {
        let Some(NodeClipboard::Node { body, .. }) = self.node_clipboard.clone() else {
            self.log("剪贴板为空或类型不匹配（需要节点剪贴板）".to_string());
            return None;
        };
        let project = self.find_project_mut(target_project)?;
        let g = project.groups.iter_mut().find(|g| g.name == target_group)?;
        let mut existing: Vec<String> = Vec::new();
        existing.extend(g.tables.iter().map(|t| t.name.clone()));
        existing.extend(g.constants.iter().map(|c| c.name.clone()));
        existing.extend(g.enums.iter().map(|e| e.name.clone()));
        let base = match &body {
            NodeBody::Table(t) => t.name.clone(),
            NodeBody::Constant(c) => c.name.clone(),
            NodeBody::Enum(e) => e.name.clone(),
        };
        let final_name = if existing.iter().any(|n| n == &base) {
            dedupe_name(&base, existing.iter().map(String::as_str))
        } else {
            base.clone()
        };
        let new_path = g.dir.join(format!("{}.tbl", final_name));
        let kind_label = match body {
            NodeBody::Table(mut t) => {
                t.name = final_name.clone();
                t.path = new_path;
                t.dirty = true;
                t.original = String::new();
                g.tables.push(t);
                "Table"
            }
            NodeBody::Constant(mut c) => {
                c.name = final_name.clone();
                c.path = new_path;
                c.dirty = true;
                c.original = String::new();
                g.constants.push(c);
                "Constant"
            }
            NodeBody::Enum(mut e) => {
                e.name = final_name.clone();
                e.path = new_path;
                e.dirty = true;
                e.original = String::new();
                g.enums.push(e);
                "Enum"
            }
        };
        self.log(format!(
            "已粘贴 {} 到 {}/{}/{}",
            kind_label, target_project, target_group, final_name
        ));
        if let Some(p) = self.find_project_mut(target_project) {
            sync_schema_from_groups(p);
        }
        let prev = self.active_idx;
        if let Some(idx) = self.projects.iter().position(|p| p.schema.meta.id == target_project) {
            self.active_idx = Some(idx);
            self.revalidate(target_group, &final_name);
            self.active_idx = prev;
        }
        Some(final_name)
    }

    /// 粘贴整组到目标 project，作为兄弟 Group。
    /// - group 名字冲突走 `_copy` / `_copy2`
    /// - group.dir 重写到 `<project.data_dir()>/<final_name>`，is_new=true（save 时建目录）
    /// - 每个 child path 重写到新 dir 下，dirty=true，original=""
    /// - 末尾整 project revalidate
    /// 返回新 group 名。
    pub fn paste_group_to(&mut self, target_project: &str) -> Option<String> {
        let Some(NodeClipboard::Group { snapshot, .. }) = self.node_clipboard.clone() else {
            self.log("剪贴板为空或类型不匹配（需要 Group 剪贴板）".to_string());
            return None;
        };
        let project = self.find_project_mut(target_project)?;
        let existing: Vec<String> = project.groups.iter().map(|g| g.name.clone()).collect();
        let final_name = if existing.iter().any(|n| n == &snapshot.name) {
            dedupe_name(&snapshot.name, existing.iter().map(String::as_str))
        } else {
            snapshot.name.clone()
        };
        let new_dir = project.data_dir().join(&final_name);
        let mut new_group = snapshot;
        new_group.name = final_name.clone();
        new_group.dir = new_dir.clone();
        new_group.is_new = true;
        for t in &mut new_group.tables {
            t.path = new_dir.join(format!("{}.tbl", t.name));
            t.dirty = true;
            t.original = String::new();
        }
        for c in &mut new_group.constants {
            c.path = new_dir.join(format!("{}.tbl", c.name));
            c.dirty = true;
            c.original = String::new();
        }
        for e in &mut new_group.enums {
            e.path = new_dir.join(format!("{}.tbl", e.name));
            e.dirty = true;
            e.original = String::new();
        }
        let summary = format!(
            "T{}+C{}+E{}",
            new_group.tables.len(),
            new_group.constants.len(),
            new_group.enums.len(),
        );
        project.groups.push(new_group);
        sync_schema_from_groups(project);
        self.log(format!("已粘贴 Group 到 {}/{}（{}）", target_project, final_name, summary));
        let prev = self.active_idx;
        if let Some(idx) = self.projects.iter().position(|p| p.schema.meta.id == target_project) {
            self.active_idx = Some(idx);
            self.revalidate_all();
            self.active_idx = prev;
        }
        Some(final_name)
    }

    /// 清空剪贴板（关闭/退出由 drop 自动处理；本接口供未来"清空剪贴板"按钮用）。
    pub fn clipboard_clear(&mut self) {
        self.node_clipboard = None;
    }

    // --- PLACEHOLDER_ACTIONS ---

    pub fn execute_action(&mut self, action: &ProjectAction) {
        match action {
            ProjectAction::NewGroup { project_id, name } => {
                let Some(project) = self.find_project_mut(project_id) else {
                    self.log(format!("Project 不存在: {}", project_id));
                    return;
                };
                let dir = project.data_dir().join(name);
                project.groups.push(Group {
                    name: name.clone(),
                    dir,
                    tables: Vec::new(),
                    constants: Vec::new(),
                    enums: Vec::new(),
                    is_new: true,
                });
                sync_schema_from_groups(project);
                self.log(format!("[{}] 新建 Group: {}", project_id, name));
            }
            ProjectAction::NewTable { project_id, group, name } => {
                let mut ok = false;
                if let Some(project) = self.find_project_mut(project_id) {
                    if let Some(g) = project.groups.iter_mut().find(|g| g.name == *group) {
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
                            },
                            records: Vec::new(),
                            dirty: true,
                            deleted: false,
                            original: String::new(),
                        });
                        ok = true;
                    }
                    sync_schema_from_groups(project);
                }
                if ok {
                    self.log(format!("[{}] 新建 Table: {}/{}", project_id, group, name));
                }
            }
            ProjectAction::NewConstant { project_id, group, name } => {
                let mut ok = false;
                if let Some(project) = self.find_project_mut(project_id) {
                    if let Some(g) = project.groups.iter_mut().find(|g| g.name == *group) {
                        let path = g.dir.join(format!("{}.tbl", name));
                        g.constants.push(Constant {
                            name: name.clone(),
                            path,
                            entries: Vec::new(),
                            dirty: true,
                            deleted: false,
                            original: String::new(),
                        });
                        ok = true;
                    }
                    sync_schema_from_groups(project);
                }
                if ok {
                    self.log(format!("[{}] 新建 Constant: {}/{}", project_id, group, name));
                }
            }
            ProjectAction::NewEnum { project_id, group, name } => {
                let mut ok = false;
                if let Some(project) = self.find_project_mut(project_id) {
                    if let Some(g) = project.groups.iter_mut().find(|g| g.name == *group) {
                        let path = g.dir.join(format!("{}.tbl", name));
                        g.enums.push(EnumDef {
                            name: name.clone(),
                            path,
                            entries: Vec::new(),
                            dirty: true,
                            deleted: false,
                            original: String::new(),
                        });
                        ok = true;
                    }
                    sync_schema_from_groups(project);
                }
                if ok {
                    self.log(format!("[{}] 新建 Enum: {}/{}", project_id, group, name));
                }
            }
            ProjectAction::RenameGroup { project_id, old_name, new_name } => {
                let pid = project_id.clone();
                if let Some(project) = self.find_project_mut(&pid) {
                    let config_dir = project.data_dir();
                    let old_dir = config_dir.join(old_name);
                    let new_dir = config_dir.join(new_name);
                    let _ = std::fs::rename(&old_dir, &new_dir);
                    if let Some(g) = project.groups.iter_mut().find(|g| g.name == *old_name) {
                        g.name = new_name.clone();
                        g.dir = new_dir.clone();
                        // 同步组下子节点的 path（每个 .tbl 都在 group dir 下）
                        for t in &mut g.tables { t.path = new_dir.join(format!("{}.tbl", t.name)); }
                        for c in &mut g.constants { c.path = new_dir.join(format!("{}.tbl", c.name)); }
                        for e in &mut g.enums { e.path = new_dir.join(format!("{}.tbl", e.name)); }
                    }
                    sync_schema_from_groups(project);
                }
                // (pid, old_name, *) → (pid, new_name, *)
                let migrated: Vec<_> = self.validation_errors.iter()
                    .filter(|(p, g, _, _, _)| p == project_id && g == old_name)
                    .cloned().collect();
                for entry in &migrated { self.validation_errors.remove(entry); }
                for (_, _, n, r, c) in migrated {
                    self.validation_errors.insert((project_id.clone(), new_name.clone(), n, r, c));
                }
                self.log(format!("[{}] 重命名 Group: {} → {}", project_id, old_name, new_name));
            }
            ProjectAction::RenameNode { project_id, group, old_name, new_name } => {
                if let Some(project) = self.find_project_mut(project_id) {
                    if let Some(g) = project.groups.iter_mut().find(|g| g.name == *group) {
                        let old_path = g.dir.join(format!("{}.tbl", old_name));
                        let new_path = g.dir.join(format!("{}.tbl", new_name));
                        let _ = std::fs::rename(&old_path, &new_path);
                        if let Some(t) = g.tables.iter_mut().find(|t| t.name == *old_name) {
                            t.name = new_name.clone();
                            t.path = new_path.clone();
                        }
                        if let Some(c) = g.constants.iter_mut().find(|c| c.name == *old_name) {
                            c.name = new_name.clone();
                            c.path = new_path.clone();
                        }
                        if let Some(e) = g.enums.iter_mut().find(|e| e.name == *old_name) {
                            e.name = new_name.clone();
                            e.path = new_path;
                        }
                    }
                    sync_schema_from_groups(project);
                }
                let migrated: Vec<_> = self.validation_errors.iter()
                    .filter(|(p, g, n, _, _)| p == project_id && g == group && n == old_name)
                    .cloned().collect();
                for entry in &migrated { self.validation_errors.remove(entry); }
                for (_, g, _, r, c) in migrated {
                    self.validation_errors.insert((project_id.clone(), g, new_name.clone(), r, c));
                }
                self.log(format!("[{}] 重命名: {}/{} → {}", project_id, group, old_name, new_name));
            }
            ProjectAction::RenameProject { old_id, new_id, new_name } => {
                self.execute_rename_project(old_id, new_id, new_name);
            }
            ProjectAction::DeleteProject { project_id } => {
                self.execute_delete_project(project_id);
            }
        }
        // 项目结构变化（新建 / 重命名）：必须同步 validation_errors。
        // 重命名分支已在 match 内部完成 key 平移；新建分支在此统一 revalidate。
        // 临时 set_active 跑 revalidate（active scope），结束后恢复。
        match action {
            ProjectAction::NewGroup { .. } => {}
            ProjectAction::NewTable { project_id, group, name }
            | ProjectAction::NewConstant { project_id, group, name }
            | ProjectAction::NewEnum { project_id, group, name } => {
                let prev = self.active_idx;
                if self.set_active_by_id(project_id) {
                    self.revalidate(group, name);
                    self.active_idx = prev;
                }
            }
            _ => {}
        }
    }

    /// `RenameProject` 的实际逻辑（拆出便于阅读）。
    fn execute_rename_project(&mut self, old_id: &str, new_id: &str, new_name: &str) {
        if old_id == new_id && new_name == self.find_project(old_id).map(|p| p.schema.meta.name.as_str()).unwrap_or("") {
            return;
        }
        let Some(idx) = self.projects.iter().position(|p| p.schema.meta.id == old_id) else {
            self.log(format!("Project 不存在: {}", old_id));
            return;
        };
        let project = &mut self.projects[idx];
        let only_name_change = old_id == new_id;
        if !only_name_change {
            // 检查 new_id 不存在（含 closed 的 available）
            if self.available_projects.iter().any(|a| a.id == new_id) {
                self.log(format!("Project id 已存在: {}", new_id));
                return;
            }
            let project = &mut self.projects[idx];
            let old_root = project.project_root.clone();
            let new_root = project.workdir.join(crate::project::PROJECTS_DIR).join(new_id);
            if let Err(e) = std::fs::rename(&old_root, &new_root) {
                self.log(format!("重命名 project 目录失败: {}", e));
                return;
            }
            project.project_root = new_root.clone();
            project.schema.meta.id = new_id.to_string();
            project.schema.meta.name = new_name.to_string();
            // 同步所有 group dir + 节点 path
            let new_data_dir = project.data_dir();
            for g in &mut project.groups {
                let group_name = g.name.clone();
                g.dir = new_data_dir.join(&group_name);
                for t in &mut g.tables { t.path = g.dir.join(format!("{}.tbl", t.name)); }
                for c in &mut g.constants { c.path = g.dir.join(format!("{}.tbl", c.name)); }
                for e in &mut g.enums { e.path = g.dir.join(format!("{}.tbl", e.name)); }
            }
        } else {
            project.schema.meta.name = new_name.to_string();
        }
        // 写 project.tblschema：项目身份归 schema.meta
        let project = &mut self.projects[idx];
        project.schema_dirty = true;
        let schema_path = project.project_root.join(crate::project::PROJECT_SCHEMA_FILE);
        let txt = crate::tblschema::serialize_tblschema(&project.schema);
        if let Err(e) = std::fs::write(&schema_path, txt) {
            self.log(format!("写 project.tblschema 失败: {}", e));
        } else {
            project.schema_dirty = false;
        }
        // 同步 available_projects（id / name / root）
        if let Some(ap) = self.available_projects.iter_mut().find(|a| a.id == old_id) {
            ap.id = new_id.to_string();
            ap.name = if new_name.is_empty() { new_id.to_string() } else { new_name.to_string() };
            ap.root = self.projects[idx].project_root.clone();
        }
        // 迁移 validation_errors 索引：(old_id, ...) → (new_id, ...)
        if !only_name_change {
            let migrated: Vec<_> = self.validation_errors.iter()
                .filter(|(p, _, _, _, _)| p == old_id).cloned().collect();
            for entry in &migrated { self.validation_errors.remove(entry); }
            for (_, g, n, r, c) in migrated {
                self.validation_errors.insert((new_id.to_string(), g, n, r, c));
            }
        }
        self.log(format!("重命名 Project: {} → {} (name={})", old_id, new_id, new_name));
    }

    /// `DeleteProject` 的实际逻辑（拆出便于阅读）。
    /// 用户允许删到 0 个 project（DBeaver-style 全部关闭）。
    fn execute_delete_project(&mut self, project_id: &str) {
        // 找到要删的盘上根 + 是否仅在内存中（root_pending_create）
        let (project_root, in_memory_only) = if let Some(idx) = self.projects.iter().position(|p| p.schema.meta.id == project_id) {
            (self.projects[idx].project_root.clone(), self.projects[idx].root_pending_create)
        } else if let Some(ap) = self.available_projects.iter().find(|a| a.id == project_id) {
            (ap.root.clone(), false)
        } else {
            self.log(format!("Project 不存在: {}", project_id));
            return;
        };

        // 内存项目（还没落盘）：跳过文件系统删除；NotFound 也忽略，按"目标已不在"处理。
        if !in_memory_only && project_root.exists() {
            if let Err(e) = std::fs::remove_dir_all(&project_root) {
                self.log(format!("删除 project 目录失败: {}", e));
                return;
            }
        }

        // 从 opened 移除（如果在）
        if let Some(idx) = self.projects.iter().position(|p| p.schema.meta.id == project_id) {
            self.projects.remove(idx);
            // active_idx Option 维护
            match self.active_idx {
                Some(active) if active == idx => self.active_idx = None,
                Some(active) if active > idx => self.active_idx = Some(active - 1),
                _ => {}
            }
            if self.projects.is_empty() {
                self.active_idx = None;
            }
        }
        // 从 available 移除
        self.available_projects.retain(|a| a.id != project_id);
        // 清掉 validation_errors
        self.validation_errors.retain(|(p, _, _, _, _)| p != project_id);
        self.log(format!("已删除 Project: {}", project_id));
    }

    /// 该 Project 是否有未保存改动。
    pub fn is_project_dirty(&self, project_id: &str) -> bool {
        let Some(project) = self.find_project(project_id) else { return false; };
        // 内存模式新建 / 克隆出来的项目：root 还没落盘 / schema 还没落盘 → 一定 dirty。
        // 即便 groups 为空也算（空 Empty 项目也不能"关掉就没了"）
        if project.root_pending_create || project.schema_dirty { return true; }
        for g in &project.groups {
            if g.is_new { return true; }
            for t in &g.tables { if t.dirty || t.deleted { return true; } }
            for c in &g.constants { if c.dirty || c.deleted { return true; } }
            for e in &g.enums { if e.dirty || e.deleted { return true; } }
        }
        false
    }

    /// 校验 rename project 时新 id 合法性。返回 None 表示通过。
    /// 检查范围：所有 available（即盘上 projects/<id>/），而非仅已打开。
    pub fn validate_project_id_rename(&self, new_id: &str, old_id: &str) -> Option<String> {
        if new_id.is_empty() { return Some("Project id 不能为空".to_string()); }
        if !crate::tblschema::is_valid_metadata_id(new_id) {
            return Some("Project id 必须是 [a-z0-9_-]，长度 1-32".to_string());
        }
        if new_id != old_id && self.available_projects.iter().any(|a| a.id == new_id) {
            return Some(format!("Project id 已存在: {}", new_id));
        }
        None
    }

    /// 保存指定 Project 的所有改动；其它 Project 不动。
    /// 不通过验证则放弃保存。
    pub fn save_project(&mut self, project_id: &str) {
        // 先全量重算（active scope 不够，因为我们改的 project 不一定是 active）
        self.revalidate_all_projects();
        let pid_errs = self.validation_errors.iter()
            .filter(|(p, _, _, _, _)| p == project_id).count();
        if pid_errs > 0 {
            let errors = self.validate_project(project_id);
            for e in &errors { self.logs.push(e.clone()); }
            self.logs.push(format!("[{}] [保存失败] 共 {} 个验证错误", project_id, pid_errs));
            return;
        }
        let Some(project) = self.find_project_mut(project_id) else {
            self.log(format!("Project 不存在: {}", project_id));
            return;
        };
        let (count, deleted) = save_project_files(project);
        if count > 0 || deleted > 0 {
            self.log(format!("[{}] 已保存 {} 个文件, 删除 {} 个", project_id, count, deleted));
        } else {
            self.log(format!("[{}] 无修改需要保存", project_id));
        }
    }

    /// 收集指定 Project 全部验证错误的格式化日志（仅打日志，不入索引）。
    pub fn validate_project(&self, project_id: &str) -> Vec<String> {
        let Some(project) = self.find_project(project_id) else { return Vec::new(); };
        let mut errors = Vec::new();
        let sep = &project.config.separators;
        let refs = RefIndex::build(&project.groups);
        let allow_ref = project.config.ui.as_ref()
            .map_or(true, |u| u.constant_ref_allowed);
        for group in &project.groups {
            for table in &group.tables {
                if table.deleted { continue; }
                for err in validate_table(table, sep, Some(&refs)) {
                    errors.push(format_validation_log(&group.name, &table.name, &err));
                }
            }
            for constant in &group.constants {
                if constant.deleted { continue; }
                for err in validate_constant(constant, sep, allow_ref, Some(&refs)) {
                    errors.push(format_validation_log(&group.name, &constant.name, &err));
                }
            }
            for enum_def in &group.enums {
                if enum_def.deleted { continue; }
                for err in validate_enum(enum_def) {
                    errors.push(format_validation_log(&group.name, &enum_def.name, &err));
                }
            }
        }
        errors
    }

    pub fn generate_test_config(&mut self) {
        let config_dir = self.project().data_dir();
        let opts = crate::test_util::TestGenOptions::full();
        crate::test_util::generate_test_config(&config_dir, &opts);
        self.log("已生成测试配置文件".to_string());
        self.reload();
    }

    pub fn clear_all_config(&mut self) {
        let config_dir = self.project().data_dir();
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
        if self.project().groups.iter().any(|g| g.name.to_lowercase() == lower) {
            return Some("组名重复（忽略大小写）".to_string());
        }
        None
    }

    pub fn validate_group_name_rename(&self, name: &str, old_name: &str) -> Option<String> {
        if name.is_empty() { return Some("名称不能为空".to_string()); }
        if !is_valid_group_name(name) { return Some("组名只能包含中英文数字下划线".to_string()); }
        let lower = name.to_lowercase();
        if self.project().groups.iter().any(|g| g.name.to_lowercase() == lower && g.name != old_name) {
            return Some("组名重复（忽略大小写）".to_string());
        }
        None
    }

    pub fn validate_node_name(&self, name: &str) -> Option<String> {
        if name.is_empty() { return Some("名称不能为空".to_string()); }
        if !is_valid_node_name(name) { return Some("配置项名必须符合Java类名规则(大写开头,英文数字下划线)".to_string()); }
        let lower = name.to_lowercase();
        for g in &self.project().groups {
            for t in &g.tables {
                if !t.deleted && t.name.to_lowercase() == lower { return Some("配置项名重复（忽略大小写）".to_string()); }
            }
            for c in &g.constants {
                if !c.deleted && c.name.to_lowercase() == lower { return Some("配置项名重复（忽略大小写）".to_string()); }
            }
            for e in &g.enums {
                if !e.deleted && e.name.to_lowercase() == lower { return Some("配置项名重复（忽略大小写）".to_string()); }
            }
        }
        None
    }

    pub fn validate_node_name_rename(&self, name: &str, old_name: &str) -> Option<String> {
        if name.is_empty() { return Some("名称不能为空".to_string()); }
        if !is_valid_node_name(name) { return Some("配置项名必须符合Java类名规则(大写开头,英文数字下划线)".to_string()); }
        let lower = name.to_lowercase();
        for g in &self.project().groups {
            for t in &g.tables {
                if !t.deleted && t.name.to_lowercase() == lower && t.name != old_name { return Some("配置项名重复（忽略大小写）".to_string()); }
            }
            for c in &g.constants {
                if !c.deleted && c.name.to_lowercase() == lower && c.name != old_name { return Some("配置项名重复（忽略大小写）".to_string()); }
            }
            for e in &g.enums {
                if !e.deleted && e.name.to_lowercase() == lower && e.name != old_name { return Some("配置项名重复（忽略大小写）".to_string()); }
            }
        }
        None
    }

    pub fn export_json(&mut self) -> anyhow::Result<crate::export::ExportResult> {
        let result = crate::export::export_all_json(self.project())?;
        self.log_export("JSON", &result);
        Ok(result)
    }

    pub fn export_xml(&mut self) -> anyhow::Result<crate::export::ExportResult> {
        let result = crate::export::export_all_xml(self.project())?;
        self.log_export("XML", &result);
        Ok(result)
    }

    pub fn export_java(&mut self) -> anyhow::Result<crate::export::ExportResult> {
        let result = crate::export::export_all_java(self.project())?;
        self.log_export("Java", &result);
        Ok(result)
    }

    pub fn export_lua(&mut self) -> anyhow::Result<crate::export::ExportResult> {
        let result = crate::export::export_all_lua(self.project())?;
        self.log_export("Lua", &result);
        Ok(result)
    }

    pub fn export_go(&mut self) -> anyhow::Result<crate::export::ExportResult> {
        let result = crate::export::export_all_go(self.project())?;
        self.log_export("Go", &result);
        Ok(result)
    }

    pub fn export_gdscript(&mut self) -> anyhow::Result<crate::export::ExportResult> {
        let result = crate::export::export_all_gdscript(self.project())?;
        self.log_export("GDScript", &result);
        Ok(result)
    }

    /// 导出指定 Project（按 id）。给"右键此 project 导出"用。
    pub fn export_project<F>(&mut self, project_id: &str, f: F, label: &str)
        -> anyhow::Result<crate::export::ExportResult>
    where
        F: FnOnce(&Project) -> anyhow::Result<crate::export::ExportResult>,
    {
        let p = self.find_project(project_id)
            .ok_or_else(|| anyhow::anyhow!("Project 不存在: {}", project_id))?;
        let result = f(p)?;
        self.log_export(label, &result);
        Ok(result)
    }

    /// 把 active project 当前临时切换到指定 id 跑闭包，结束后恢复。
    /// 给"导出全部 project / 保存全部 project"这类需要遍历所有 project 但
    /// 现有 export_* 接口已绑定 active 的场景用。
    pub fn with_active<R>(&mut self, project_id: &str, f: impl FnOnce(&mut Self) -> R) -> Option<R> {
        let idx = self.projects.iter().position(|p| p.schema.meta.id == project_id)?;
        let prev = self.active_idx;
        self.active_idx = Some(idx);
        let r = f(self);
        self.active_idx = prev;
        Some(r)
    }

    fn log_export(&mut self, label: &str, result: &crate::export::ExportResult) {
        use crate::export::FileStatus;
        self.log(format!("[{}] {} 新增, {} 修改, {} 删除, {} 不变",
            label, result.added(), result.modified(), result.deleted(), result.unchanged()));
        for f in &result.files {
            match f.status {
                FileStatus::Added => self.log(format!("  [新增] {}", f.path)),
                FileStatus::Modified => self.log(format!("  [修改] {}", f.path)),
                FileStatus::Deleted => self.log(format!("  [删除] {}", f.path)),
                FileStatus::Unchanged => {}
            }
        }
    }
}
