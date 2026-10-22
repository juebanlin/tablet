use std::collections::HashSet;
use std::path::{Path, PathBuf};
use crate::model::*;
use crate::tbl::DEFAULT_TBL_VERSION;
use crate::validate::*;

/// 把一条 ValidationError 格式化为日志行（统一 B 格式：`位置:[内容] -> 原因`）：
/// - Table 表头：`[验证] group/node 表头第N行X列:[field] -> message`
///   N = 1 desc / 2 export / 3 type / 4 field（与 UI 表头从上到下一致）
/// - Constant/Enum 表头：`[验证] group/node 表头X列:[field] -> message`
/// - 数据行：`[验证] group/node X<row>:[value] -> message`，value 过长截断为 `xxx...`
pub fn format_validation_log(group: &str, node: &str, err: &ValidationError) -> String {
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
///
/// 这里不碰任何"脏"状态：脏由每层自己跟盘上快照比出来（@02.26.2），
/// 改名改回原名后会自动回到干净。
fn sync_schema_from_groups(project: &mut Project) {
    // sync 永远不带 preset：project.tblschema 是「结构骨架」，行数据归项目里的 .tbl
    project.resync_schema();
}

/// 把类型串里的 `@Old` 改写成 `@New`；没有命中返回 `None`。
///
/// 按**名字边界**匹配：`@Hero` 不能命中 `@HeroBase`。做法是看 `@Hero` 之后那个
/// 字符——还是字母/数字/下划线就说明这是另一个名字，放过去。
///
/// 只做替换，不 parse 再 `to_type_string()` 重排：后者会把 `List< @Hero >`
/// 这类手写空格一并规整掉，等于顺手改了与自己无关的列。
fn rename_ref_in_type(type_str: &str, old: &str, new: &str) -> Option<String> {
    let needle = format!("@{}", old);
    let mut out = String::with_capacity(type_str.len());
    let mut rest = type_str;
    let mut changed = false;
    while let Some(pos) = rest.find(&needle) {
        let after = pos + needle.len();
        let is_boundary = rest[after..]
            .chars()
            .next()
            .map_or(true, |c| !(c.is_ascii_alphanumeric() || c == '_'));
        out.push_str(&rest[..pos]);
        if is_boundary {
            out.push('@');
            out.push_str(new);
            changed = true;
        } else {
            out.push_str(&needle);
        }
        rest = &rest[after..];
    }
    out.push_str(rest);
    if changed { Some(out) } else { None }
}

/// 节点改名后，把**同项目内**所有 `@OldName` 类型引用改写成 `@NewName`；返回改写处数。
///
/// 引用只在本项目内解析（`RefIndex::build` 只看 `project.groups`，跨 project 的
/// 引用不存在），所以扫一遍本项目就是全集。
///
/// 只动**类型串**：引用值本身是 id、不是名字，数据行一格都不用碰。
///
/// 为什么必须改：`HeroBase` 有一列 `type:@HeroBase`（自引用）这种很常见，不改的话
/// 改名当场让项目变成非法 → 保存被验证闸门拦下 → **改名永远落不了盘**。
/// @01.8.5 早就写着改名"给警告、不阻断保存"，把引用一起带过去比给警告更干净。
fn rewrite_refs_after_rename(project: &mut Project, old: &str, new: &str) -> usize {
    let mut n = 0;
    for g in &mut project.groups {
        for t in &mut g.tables {
            for f in &mut t.schema.fields {
                if let Some(s) = rename_ref_in_type(&f.tbl_type, old, new) {
                    f.tbl_type = s;
                    n += 1;
                }
            }
            // 类型串变了 → original_fields 对不上 → 该表自己要写盘
            t.update_dirty();
        }
        for c in &mut g.constants {
            for e in &mut c.entries {
                if let Some(s) = rename_ref_in_type(&e.tbl_type, old, new) {
                    e.tbl_type = s;
                    n += 1;
                }
            }
            c.update_dirty();
        }
    }
    n
}

/// 保存前把「改了名但还没落盘」的组目录 / `.tbl` 文件在盘上搬到位。
///
/// 改名是唯一一个曾经「保存前就碰文件系统」的操作（撤销也要它让路），所以现在
/// 改名只写内存——`name` 变了、`original_name` 不动，搬文件推迟到这里。
///
/// 旧位置由「上次落盘时的名字」推出来，于是两种难缠的情况都自然成立：
/// - **连续改名** A→B→C：`original_name` 始终是 `A`，盘上确实还是 `A.tbl`
/// - **父子同时改名**：组记组的原名、节点记节点的原名，两级各推各的，拼起来就对了
///
/// 顺序是**先组后子**：组目录整个搬走时组内文件跟着走，叶子名没变的子节点到
/// 这一步就已经到位了。
///
/// 搬不动的（文件被占用 / 没权限）就**把内存退回去**，宁可这次改名不生效，也不要
/// 内存说叫 B、盘上还叫 A。退回去这件事通过返回值报到 UI 日志，不吞。
fn apply_pending_renames(project: &mut Project) -> Vec<String> {
    let mut warnings = Vec::new();
    let data_dir = project.data_dir();
    for group in &mut project.groups {
        // ── 组目录 ──
        if group.original_name != group.name {
            let old_dir = data_dir.join(&group.original_name);
            let new_dir = data_dir.join(&group.name);
            // 未落过盘的组（is_new）盘上没有旧目录 → 直接认下新名
            let moved = if old_dir.is_dir() {
                match std::fs::rename(&old_dir, &new_dir) {
                    Ok(()) => true,
                    Err(e) => {
                        warnings.push(format!(
                            "重命名组 {} → {} 失败，保留原名: {}",
                            group.original_name, group.name, e
                        ));
                        false
                    }
                }
            } else {
                true
            };
            if moved {
                group.dir = new_dir;
                group.original_name = group.name.clone();
            } else {
                // 组名退回旧值。**不跳过组内节点**——组改名失败不该连累节点自己的
                // 改名，下面那一轮按退回后的 `group.dir` 继续走。
                group.name = group.original_name.clone();
                group.dir = old_dir;
            }
        }
        // 组目录定下来后重算子节点 path：`dir` 变了，叶子名没变的也得跟着改
        let dir = group.dir.clone();
        for t in &mut group.tables { t.path = dir.join(format!("{}.tbl", t.name)); }
        for c in &mut group.constants { c.path = dir.join(format!("{}.tbl", c.name)); }
        for e in &mut group.enums { e.path = dir.join(format!("{}.tbl", e.name)); }

        // ── 叶子文件 ──
        for t in &mut group.tables {
            if let Some(w) = rename_tbl_file(&dir, &mut t.name, &mut t.original_name, &mut t.path) {
                warnings.push(w);
            }
        }
        for c in &mut group.constants {
            if let Some(w) = rename_tbl_file(&dir, &mut c.name, &mut c.original_name, &mut c.path) {
                warnings.push(w);
            }
        }
        for e in &mut group.enums {
            if let Some(w) = rename_tbl_file(&dir, &mut e.name, &mut e.original_name, &mut e.path) {
                warnings.push(w);
            }
        }
    }
    warnings
}

/// 把单个 `.tbl` 从 `original_name` 搬到 `name`（同一组目录内）。
///
/// `original_name == name` 时**什么也不做**——包括不重算 `path`：组目录刚刚搬过时
/// 调用方已经按新目录重算好了，这里再算一次是多余的。
/// 盘上没有旧文件（新节点）时只更新内存。
/// 失败时把 `name` / `path` 退回旧值并返回一条警告。
fn rename_tbl_file(
    dir: &Path,
    name: &mut String,
    original_name: &mut String,
    path: &mut PathBuf,
) -> Option<String> {
    if *original_name == *name { return None; }
    let old_path = dir.join(format!("{}.tbl", original_name));
    let new_path = dir.join(format!("{}.tbl", name));
    if old_path.is_file() {
        if let Err(e) = std::fs::rename(&old_path, &new_path) {
            let w = format!("重命名 {} 失败，保留原名: {}", old_path.display(), e);
            *name = original_name.clone();
            *path = old_path;
            return Some(w);
        }
    }
    *original_name = name.clone();
    *path = new_path;
    None
}

/// 保存单个 Project 的所有 dirty / deleted 节点；返回 (保存数, 删除数, 警告)。
/// 抽出来给 save_all（active 一份）和 save_all_projects（遍历）共用。
///
/// 项目级落盘（root 目录创建 / .tblschema 写入）算 1 次保存动作，
/// 让"空项目（仅 meta，无 group）"首次保存的日志为"已保存 1 个文件"而不是"无修改"。
fn save_project_files(project: &mut Project) -> (usize, usize, Vec<String>) {
    let mut count = 0;
    let mut deleted = 0;
    // 第一次落盘：盘上还什么都没有，schema 必须无条件写一份
    let first_save = project.state.is_pending();
    // 第一步：克隆出来但未落盘的项目，先建 root 目录 + 让 schema 一并落地
    if first_save {
        if let Err(e) = std::fs::create_dir_all(&project.project_root) {
            eprintln!("warn: create project root failed: {} ({})", project.project_root.display(), e);
            return (0, 0, Vec::new());
        }
        project.state = crate::model::ProjectState::Loaded;

        // 未落盘项目首次保存时，根据当前 project_root 重新推演所有路径
        // （应对"项目设置"修改 id 的场景：id 改了、project_root 更新了，但子节点路径还是旧的）
        let new_data_dir = project.data_dir();
        for group in &mut project.groups {
            group.dir = new_data_dir.join(&group.name);
            for table in &mut group.tables {
                table.path = group.dir.join(format!("{}.tbl", table.name));
            }
            for constant in &mut group.constants {
                constant.path = group.dir.join(format!("{}.tbl", constant.name));
            }
            for enum_def in &mut group.enums {
                enum_def.path = group.dir.join(format!("{}.tbl", enum_def.name));
            }
        }
    }
    // `project.toml` 是**可选的 `[export]` 覆盖段**：只在用户真的改过 export 时才写。
    // 没改过就不该有文件——export 继续从工作区 `tablet.toml` 继承，改全局能传播。
    // 写的内容是项目**当前的** `config.export`（曾经这里写的是 `ExportConfig::default()`，
    // 于是"新建项目首次保存"会把从全局拷来的配置静默重置掉，@02.26.3）。
    if project.toml_changed() {
        let path = project.project_root.join(crate::project::PROJECT_TOML_FILE);
        match crate::project::project_toml_text(&project.config.export) {
            Ok(txt) => match std::fs::write(&path, txt) {
                Ok(()) => {
                    project.original_export = project.config.export.clone();
                    count += 1;
                }
                Err(e) => eprintln!(
                    "warn: write project.toml failed: {} ({})",
                    path.display(),
                    e
                ),
            },
            Err(e) => eprintln!(
                "warn: 生成 project.toml 内容失败: {} ({})",
                path.display(),
                e
            ),
        }
    }
    // schema 文件只在**真的会不一样**时才写：改一个格子的值不该动它。
    if first_save || project.schema_changed() {
        project.resync_schema();
        let txt = crate::tblschema::serialize_tblschema(&project.schema);
        let path = project.project_root.join(crate::project::PROJECT_SCHEMA_FILE);
        if std::fs::write(&path, txt).is_ok() {
            // 落盘后「盘上是什么」才追上内存
            project.original_meta = project.schema.meta.clone();
            project.original_separators = project.schema.separators.clone();
            count += 1;
        }
    }
    // 改名先落位（盘上搬完），再走下面的写入循环——两边都靠 original_name 对齐
    let warnings = apply_pending_renames(project);
    for group in &mut project.groups {
        if group.is_new {
            let _ = std::fs::create_dir_all(&group.dir);
            group.is_new = false;
        }
        for table in &mut group.tables {
            if table.deleted {
                if table.saved {
                    let _ = std::fs::remove_file(&table.path);
                }
                deleted += 1;
            } else if table.dirty {
                let content = crate::tbl::serialize_table(table);
                if std::fs::write(&table.path, &content).is_ok() {
                    table.original_records = table.records.clone();
                    table.original_fields = table.schema.fields.clone();
                    // 落盘后「盘上叫什么」才追上新名字
                    table.original_name = table.name.clone();
                    table.dirty = false;
                    table.saved = true;
                    count += 1;
                }
            }
        }
        for constant in &mut group.constants {
            if constant.deleted {
                if constant.saved {
                    let _ = std::fs::remove_file(&constant.path);
                }
                deleted += 1;
            } else if constant.dirty {
                let content = crate::tbl::serialize_constant(constant);
                if std::fs::write(&constant.path, &content).is_ok() {
                    constant.original_entries = constant.entries.clone();
                    constant.original_name = constant.name.clone();
                    constant.dirty = false;
                    constant.saved = true;
                    count += 1;
                }
            }
        }
        for enum_def in &mut group.enums {
            if enum_def.deleted {
                if enum_def.saved {
                    let _ = std::fs::remove_file(&enum_def.path);
                }
                deleted += 1;
            } else if enum_def.dirty {
                let content = crate::tbl::serialize_enum(enum_def);
                if std::fs::write(&enum_def.path, &content).is_ok() {
                    enum_def.original_entries = enum_def.entries.clone();
                    enum_def.original_name = enum_def.name.clone();
                    enum_def.dirty = false;
                    enum_def.saved = true;
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
    (count, deleted, warnings)
}

pub struct ProjectEngine {
    /// 顶层 workdir（@04.2.0 多 Project 同时管理）。所有 project 共用。
    pub workdir: PathBuf,
    /// 全局配置（来自 tablet.toml），所有 Project 共享。
    pub global_config: GlobalConfig,
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
    /// 启动时从 workspace `tablet.toml [separators]` 读到的"程序级默认分隔符"；
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
    /// 删除 project：rm -rf `projects/<id>/`、从内存 `Vec<Project>` 移除。
    /// 至少要保留一个 project；删最后一个会失败（log 报错）。
    DeleteProject { project_id: String },
}

impl ProjectEngine {
    /// 构造单个 Project 实例（用于测试和简单场景）。
    /// available_projects 仅含该 project；workdir 取自该 project。
    ///
    /// `global_config` 参数：传入真实的全局配置，避免从已 merge 的 project.config 反推。
    /// 测试场景可传 `GlobalConfig::default()`。
    pub fn new(project: Project, global_config: GlobalConfig) -> Self {
        let workdir = project.workdir.clone();
        let available = vec![AvailableProject::from_project(&project)];
        let mut global_config = global_config;
        // 更新 project_management 段以匹配当前 project
        global_config.project_management.last_project = project.schema.meta.id.clone();
        global_config.project_management.opened_projects = vec![project.schema.meta.id.clone()];
        Self {
            workdir,
            global_config,
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
    ///
    /// `global_config` 参数：传入真实的全局配置，避免从已 merge 的 project.config 反推。
    pub fn new_multi(projects: Vec<Project>, last_id: Option<&str>, global_config: GlobalConfig) -> Self {
        assert!(!projects.is_empty(), "ProjectEngine::new_multi 至少要有一个 Project");
        let workdir = projects[0].workdir.clone();
        let available = projects.iter().map(AvailableProject::from_project).collect();
        let active_idx = last_id
            .and_then(|id| projects.iter().position(|p| p.schema.meta.id == id))
            .or(Some(0));
        let mut global_config = global_config;
        // 更新 project_management 段以匹配当前打开的 projects
        global_config.project_management.last_project = last_id.unwrap_or("").to_string();
        global_config.project_management.opened_projects = projects.iter().map(|p| p.schema.meta.id.clone()).collect();
        Self {
            workdir,
            global_config,
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
        global_config: GlobalConfig,
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
            global_config,
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

    /// （已废弃）原用于重新合并配置，现在项目配置独立维护，不再需要合并。
    /// 保留空实现以兼容旧调用点，未来可删除。
    pub fn remerge_project_config(&mut self, _project_id: &str) {
        // 不再需要合并：项目配置独立维护
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
            self.error_log(format!("Project 未打开: {}", project_id));
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

    /// 内存模式创建新 Project：与 clone_project_in_memory 行为对齐。
    /// - 不立即落盘；state=Pending
    /// - sections 展开为 group.is_new=true、节点 original="" → 树面板 `+` 标记
    /// - config 借用现有任意 opened project 的 ProjectConfig，否则用默认 fallback
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

        // 从全局配置拷贝初始配置（新项目时的种子）
        let config = ProjectConfig {
            export: self.global_config.export.clone().unwrap_or_default(),
        };
        let config_export = config.export.clone();

        let mut new_project = Project {
            workdir: self.workdir.clone(),
            project_root: project_root.clone(),
            config,
            schema: schema.clone(),
            // 还没落盘 → 「盘上」是空的
            original_meta: crate::tblschema::SchemaMetadata::default(),
            original_separators: Default::default(),
            // 盘上没有 project.toml，而加载缺省时 export 是从全局继承的 → 现值就是"盘上值"
            original_export: config_export,
            groups: Vec::new(),
            state: crate::model::ProjectState::Pending,
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

        // 清除 schema 的 preset（数据已灌入 groups，项目 schema 只保留结构骨架）
        for sec in &mut new_project.schema.sections {
            sec.preset.clear();
        }

        let avail = AvailableProject::from_project(&new_project);
        self.available_projects.push(avail);
        self.available_projects.sort_by(|a, b| a.id.cmp(&b.id));
        self.projects.push(new_project);
        Ok(project_id)
    }

    /// 从 schema 落地一个新 Project：内存态建好（L2）→ 过验证闸门 → 立刻保存（L1）。
    ///
    /// **「立即落盘」不是另一种实现，它就是 L2 加一次保存。** 和界面从模板 / 文件
    /// 新建走的是同一条 `create_project_in_memory_with`，差别只在界面停在内存态等
    /// 用户点保存（@02.26.3 那条分界）。
    ///
    /// 闸门（`validate_project`）不过就**整个撤回**并把错误原样返回——不留一个落不了盘
    /// 的空壳在 `projects` / `available_projects` 里，也让调用方能如实报出来。
    pub fn create_project_from_schema(
        &mut self,
        schema: crate::tblschema::TblSchema,
    ) -> Result<String, String> {
        let with_preset = schema.meta.has_preset;
        let id = self.create_project_in_memory_with(schema, with_preset)?;

        let errs = self.validate_project(&id);
        if !errs.is_empty() {
            self.close_project(&id);
            self.available_projects.retain(|a| a.id != id);
            return Err(format!("schema 校验未通过，未创建项目:\n{}", errs.join("\n")));
        }

        self.save_project(&id);
        Ok(id)
    }

    /// 内存深拷贝克隆：把 source_project_id 的当前状态（含未保存改动）整体复制为新 project，
    /// 加进 self.projects 但**不立即落盘**（state=Pending，
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
            self.error_log(format!("project id 不合法: {}", new_id));
            return None;
        }
        if self.available_projects.iter().any(|a| a.id == new_id) {
            self.error_log(format!("project id 已存在: {}", new_id));
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
        // 克隆体的「盘上」是空的，不能继承源项目盘上的那份
        new_project.original_meta = crate::tblschema::SchemaMetadata::default();
        new_project.original_separators = Default::default();
        new_project.original_export = new_project.config.export.clone();
        new_project.state = crate::model::ProjectState::Pending;

        let new_data_dir = new_project.data_dir();
        for g in &mut new_project.groups {
            g.dir = new_data_dir.join(&g.name);
            g.original_name = g.name.clone();
            g.is_new = true;
            for t in &mut g.tables {
                t.path = g.dir.join(format!("{}.tbl", t.name));
                t.dirty = true;
                t.original_records.clear();
                t.original_fields.clear();
                t.saved = false;
                t.original_name = t.name.clone();
            }
            for c in &mut g.constants {
                c.path = g.dir.join(format!("{}.tbl", c.name));
                c.dirty = true;
                c.original_entries.clear(); c.saved = false;
                c.original_name = c.name.clone();
            }
            for e in &mut g.enums {
                e.path = g.dir.join(format!("{}.tbl", e.name));
                e.dirty = true;
                e.original_entries.clear(); e.saved = false;
                e.original_name = e.name.clone();
            }
        }
        // 同步 schema.sections 与 groups（克隆下来的 sections 已经一致，但保险起见走一次）
        sync_schema_from_groups(&mut new_project);
        let avail = AvailableProject::from_project(&new_project);
        self.available_projects.push(avail);
        self.available_projects.sort_by(|a, b| a.id.cmp(&b.id));
        self.projects.push(new_project);
        self.ui_log(format!(
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

    /// 操作日志：显示在 UI 面板 + 写文件（info 级别）
    pub fn ui_log(&mut self, msg: String) {
        let now = chrono::Local::now().format("%H:%M:%S").to_string();
        self.logs.push(format!("{} {}", now, msg));
        log::info!("{}", msg);
    }

    /// 调试日志：只写文件（debug 级别），不显示 UI
    pub fn debug_log(&mut self, msg: String) {
        log::debug!("{}", msg);
    }

    /// 警告日志：显示在 UI 面板（带 ⚠️）+ 写文件（warn 级别）
    pub fn warn_log(&mut self, msg: String) {
        let now = chrono::Local::now().format("%H:%M:%S").to_string();
        self.logs.push(format!("{} ⚠️ {}", now, msg));
        log::warn!("{}", msg);
    }

    /// 错误日志：显示在 UI 面板（带 ❌）+ 写文件（error 级别）
    pub fn error_log(&mut self, msg: String) {
        let now = chrono::Local::now().format("%H:%M:%S").to_string();
        self.logs.push(format!("{} ❌ {}", now, msg));
        log::error!("{}", msg);
    }

    /// 旧接口兼容：默认为 ui_log
    #[deprecated(note = "使用 ui_log/debug_log/warn_log/error_log 替代")]
    pub fn log(&mut self, msg: String) {
        self.ui_log(msg);
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

    pub fn paste_enum_data(&mut self, group: &str, name: &str, start_row: usize, start_col: usize, grid: &[Vec<String>]) {
        if grid.is_empty() { return; }
        if let Some(g) = self.project_mut().groups.iter_mut().find(|g| g.name == group) {
            if let Some(en) = g.enums.iter_mut().find(|e| e.name == name) {
                for (i, row_cells) in grid.iter().enumerate() {
                    let row = start_row + i;
                    while en.entries.len() <= row {
                        en.entries.push(EnumEntry::default());
                    }
                    for (j, cell) in row_cells.iter().enumerate() {
                        let col = start_col + j;
                        let entry = &mut en.entries[row];
                        match col {
                            0 => entry.id = cell.clone(),
                            1 => entry.name = cell.clone(),
                            2 => entry.desc = cell.clone(),
                            _ => {}
                        }
                    }
                }
                en.update_dirty();
            }
        }
        self.ui_log(format!("粘贴 {}行 数据", grid.len()));
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
                    self.ui_log(format!("已删除 {} 行", end - start));
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
                        3 => entry.export = Export::from_input(&val),
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
                        1 => field.export = Export::from_input(&val),
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
        self.project_mut().resync_schema();
        if let Some(kw) = keyword_err {
            self.ui_log(format!("字段名 '{}' 是保留关键字，不允许使用", kw));
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
                    self.ui_log(format!("已删除 {} 行", end - start));
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
        self.project_mut().resync_schema();
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
        self.project_mut().resync_schema();
    }

    pub fn paste_table_data(&mut self, group: &str, name: &str, start_row: usize, start_col: usize, grid: &[Vec<String>]) {
        if grid.is_empty() { return; }
        if let Some(g) = self.project_mut().groups.iter_mut().find(|g| g.name == group) {
            if let Some(t) = g.tables.iter_mut().find(|t| t.name == name) {
                let cols = t.schema.fields.len();
                for (i, row_cells) in grid.iter().enumerate() {
                    let row = start_row + i;
                    while t.records.len() <= row { t.records.push(vec![String::new(); cols]); }
                    for (j, cell) in row_cells.iter().enumerate() {
                        let col = start_col + j;
                        if col < cols {
                            let record = &mut t.records[row];
                            while record.len() <= col { record.push(String::new()); }
                            record[col] = cell.clone();
                        }
                    }
                }
                t.update_dirty();
            }
        }
        self.ui_log(format!("粘贴 {}行 数据", grid.len()));
    }

    pub fn paste_constant_data(&mut self, group: &str, name: &str, start_row: usize, start_col: usize, grid: &[Vec<String>]) {
        if grid.is_empty() { return; }
        if let Some(g) = self.project_mut().groups.iter_mut().find(|g| g.name == group) {
            if let Some(c) = g.constants.iter_mut().find(|c| c.name == name) {
                for (i, row_cells) in grid.iter().enumerate() {
                    let row = start_row + i;
                    while c.entries.len() <= row {
                        c.entries.push(ConstEntry {
                            name: String::new(), tbl_type: "str".to_string(),
                            value: String::new(), export: Export::ClientServer, desc: String::new(),
                        });
                    }
                    for (j, cell) in row_cells.iter().enumerate() {
                        let col = start_col + j;
                        let entry = &mut c.entries[row];
                        match col {
                            0 => entry.name = cell.clone(),
                            1 => entry.tbl_type = cell.clone(),
                            2 => entry.value = cell.clone(),
                            3 => entry.export = Export::from_input(cell),
                            4 => entry.desc = cell.clone(),
                            _ => {}
                        }
                    }
                }
                c.update_dirty();
            }
        }
        self.ui_log(format!("粘贴 {}行 数据", grid.len()));
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
                            3 => entry.export = Export::ClientServer,
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

    /// 结构化校验结果：`(group, node, 错误)`。
    ///
    /// 与 [`validate`](Self::validate) 跑同一套校验，只是不预先拼成字符串——
    /// CLI 要拿 `header_row` / `field` / `message` 做定位与过滤，拼字符串的版本拿不到。
    pub fn validate_errors(&self) -> Vec<(String, String, ValidationError)> {
        let mut out = Vec::new();
        let sep = &self.project().schema.separators;
        let refs = RefIndex::build(&self.project().groups);
        let allow_ref = self.global_config.ui.as_ref()
            .map_or(true, |u| u.constant_ref_allowed);
        for group in &self.project().groups {
            for table in &group.tables {
                if table.deleted { continue; }
                for err in validate_table(table, sep, Some(&refs)) {
                    out.push((group.name.clone(), table.name.clone(), err));
                }
            }
            for constant in &group.constants {
                if constant.deleted { continue; }
                for err in validate_constant(constant, sep, allow_ref, Some(&refs)) {
                    out.push((group.name.clone(), constant.name.clone(), err));
                }
            }
            for enum_def in &group.enums {
                if enum_def.deleted { continue; }
                for err in validate_enum(enum_def) {
                    out.push((group.name.clone(), enum_def.name.clone(), err));
                }
            }
        }
        out
    }

    /// 校验并渲染成 GUI 日志同款文本（`format_validation_log`）。
    pub fn validate(&self) -> Vec<String> {
        self.validate_errors().iter()
            .map(|(group, node, err)| format_validation_log(group, node, err))
            .collect()
    }

    pub fn revalidate(&mut self, group: &str, name: &str) {
        let Some(pid) = self.active_project_id().map(str::to_string) else { return; };
        self.validation_errors.retain(|(p, g, n, _, _)| p != &pid || g != group || n != name);
        let sep = self.project().schema.separators.clone();
        let refs = RefIndex::build(&self.project().groups);
        let allow_ref = self.global_config.ui.as_ref()
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
    ///
    /// 返回**是否真的走完了落盘**。调用方如果对外声称"已保存/已改名"，
    /// 必须看这个返回值——验证不过时这里只打日志、什么都不写。
    pub fn save_all(&mut self) -> bool {
        if self.active_idx.is_none() {
            self.ui_log("没有打开的 Project，无需保存".to_string());
            return false;
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
            return false;
        }
        let (count, deleted, warnings) = save_project_files(self.project_mut());
        for w in warnings { self.warn_log(w); }
        if count > 0 || deleted > 0 {
            self.ui_log(format!("已保存 {} 个文件, 删除 {} 个", count, deleted));
        } else {
            self.ui_log("无修改需要保存".to_string());
        }
        true
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
        let mut warnings = Vec::new();
        for project in &mut self.projects {
            let (c, d, w) = save_project_files(project);
            total_count += c;
            total_deleted += d;
            warnings.extend(w);
        }
        for w in warnings { self.warn_log(w); }
        if total_count > 0 || total_deleted > 0 {
            self.ui_log(format!("已保存 {} 个文件, 删除 {} 个（{} 个 Project）",
                total_count, total_deleted, self.projects.len()));
        } else {
            self.ui_log("无修改需要保存".to_string());
        }
    }

    /// 把「当前运行时布局」作为 `[project]` 与 engine 持有的 ui/export/separators 一起
    /// **整写**回 `<workdir>/tablet.toml`（唯一整写入口，见 `project::write_global_config`）。
    /// 会先用运行时重建 `project_management` 并回写内存（内存=盘）。
    /// GUI 在 open/close/sort/delete 与全局设置保存时都走这里。
    pub fn persist_global_config(
        &mut self,
        project_sort: crate::enums::ProjectSort,
        project_order: &[String],
    ) -> anyhow::Result<()> {
        self.global_config.project_management = crate::model::ProjectManagementConfig {
            last_project: self.active_project_id().unwrap_or("").to_string(),
            opened_projects: self.opened_ids(),
            project_sort,
            project_order: project_order.to_vec(),
        };
        crate::project::write_global_config(&self.workdir, &self.global_config)
    }

    pub fn reload(&mut self) {
        let workdir = self.workdir.clone();
        let last_id = self.active_project_id().map(str::to_string);
        match crate::project::load_workspace(&workdir) {
            Ok((mut new_engine, _config_fixed, failed)) => {
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
                // reload 后采纳新解析的全局配置与默认分隔符，避免整写/后续读用 stale 副本
                self.global_config = new_engine.global_config;
                self.default_separators = new_engine.default_separators;
                self.validation_errors.clear();
                self.ui_log(format!("重新加载完成，共 {} 个 Project / {} 个 Group", pcount, group_total));
                // 打不开的项目要吵：reload 是显式动作，静静少一个项目比报错更难查
                for f in &failed {
                    self.error_log(format!("[重新加载] Project {} 打不开: {}", f.id, f.message));
                }
            }
            Err(e) => self.ui_log(format!("加载失败: {}", e)),
        }
    }

    /// 当前 active Project 是否有未保存改动；`is_dirty_any` 跨全部 Project。
    /// 全部关闭时返回 false。
    pub fn is_dirty(&self) -> bool {
        self.active().is_some_and(|p| p.is_dirty())
    }

    /// 任一 Project 有未保存改动。退出工具 / 全保存判空用。
    pub fn is_dirty_any(&self) -> bool {
        self.projects.iter().any(|p| p.is_dirty())
    }

    pub fn delete_group(&mut self, group_name: &str) {
        if let Some(g) = self.project().groups.iter().find(|g| g.name == group_name) {
            if g.is_new {
                self.project_mut().groups.retain(|g| g.name != group_name);
                self.ui_log(format!("已移除新建 Group: {}", group_name));
            } else {
                if let Some(g) = self.project_mut().groups.iter_mut().find(|g| g.name == group_name) {
                    for t in &mut g.tables { t.deleted = true; }
                    for c in &mut g.constants { c.deleted = true; }
                    for e in &mut g.enums { e.deleted = true; }
                }
                self.ui_log(format!("已标记删除 Group: {}", group_name));
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
        self.ui_log(format!("已标记删除: {}/{}", group_name, node_name));
    }

    /// 把某个节点还原成盘上的样子（「还原到已保存」，@02.26）。
    ///
    /// 只管**编辑**（内容 / 结构 / 身份），**不管存在性**：新建未保存的节点压根没有
    /// 盘上的样子可还原，软删的节点还原内容后仍然是删着的。所以「没有可还原的东西」
    /// 不是错误，只是没动作——返回 false，由调用方决定要不要说话。
    ///
    /// 这是「把 `original_*` 写回状态」这条普通路径的入口。将来做连续撤销时，
    /// 它就该变成一次普通的历史记录，不需要另开旁路（@02.26）。
    pub fn revert_node(&mut self, group_name: &str, node_name: &str) -> bool {
        let mut did = false;
        if let Some(g) = self.project_mut().groups.iter_mut().find(|g| g.name == group_name) {
            if let Some(t) = g.tables.iter_mut().find(|t| t.name == node_name) {
                if t.revertable() {
                    t.revert_to_saved();
                    did = true;
                }
            }
            if let Some(c) = g.constants.iter_mut().find(|c| c.name == node_name) {
                if c.revertable() {
                    c.revert_to_saved();
                    did = true;
                }
            }
            if let Some(e) = g.enums.iter_mut().find(|e| e.name == node_name) {
                if e.revertable() {
                    e.revert_to_saved();
                    did = true;
                }
            }
        }
        if !did {
            return false;
        }
        // 表头 / 表名 / # @display 都可能变，schema 投影要重建
        sync_schema_from_groups(self.project_mut());
        // 内容换了，这个节点的校验结果得按新内容重算（revalidate_all 是 active 作用域的，
        // 而 grid 显示的就是 active project 的节点）
        self.revalidate_all();
        self.ui_log(format!("已还原到上次保存: {}/{}", group_name, node_name));
        true
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
            self.ui_log(format!("Project 不存在: {}", project_id));
            return;
        };
        let Some(g) = p.groups.iter().find(|g| g.name == group) else {
            self.ui_log(format!("Group 不存在: {}/{}", project_id, group));
            return;
        };
        let body = match kind {
            NodeKind::Table => g.tables.iter().find(|t| t.name == node_name).cloned().map(NodeBody::Table),
            NodeKind::Constant => g.constants.iter().find(|c| c.name == node_name).cloned().map(NodeBody::Constant),
            NodeKind::Enum => g.enums.iter().find(|e| e.name == node_name).cloned().map(NodeBody::Enum),
        };
        let Some(body) = body else {
            self.ui_log(format!("节点不存在: {}/{}/{}", project_id, group, node_name));
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
        self.ui_log(format!("已复制 {} 到剪贴板: {}/{}/{}", kind_label, project_id, group, node_name));
    }

    /// 整组深拷贝到剪贴板（含组内全部 table/constant/enum）。
    pub fn clipboard_copy_group(&mut self, project_id: &str, group: &str) {
        let Some(p) = self.find_project(project_id) else {
            self.ui_log(format!("Project 不存在: {}", project_id));
            return;
        };
        let Some(g) = p.groups.iter().find(|g| g.name == group) else {
            self.ui_log(format!("Group 不存在: {}/{}", project_id, group));
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
        self.ui_log(format!("已复制 Group 到剪贴板: {}/{}（{}）", project_id, group, summary));
    }

    /// 粘贴单节点到目标 (project, group)。
    /// - 名字冲突走 `_copy` / `_copy2` …
    /// - path 重写到 `<target_group.dir>/<final_name>.tbl`
    /// - dirty=true，original=""，is_new 在 Group 层面已独立标识，节点本身不需要
    /// - 末尾对该节点跑一次 revalidate（active 状态下生效；非 active 暂不需要红框）
    /// 返回新节点名（成功时）。
    pub fn paste_node_to(&mut self, target_project: &str, target_group: &str) -> Option<String> {
        let Some(NodeClipboard::Node { body, .. }) = self.node_clipboard.clone() else {
            self.warn_log("剪贴板为空或类型不匹配（需要节点剪贴板）".to_string());
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
                t.original_records.clear(); t.original_fields.clear(); t.saved = false;
                // 新节点盘上还不存在 → 原名就是新名（不是源节点在盘上的名字）
                t.original_name = final_name.clone();
                g.tables.push(t);
                "Table"
            }
            NodeBody::Constant(mut c) => {
                c.name = final_name.clone();
                c.path = new_path;
                c.dirty = true;
                c.original_entries.clear(); c.saved = false;
                c.original_name = final_name.clone();
                g.constants.push(c);
                "Constant"
            }
            NodeBody::Enum(mut e) => {
                e.name = final_name.clone();
                e.path = new_path;
                e.dirty = true;
                e.original_entries.clear(); e.saved = false;
                e.original_name = final_name.clone();
                g.enums.push(e);
                "Enum"
            }
        };
        self.ui_log(format!(
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
            self.warn_log("剪贴板为空或类型不匹配（需要 Group 剪贴板）".to_string());
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
        new_group.original_name = final_name.clone();
        new_group.dir = new_dir.clone();
        new_group.is_new = true;
        for t in &mut new_group.tables {
            t.path = new_dir.join(format!("{}.tbl", t.name));
            t.dirty = true;
            t.original_records.clear(); t.original_fields.clear(); t.saved = false;
            t.original_name = t.name.clone();
        }
        for c in &mut new_group.constants {
            c.path = new_dir.join(format!("{}.tbl", c.name));
            c.dirty = true;
            c.original_entries.clear(); c.saved = false;
            c.original_name = c.name.clone();
        }
        for e in &mut new_group.enums {
            e.path = new_dir.join(format!("{}.tbl", e.name));
            e.dirty = true;
            e.original_entries.clear(); e.saved = false;
            e.original_name = e.name.clone();
        }
        let summary = format!(
            "T{}+C{}+E{}",
            new_group.tables.len(),
            new_group.constants.len(),
            new_group.enums.len(),
        );
        project.groups.push(new_group);
        sync_schema_from_groups(project);
        self.ui_log(format!("已粘贴 Group 到 {}/{}（{}）", target_project, final_name, summary));
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
                    self.ui_log(format!("Project 不存在: {}", project_id));
                    return;
                };
                let dir = project.data_dir().join(name);
                project.groups.push(Group {
                    name: name.clone(),
                    original_name: name.clone(),
                    dir,
                    tables: Vec::new(),
                    constants: Vec::new(),
                    enums: Vec::new(),
                    is_new: true,
                });
                sync_schema_from_groups(project);
                self.ui_log(format!("[{}] 新建 Group: {}", project_id, name));
            }
            ProjectAction::NewTable { project_id, group, name } => {
                let mut ok = false;
                if let Some(project) = self.find_project_mut(project_id) {
                    if let Some(g) = project.groups.iter_mut().find(|g| g.name == *group) {
                        let path = g.dir.join(format!("{}.tbl", name));
                        g.tables.push(Table {
                            name: name.clone(),
                            original_name: name.clone(),
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
                            original_records: Vec::new(),
                            original_fields: vec![FieldDef {
                                name: "id".to_string(), desc: "ID".to_string(),
                                tbl_type: "int".to_string(), export: Export::ClientServer,
                            }],
                            saved: false,
                            fmt_ver: DEFAULT_TBL_VERSION,
                            display_field: None,
                            original_display_field: None,
                        });
                        ok = true;
                    }
                    sync_schema_from_groups(project);
                }
                if ok {
                    self.ui_log(format!("[{}] 新建 Table: {}/{}", project_id, group, name));
                }
            }
            ProjectAction::NewConstant { project_id, group, name } => {
                let mut ok = false;
                if let Some(project) = self.find_project_mut(project_id) {
                    if let Some(g) = project.groups.iter_mut().find(|g| g.name == *group) {
                        let path = g.dir.join(format!("{}.tbl", name));
                        g.constants.push(Constant {
                            name: name.clone(),
                            original_name: name.clone(),
                            path,
                            entries: Vec::new(),
                            dirty: true,
                            deleted: false,
                            original_entries: Vec::new(), saved: false,
                        });
                        ok = true;
                    }
                    sync_schema_from_groups(project);
                }
                if ok {
                    self.ui_log(format!("[{}] 新建 Constant: {}/{}", project_id, group, name));
                }
            }
            ProjectAction::NewEnum { project_id, group, name } => {
                let mut ok = false;
                if let Some(project) = self.find_project_mut(project_id) {
                    if let Some(g) = project.groups.iter_mut().find(|g| g.name == *group) {
                        let path = g.dir.join(format!("{}.tbl", name));
                        g.enums.push(EnumDef {
                            name: name.clone(),
                            original_name: name.clone(),
                            path,
                            entries: Vec::new(),
                            dirty: true,
                            deleted: false,
                            original_entries: Vec::new(), saved: false,
                            display_field: None,
                            original_display_field: None,
                        });
                        ok = true;
                    }
                    sync_schema_from_groups(project);
                }
                if ok {
                    self.ui_log(format!("[{}] 新建 Enum: {}/{}", project_id, group, name));
                }
            }
            ProjectAction::RenameGroup { project_id, old_name, new_name } => {
                // 重命名弹窗预填当前名，直接回车 = 没改名。放过它，免得盘上做一次
                // A → A 的空 rename、日志里多一条假改名。
                if old_name == new_name { return; }
                let pid = project_id.clone();
                if let Some(project) = self.find_project_mut(&pid) {
                    let config_dir = project.data_dir();
                    let new_dir = config_dir.join(new_name);
                    if let Some(g) = project.groups.iter_mut().find(|g| g.name == *old_name) {
                        // **只改内存**：盘上还是旧目录名，save 时才搬（见 apply_pending_renames）。
                        // 以前这里立刻 `fs::rename`，于是「文件已经动了」与「schema 还没写」
                        // 同时成立——树上那个 🟡 说不清究竟是什么没保存。
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
                self.ui_log(format!("[{}] 重命名 Group: {} → {}", project_id, old_name, new_name));
            }
            ProjectAction::RenameNode { project_id, group, old_name, new_name } => {
                // 同 RenameGroup：预填后直接确认属常见操作，不是一次真正的改名。
                if old_name == new_name { return; }
                let mut refs_rewritten = 0usize;
                if let Some(project) = self.find_project_mut(project_id) {
                    if let Some(g) = project.groups.iter_mut().find(|g| g.name == *group) {
                        let new_path = g.dir.join(format!("{}.tbl", new_name));
                        // 同 RenameGroup：只改内存，落盘留给 save。
                        // `original_name` 保持旧值 —— 它记的是「盘上叫什么」，save 才更新。
                        if let Some(t) = g.tables.iter_mut().find(|t| t.name == *old_name) {
                            t.name = new_name.clone();
                            t.path = new_path.clone();
                            t.update_dirty();   // 改名是一次未落盘的改动 → 树上应显示 🟡
                        }
                        if let Some(c) = g.constants.iter_mut().find(|c| c.name == *old_name) {
                            c.name = new_name.clone();
                            c.path = new_path.clone();
                            c.update_dirty();
                        }
                        if let Some(e) = g.enums.iter_mut().find(|e| e.name == *old_name) {
                            e.name = new_name.clone();
                            e.path = new_path;
                            e.update_dirty();
                        }
                    }
                    // 引用跟着改名走（必须在 sync 之前——sync 读的就是这些类型串）
                    refs_rewritten = rewrite_refs_after_rename(project, old_name, new_name);
                    sync_schema_from_groups(project);
                }
                if refs_rewritten > 0 {
                    self.ui_log(format!(
                        "已同步改写 {} 处 @{} 引用 → @{}", refs_rewritten, old_name, new_name
                    ));
                }
                let migrated: Vec<_> = self.validation_errors.iter()
                    .filter(|(p, g, n, _, _)| p == project_id && g == group && n == old_name)
                    .cloned().collect();
                for entry in &migrated { self.validation_errors.remove(entry); }
                for (_, g, _, r, c) in migrated {
                    self.validation_errors.insert((project_id.clone(), g, new_name.clone(), r, c));
                }
                self.ui_log(format!("[{}] 重命名: {}/{} → {}", project_id, group, old_name, new_name));
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

    /// `DeleteProject` 的实际逻辑（拆出便于阅读）。
    /// 用户允许删到 0 个 project（DBeaver-style 全部关闭）。
    fn execute_delete_project(&mut self, project_id: &str) {
        // 找到要删的盘上根 + 是否仅在内存中（state.is_pending()）
        let (project_root, in_memory_only) = if let Some(idx) = self.projects.iter().position(|p| p.schema.meta.id == project_id) {
            (self.projects[idx].project_root.clone(), self.projects[idx].state.is_pending())
        } else if let Some(ap) = self.available_projects.iter().find(|a| a.id == project_id) {
            (ap.root.clone(), false)
        } else {
            self.ui_log(format!("Project 不存在: {}", project_id));
            return;
        };

        // 内存项目（还没落盘）：跳过文件系统删除；NotFound 也忽略，按"目标已不在"处理。
        if !in_memory_only && project_root.exists() {
            if let Err(e) = std::fs::remove_dir_all(&project_root) {
                self.ui_log(format!("删除 project 目录失败: {}", e));
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
        self.ui_log(format!("已删除 Project: {}", project_id));
    }

    /// 该 Project 是否有未保存改动。
    /// 内存模式新建 / 克隆出来的项目：root 还没落盘 → 一定 dirty，即便 groups 为空
    /// 也算（空项目也不能"关掉就没了"）。`Project::is_dirty` 里已经含这条。
    pub fn is_project_dirty(&self, project_id: &str) -> bool {
        self.find_project(project_id).is_some_and(|p| p.is_dirty())
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
            self.ui_log(format!("Project 不存在: {}", project_id));
            return;
        };
        let (count, deleted, warnings) = save_project_files(project);
        for w in warnings { self.warn_log(w); }
        if count > 0 || deleted > 0 {
            self.ui_log(format!("[{}] 已保存 {} 个文件, 删除 {} 个", project_id, count, deleted));
        } else {
            self.ui_log(format!("[{}] 无修改需要保存", project_id));
        }
    }

    /// 收集指定 Project 全部验证错误的格式化日志（仅打日志，不入索引）。
    pub fn validate_project(&self, project_id: &str) -> Vec<String> {
        let Some(project) = self.find_project(project_id) else { return Vec::new(); };
        let mut errors = Vec::new();
        let sep = &project.schema.separators;
        let refs = RefIndex::build(&project.groups);
        let allow_ref = self.global_config.ui.as_ref()
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

    pub fn clear_all_config(&mut self) {
        let config_dir = self.project().data_dir();
        if config_dir.is_dir() {
            let _ = std::fs::remove_dir_all(&config_dir);
            let _ = std::fs::create_dir_all(&config_dir);
        }
        self.ui_log("已清空所有配置文件".to_string());
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

    pub fn export_json_filtered(&mut self, filter: &crate::export::DataFilter) -> anyhow::Result<crate::export::ExportResult> {
        let result = crate::export::export_json_filtered(self.project(), filter)?;
        self.log_export("JSON", &result);
        Ok(result)
    }

    pub fn export_xml(&mut self) -> anyhow::Result<crate::export::ExportResult> {
        let result = crate::export::export_all_xml(self.project())?;
        self.log_export("XML", &result);
        Ok(result)
    }

    pub fn export_xml_filtered(&mut self, filter: &crate::export::DataFilter) -> anyhow::Result<crate::export::ExportResult> {
        let result = crate::export::export_xml_filtered(self.project(), filter)?;
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

    pub fn export_typescript_client(&mut self) -> anyhow::Result<crate::export::ExportResult> {
        let result = crate::export::export_all_typescript(self.project(), crate::export::TypeScriptSide::Client)?;
        self.log_export("TypeScript (前端)", &result);
        Ok(result)
    }

    pub fn export_typescript_server(&mut self) -> anyhow::Result<crate::export::ExportResult> {
        let result = crate::export::export_all_typescript(self.project(), crate::export::TypeScriptSide::Server)?;
        self.log_export("TypeScript (Node.js)", &result);
        Ok(result)
    }

    pub fn export_cpp(&mut self) -> anyhow::Result<crate::export::ExportResult> {
        let result = crate::export::export_all_cpp(self.project())?;
        self.log_export("C++", &result);
        Ok(result)
    }

    pub fn export_csharp_dotnet(&mut self) -> anyhow::Result<crate::export::ExportResult> {
        let result = crate::export::export_all_csharp(self.project(), crate::export::CSharpRuntime::Dotnet)?;
        self.log_export("C# (.NET)", &result);
        Ok(result)
    }

    pub fn export_csharp_unity(&mut self) -> anyhow::Result<crate::export::ExportResult> {
        let result = crate::export::export_all_csharp(self.project(), crate::export::CSharpRuntime::Unity)?;
        self.log_export("C# (Unity)", &result);
        Ok(result)
    }

    pub fn export_csharp_godot(&mut self) -> anyhow::Result<crate::export::ExportResult> {
        let result = crate::export::export_all_csharp(self.project(), crate::export::CSharpRuntime::Godot)?;
        self.log_export("C# (Godot)", &result);
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
        self.ui_log(format!("[{}] {} 新增, {} 修改, {} 删除, {} 不变",
            label, result.added(), result.modified(), result.deleted(), result.unchanged()));
        for f in &result.files {
            match f.status {
                FileStatus::Added => self.ui_log(format!("  [新增] {}", f.path)),
                FileStatus::Modified => self.ui_log(format!("  [修改] {}", f.path)),
                FileStatus::Deleted => self.ui_log(format!("  [删除] {}", f.path)),
                FileStatus::Unchanged => {}
            }
        }
        for w in &result.warnings {
            self.ui_log(format!("  [警告] {}", w));
        }
    }
}

#[cfg(test)]
mod project_toml_template_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn unique_tmp(label: &str) -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("tbl_save_{}_{}_{}", label, std::process::id(), n))
    }

    fn make_project(workdir: PathBuf, project_root: PathBuf, id: &str) -> Project {
        let mut schema = crate::tblschema::TblSchema::default();
        schema.meta.id = id.to_string();
        schema.meta.name = id.to_string();
        let config = ProjectConfig::default();
        let original_export = config.export.clone();
        Project {
            workdir,
            project_root,
            config,
            schema,
            original_meta: crate::tblschema::SchemaMetadata::default(),
            original_separators: Default::default(),
            original_export,
            groups: Vec::new(),
            state: crate::model::ProjectState::Pending,
        }
    }

    #[test]
    fn project_toml_is_only_written_when_export_actually_changed() {
        // `project.toml` 是**可选的 [export] 覆盖段**：没改过就不该有文件，
        // export 继续从工作区 tablet.toml 继承（@02.26.3）。
        let dir = unique_tmp("toml_optional");
        std::fs::create_dir_all(&dir).unwrap();
        let project_root = dir.join("projects").join("p1");
        let mut project = make_project(dir.clone(), project_root.clone(), "p1");
        let toml_path = project_root.join(crate::project::PROJECT_TOML_FILE);

        save_project_files(&mut project);
        assert!(!toml_path.exists(), "没改过 export 就不该生成覆盖文件");
        assert!(project_root.join(crate::project::PROJECT_SCHEMA_FILE).exists(),
            "但 tblschema 首次落盘还是要写");

        // 改 export（模拟「项目设置」）→ 保存才写入
        project.config.export.json.as_mut().unwrap().empty_as = Some(crate::enums::JsonEmptyAs::Omit);
        assert!(project.toml_changed(), "改了 export → 该写 project.toml");
        save_project_files(&mut project);

        let txt = std::fs::read_to_string(&toml_path).unwrap();
        assert!(txt.lines().any(|l| l.trim() == "[export.json]"), "缺 [export.json]: {}", txt);
        assert!(!txt.lines().any(|l| l.trim() == "[project]"), "项目级 toml 不应有 [project] 段");
        assert!(!txt.lines().any(|l| l.trim() == "[ui]"), "项目级 toml 不应有 [ui] 段（工作区级）");
        assert!(!project.toml_changed(), "落盘后应当回到「没改过」");

        // 没改过再存一次，不该重写
        let before = std::fs::read_to_string(&toml_path).unwrap();
        save_project_files(&mut project);
        assert_eq!(std::fs::read_to_string(&toml_path).unwrap(), before, "没改过不该重写");

        // 重载：写出去的必须是**项目当前的**配置，不是默认值。
        // 曾经这里写的是 `ExportConfig::default()`，于是"新建项目首次保存"会把
        // 从全局拷来的配置静默重置成全默认。
        std::fs::write(dir.join(crate::CONFIG_FILE), "").unwrap();
        let loaded = crate::project::load_specific_project(&dir, "p1").expect("reload");
        assert_eq!(
            loaded.config.export.json.as_ref().unwrap().empty_as,
            Some(crate::enums::JsonEmptyAs::Omit),
            "重载后 export 应当还是写出去的那份"
        );
        assert!(!loaded.toml_changed(), "刚加载的项目不该显示 export 改过");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saving_does_not_touch_an_unchanged_project_toml() {
        // 只要没在「项目设置」里动 export，保存就不碰 project.toml。
        let dir = unique_tmp("preserve");
        let project_root = dir.join("projects").join("p1");
        std::fs::create_dir_all(&project_root).unwrap();
        let toml_path = project_root.join(crate::project::PROJECT_TOML_FILE);
        let custom = "# 手写注释
[export.server.java]
package = \"com.user.kept\"
";
        std::fs::write(&toml_path, custom).unwrap();

        let mut project = make_project(dir.clone(), project_root.clone(), "p1");
        project.state = crate::model::ProjectState::Loaded;
        let _ = save_project_files(&mut project);

        assert_eq!(std::fs::read_to_string(&toml_path).unwrap(), custom,
            "没改过 export 时保存不该碰这个文件");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_persists_display_field_and_reload_restores_it() {
        // 改节点 display_field → 保存 → project.tblschema 带 `# @display` → 重载回填（@10）。
        let dir = unique_tmp("display");
        std::fs::create_dir_all(&dir).unwrap();
        let project_root = dir.join("projects").join("p1");
        let mut project = make_project(dir.clone(), project_root.clone(), "p1");
        let gdir = project_root.join("config").join("g");

        project.groups.push(Group {
            name: "g".to_string(),
            original_name: "g".to_string(),
            dir: gdir.clone(),
            tables: vec![Table {
                name: "T".to_string(),
                original_name: "T".to_string(),
                path: gdir.join("T.tbl"),
                schema: TableSchema { fields: vec![
                    FieldDef { name: "id".into(), desc: "ID".into(), tbl_type: "int".into(), export: Export::ClientServer },
                    FieldDef { name: "name".into(), desc: "名".into(), tbl_type: "str".into(), export: Export::ClientServer },
                ]},
                records: vec![vec!["1".into(), "Alice".into()]],
                dirty: true, deleted: false,
                original_records: Vec::new(), original_fields: Vec::new(),
                saved: false, fmt_ver: crate::tbl::DEFAULT_TBL_VERSION,
                display_field: Some("name".to_string()),
                original_display_field: None,
            }],
            constants: Vec::new(),
            enums: vec![EnumDef {
                name: "E".to_string(),
                original_name: "E".to_string(),
                path: gdir.join("E.tbl"),
                entries: vec![EnumEntry { id: "1".into(), name: "WAR".into(), desc: "战士".into() }],
                dirty: true, deleted: false,
                original_entries: Vec::new(), saved: false,
                display_field: Some("desc".to_string()),
                original_display_field: None,
            }],
            is_new: true,
        });
        save_project_files(&mut project);

        let schema_txt = std::fs::read_to_string(project_root.join(crate::project::PROJECT_SCHEMA_FILE)).unwrap();
        assert!(schema_txt.contains("[g/T] table\n# @display: name"), "schema = {}", schema_txt);
        assert!(schema_txt.contains("[g/E] enum\n# @display: desc"), "schema = {}", schema_txt);

        // 重载：.tbl 里不含 display 信息，应从 schema 段回填到节点
        let (engine, _, _) = crate::project::load_workspace(&dir).expect("reload");
        assert_eq!(engine.find_table("g", "T").expect("table").display_field.as_deref(), Some("name"));
        assert_eq!(engine.find_enum("g", "E").expect("enum").display_field.as_deref(), Some("desc"));
        // 回填必须连 `original_display_field` 一起填：它是"从盘上读回来的"值。
        // 只填 display_field 的话，刚加载的项目就显示"改过"、永远亮 🟡。
        let p = engine.find_project("p1").expect("project");
        assert!(!p.is_dirty(), "刚加载的项目不该有任何未落盘的改动");
        assert!(!p.schema_changed(), "……schema 更不该");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rename_unsaved_project_only_touches_memory() {
        // 回归：未保存的内存项目（state=Pending，目录不存在）改身份信息
        // 不应因目录不存在而报错——只改内存，等 save 时统一落盘。
        let dir = unique_tmp("rename_unsaved");
        let project_root = dir.join("projects").join("p1");
        // 注意：故意不创建 project_root 目录，模拟"新建未保存"场景
        let project = make_project(dir.clone(), project_root.clone(), "p1");
        assert!(project.state.is_pending(), "make_project 默认 pending");
        let mut engine = ProjectEngine::new(project, GlobalConfig::default());

        // 直接修改内存中的 meta 字段（模拟 UI 项目设置的行为）
        {
            let p = engine.find_project_mut("p1").unwrap();
            p.schema.meta.id = "p2".to_string();
            p.schema.meta.name = "新名字".to_string();
        }

        // 内存中身份已更新
        let p = engine.find_project("p2").expect("p2 应存在于内存");
        assert_eq!(p.schema.meta.id, "p2");
        assert_eq!(p.schema.meta.name, "新名字");
        assert!(p.state.is_pending(), "仍未落盘");
        assert!(p.schema_changed(), "meta 变了 → schema 与盘上不一致");
        // 磁盘上不应有任何文件（目录都没建）
        assert!(!project_root.exists(), "旧目录不该被创建");
        assert!(!dir.join("projects").join("p2").exists(), "新目录不该被创建");

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── 重命名的「保存时才动盘」契约 ────────────────────────────────────
    //
    // 改名曾经是全程序唯一一个「保存前就碰文件系统」的操作：`.tbl` 立刻被
    // `fs::rename`，而 `project.tblschema` 没跟着写。于是「文件已经动了」与
    // 「还有东西没保存」同时成立，树上那个 🟡 说不清是什么没保存。
    // 现在改名只写内存，搬文件推迟到 save（`apply_pending_renames`）。

    fn id_field() -> FieldDef {
        FieldDef { name: "id".into(), desc: "ID".into(), tbl_type: "int".into(), export: Export::ClientServer }
    }

    /// 造一份「已经落过盘」的项目：内存建 → `save` → **从盘上重新 load**。
    ///
    /// 刻意手搓 `original_name` / `saved` / `original_meta` 是不行的——那等于
    /// 我自己给自己出题。走一遍真实链路顺带把一件事钉住：`resync_schema()` 从
    /// groups 重建出来的 schema 必须**等于**从 `project.tblschema` 解析出来的，
    /// 否则每个项目一打开就永远亮着 🟡。
    fn engine_with_saved_node(label: &str, group: &str, node: &str) -> (PathBuf, ProjectEngine) {
        let dir = unique_tmp(label);
        let project_root = dir.join("projects").join("p1");
        let gdir = project_root.join("config").join(group);

        let mut project = make_project(dir.clone(), project_root, "p1");
        project.groups.push(Group {
            name: group.to_string(),
            original_name: group.to_string(),
            dir: gdir.clone(),
            tables: vec![Table {
                name: node.to_string(),
                original_name: node.to_string(),
                path: gdir.join(format!("{}.tbl", node)),
                schema: TableSchema { fields: vec![id_field()] },
                records: vec![vec!["1".into()]],
                dirty: true, deleted: false,
                original_records: Vec::new(), original_fields: Vec::new(),
                saved: false, fmt_ver: crate::tbl::DEFAULT_TBL_VERSION, display_field: None,
            original_display_field: None,
            }],
            constants: vec![], enums: vec![], is_new: true,
        });
        save_project_files(&mut project);

        // load_specific_project 要求工作区根有 tablet.toml（全部字段都能缺省）
        std::fs::write(dir.join(crate::CONFIG_FILE), "").unwrap();
        let loaded = crate::project::load_specific_project(&dir, "p1").expect("重新加载");
        assert!(!loaded.is_dirty(), "刚加载的项目不该有任何未落盘的改动");
        assert!(!loaded.schema_changed(), "……schema 更不该");
        (dir, ProjectEngine::new(loaded, GlobalConfig::default()))
    }

    fn config_dir(dir: &Path) -> PathBuf {
        dir.join("projects").join("p1").join("config")
    }

    #[test]
    fn rename_touches_no_file_until_save() {
        let (dir, mut engine) = engine_with_saved_node("rename_defer", "hero", "HeroBase");
        let pid = engine.active_project_id().unwrap().to_string();
        let gdir = config_dir(&dir).join("hero");

        engine.execute_action(&ProjectAction::RenameNode {
            project_id: pid, group: "hero".into(),
            old_name: "HeroBase".into(), new_name: "HeroMain".into(),
        });

        // 改名那一刻盘上一步都不许动 —— 这条就是「显示脏了但文件已经改了」的修复
        assert!(gdir.join("HeroBase.tbl").is_file(), "改名不该立刻动盘");
        assert!(!gdir.join("HeroMain.tbl").exists(), "新名不该提前出现");

        // 内存里已经改了，且节点应当是脏的（树上的 🟡）
        let t = engine.find_table("hero", "HeroMain").expect("内存里应已改名");
        assert!(t.dirty, "改名是一次未落盘的改动，节点必须显示为脏");
        assert_eq!(t.original_name, "HeroBase", "原名要留着，save 才找得到盘上的旧文件");

        // 保存后才搬
        assert!(engine.save_all(), "干净项目应当保存成功");
        assert!(!gdir.join("HeroBase.tbl").exists(), "保存后旧文件应已搬走");
        assert!(gdir.join("HeroMain.tbl").is_file(), "保存后新文件应就位");
        assert_eq!(engine.find_table("hero", "HeroMain").unwrap().original_name, "HeroMain");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn chained_rename_still_finds_the_on_disk_file() {
        // A→B→C：盘上自始至终只有 A.tbl。original_name 若记成「上一次的名字」，
        // 第二次改名就把 A 覆盖成 B，save 会去找一个不存在的 B.tbl。
        let (dir, mut engine) = engine_with_saved_node("rename_chain", "hero", "A");
        let pid = engine.active_project_id().unwrap().to_string();
        let gdir = config_dir(&dir).join("hero");
        for (old, new) in [("A", "B"), ("B", "C")] {
            engine.execute_action(&ProjectAction::RenameNode {
                project_id: pid.clone(), group: "hero".into(),
                old_name: old.into(), new_name: new.into(),
            });
        }

        assert_eq!(engine.find_table("hero", "C").unwrap().original_name, "A",
            "连续改名后，盘上的原名仍是 A");
        assert!(engine.save_all());
        assert!(gdir.join("C.tbl").is_file(), "最终应落在 C.tbl");
        assert!(!gdir.join("A.tbl").exists() && !gdir.join("B.tbl").exists(),
            "中间名不该留任何文件");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn group_and_node_renamed_together_both_land() {
        // 父子同时改名：组记组的原名、节点记节点的原名，两级各推各的。
        let (dir, mut engine) = engine_with_saved_node("rename_both", "hero", "HeroBase");
        let pid = engine.active_project_id().unwrap().to_string();
        engine.execute_action(&ProjectAction::RenameGroup {
            project_id: pid.clone(), old_name: "hero".into(), new_name: "character".into(),
        });
        engine.execute_action(&ProjectAction::RenameNode {
            project_id: pid, group: "character".into(),
            old_name: "HeroBase".into(), new_name: "HeroMain".into(),
        });

        assert!(engine.save_all());
        let config = config_dir(&dir);
        assert!(!config.join("hero").exists(), "旧组目录应已搬走");
        assert!(config.join("character").join("HeroMain.tbl").is_file(),
            "组名与表名应当一起落地");
        assert!(!config.join("character").join("HeroBase.tbl").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn renaming_a_never_saved_table_just_writes_the_new_name() {
        // 盘上没有旧文件的新表改名：不该去找 A.tbl，直接按新名写出来即可。
        let dir = unique_tmp("rename_new");
        let project_root = dir.join("projects").join("p1");
        let gdir = project_root.join("config").join("hero");
        let mut project = make_project(dir.clone(), project_root, "p1");
        project.groups.push(Group {
            name: "hero".into(), original_name: "hero".into(), dir: gdir.clone(),
            tables: vec![Table {
                name: "T".into(), original_name: "T".into(), path: gdir.join("T.tbl"),
                schema: TableSchema { fields: vec![id_field()] },
                records: vec![vec!["1".into()]],
                dirty: true, deleted: false,
                original_records: Vec::new(), original_fields: Vec::new(),
                saved: false, fmt_ver: crate::tbl::DEFAULT_TBL_VERSION, display_field: None,
            original_display_field: None,
            }],
            constants: vec![], enums: vec![], is_new: true,
        });
        let mut engine = ProjectEngine::new(project, GlobalConfig::default());
        let pid = engine.active_project_id().unwrap().to_string();
        engine.execute_action(&ProjectAction::RenameNode {
            project_id: pid, group: "hero".into(),
            old_name: "T".into(), new_name: "T2".into(),
        });

        assert!(engine.save_all());
        assert!(gdir.join("T2.tbl").is_file(), "新名应被写出来");
        assert!(!gdir.join("T.tbl").exists(), "不该凭空造出旧名文件");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_rename_keeps_the_old_name_instead_of_lying() {
        // 目标名被一个同名**目录**占住 → `fs::rename` 必然失败（Windows / Linux 都是）。
        // 这时唯一诚实的做法是把内存退回旧名（盘上确实还是旧名）并把警告报出来，
        // 而不是留着「内存叫 HeroMain、盘上叫 HeroBase」的分叉。
        let (dir, mut engine) = engine_with_saved_node("rename_fail", "hero", "HeroBase");
        let pid = engine.active_project_id().unwrap().to_string();
        let gdir = config_dir(&dir).join("hero");
        std::fs::create_dir_all(gdir.join("HeroMain.tbl")).unwrap();

        engine.execute_action(&ProjectAction::RenameNode {
            project_id: pid, group: "hero".into(),
            old_name: "HeroBase".into(), new_name: "HeroMain".into(),
        });
        assert!(engine.save_all(), "save 本身不失败，只是这一次改名没生效");

        assert!(engine.find_table("hero", "HeroBase").is_some(),
            "改名失败后内存应退回旧名，不能假装成功");
        assert!(engine.find_table("hero", "HeroMain").is_none());
        assert!(gdir.join("HeroBase.tbl").is_file(), "旧文件应原地未动");
        assert!(engine.logs().iter().any(|l| l.contains("保留原名")),
            "失败必须报到日志，不能吞: {:?}", engine.logs());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn renaming_back_to_the_original_name_clears_the_project_dirty_mark() {
        // 改名 A→A1 再改回 A：节点 🟡 会消失（`update_dirty()` 是算的），
        // 项目 🟡 曾经不会——`schema_dirty` 是贴上就撕不掉的标。
        // `schema.sections` 此时与盘上逐字相同，就该回到干净。
        let (dir, mut engine) = engine_with_saved_node("rename_roundtrip", "hero", "HeroBase");
        let pid = engine.active_project_id().unwrap().to_string();
        let rename = |engine: &mut ProjectEngine, old: &str, new: &str| {
            engine.execute_action(&ProjectAction::RenameNode {
                project_id: pid.clone(), group: "hero".into(),
                old_name: old.into(), new_name: new.into(),
            });
        };

        rename(&mut engine, "HeroBase", "HeroMain");
        assert!(engine.find_project(&pid).unwrap().schema_changed(),
            "真改了名，schema 与盘上不一致 → 该脏");
        assert!(engine.is_project_dirty(&pid), "项目此时应亮 🟡");

        rename(&mut engine, "HeroMain", "HeroBase");
        assert!(!engine.find_project(&pid).unwrap().schema_changed(),
            "改回原名后 schema 与盘上一致，不该还标脏");
        assert!(!engine.find_table("hero", "HeroBase").unwrap().dirty,
            "节点同样应回到干净");
        assert!(!engine.is_project_dirty(&pid), "整个项目不该再显示 🟡");

        // 全程盘上一步没动
        assert!(config_dir(&dir).join("hero").join("HeroBase.tbl").is_file());

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── 改名要把引用一起带走（@01.8.5 改名"不阻断保存"的兑现） ──────────

    #[test]
    fn ref_rewrite_matches_by_name_boundary() {
        assert_eq!(rename_ref_in_type("@Hero", "Hero", "Unit").as_deref(), Some("@Unit"));
        assert_eq!(rename_ref_in_type("List<@Hero>", "Hero", "Unit").as_deref(), Some("List<@Unit>"));
        assert_eq!(rename_ref_in_type("Map<int,@Hero>", "Hero", "Unit").as_deref(), Some("Map<int,@Unit>"));
        assert_eq!(rename_ref_in_type("Map<@Hero,List<@Hero>>", "Hero", "Unit").as_deref(),
                   Some("Map<@Unit,List<@Unit>>"));
        // `@HeroBase` 不是 `@Hero`，不能被误伤
        assert_eq!(rename_ref_in_type("@HeroBase", "Hero", "Unit"), None);
        assert_eq!(rename_ref_in_type("List<@HeroBase>", "Hero", "Unit"), None);
        assert_eq!(rename_ref_in_type("Map<int,@HeroBase>", "Hero", "Unit"), None);
        // 无关的类型原样不动
        assert_eq!(rename_ref_in_type("int", "Hero", "Unit"), None);
        assert_eq!(rename_ref_in_type("@Skill", "Hero", "Unit"), None);
        assert_eq!(rename_ref_in_type("List<str>", "Hero", "Unit"), None);
    }

    /// 造一份带引用的项目：`HeroBase` 自引用 + `Skill` 用 `List<@HeroBase>` 引用它。
    fn engine_with_refs(label: &str) -> (PathBuf, ProjectEngine) {
        let dir = unique_tmp(label);
        let project_root = dir.join("projects").join("p1");
        let gdir = project_root.join("config").join("hero");
        let mk = |name: &str, field: FieldDef| Table {
            name: name.to_string(),
            original_name: name.to_string(),
            path: gdir.join(format!("{}.tbl", name)),
            schema: TableSchema { fields: vec![id_field(), field] },
            records: vec![vec!["1".into(), "1".into()]],
            dirty: true, deleted: false,
            original_records: Vec::new(), original_fields: Vec::new(),
            saved: false, fmt_ver: crate::tbl::DEFAULT_TBL_VERSION, display_field: None,
            original_display_field: None,
        };
        let mut project = make_project(dir.clone(), project_root, "p1");
        project.groups.push(Group {
            name: "hero".into(), original_name: "hero".into(), dir: gdir.clone(),
            tables: vec![
                mk("HeroBase", FieldDef { name: "leader".into(), desc: "关联英雄".into(),
                    tbl_type: "@HeroBase".into(), export: Export::ClientServer }),
                mk("Skill", FieldDef { name: "heroes".into(), desc: "关联英雄".into(),
                    tbl_type: "List<@HeroBase>".into(), export: Export::ClientServer }),
            ],
            constants: vec![], enums: vec![], is_new: true,
        });
        save_project_files(&mut project);
        std::fs::write(dir.join(crate::CONFIG_FILE), "").unwrap();
        let loaded = crate::project::load_specific_project(&dir, "p1").expect("重新加载");
        (dir, ProjectEngine::new(loaded, GlobalConfig::default()))
    }

    #[test]
    fn renaming_a_referenced_table_moves_the_references_with_it() {
        // 不改写引用的话，改名当场把项目变成非法（`@HeroBase` 找不到目标）→
        // save_all 的验证闸门拦下 → 改名永远落不了盘。
        let (dir, mut engine) = engine_with_refs("rename_refs");
        let pid = engine.active_project_id().unwrap().to_string();
        assert!(engine.validate().is_empty(), "起点应当是合法的: {:?}", engine.validate());

        engine.execute_action(&ProjectAction::RenameNode {
            project_id: pid.clone(), group: "hero".into(),
            old_name: "HeroBase".into(), new_name: "HeroMain".into(),
        });

        // 自引用（HeroMain.leader）与被引用（Skill.heroes）都要跟过去
        assert_eq!(engine.find_table("hero", "HeroMain").unwrap().schema.fields[1].tbl_type, "@HeroMain");
        assert_eq!(engine.find_table("hero", "Skill").unwrap().schema.fields[1].tbl_type, "List<@HeroMain>");

        // 项目仍然合法 → 改名真的能落盘
        assert!(engine.validate().is_empty(), "引用跟过去了，不该有验证错误: {:?}", engine.validate());
        assert!(engine.save_all(), "引用不再断裂，保存应当成功");

        let gdir = config_dir(&dir).join("hero");
        assert!(gdir.join("HeroMain.tbl").is_file(), "改名应已落盘");
        assert!(!gdir.join("HeroBase.tbl").exists(), "旧文件应已搬走");

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── 分层的脏：每层只回答"我和盘上一致吗"，UI 往上投影 ──────────

    #[test]
    fn a_data_edit_dirties_the_node_but_not_the_schema() {
        // 分层的关键一条。父层如果直接 OR 子节点的 `dirty`，那么**每次单元格编辑**
        // 都会把 project.tblschema 标成需要重写——改一个格子的值根本不动 schema。
        let (dir, mut engine) = engine_with_saved_node("schema_layer", "hero", "HeroBase");
        let pid = engine.active_project_id().unwrap().to_string();
        assert!(!engine.find_project(&pid).unwrap().schema_changed(), "起点应当干净");

        engine.set_table_cell("hero", "HeroBase", 0, 0, "42");

        let p = engine.find_project(&pid).unwrap();
        assert!(p.is_dirty(), "数据改了 → 这个项目有东西要写盘");
        assert!(!p.schema_changed(), "但 project.tblschema 一个字都不用动");
        assert!(engine.find_table("hero", "HeroBase").unwrap().dirty, "该表自己要写盘");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn display_field_is_part_of_the_schema_and_reverting_it_clears_it() {
        // `# @display` 写在 project.tblschema 里，所以它必须进 `schema_changed()`；
        // 而它没有 records 那种自然快照，靠 `original_display_field`。
        let (dir, mut engine) = engine_with_saved_node("schema_display", "hero", "HeroBase");
        let pid = engine.active_project_id().unwrap().to_string();

        engine.find_table_mut("hero", "HeroBase").unwrap().display_field = Some("id".into());
        assert!(engine.find_project(&pid).unwrap().schema_changed(), "改了 # @display → schema 要写");

        engine.find_table_mut("hero", "HeroBase").unwrap().display_field = None;
        assert!(!engine.find_project(&pid).unwrap().schema_changed(), "改回默认 → 回到干净");

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── 从 schema 落地项目：L2 + 闸门 + L1 ────────────────────────────

    fn schema_from(text: &str) -> crate::tblschema::TblSchema {
        crate::tblschema::parse_tblschema(text).expect("parse schema")
    }

    #[test]
    fn create_project_from_schema_lands_on_disk_and_gates_on_validation() {
        let dir = unique_tmp("cps");
        std::fs::create_dir_all(&dir).unwrap();
        let mut engine = ProjectEngine::new_workspace(
            dir.clone(), GlobalConfig::default(), Vec::new(), Vec::new(), None);

        // ① 正常：schema → 项目立刻出现在盘上
        let id = engine.create_project_from_schema(schema_from(
            "#!tblschema v1\n# @meta id: p1\n\n[hero/HeroBase] table\nid | int | cs | ID\nname | str | cs | 名称\n"
        )).expect("创建");
        assert_eq!(id, "p1");
        let root = dir.join("projects").join("p1");
        assert!(root.join(crate::project::PROJECT_SCHEMA_FILE).is_file(), "tblschema 应落盘");
        assert!(root.join("config").join("hero").join("HeroBase.tbl").is_file(), ".tbl 应落盘");
        assert!(engine.find_project("p1").is_some(), "应注册进 projects");
        assert!(engine.available_projects.iter().any(|a| a.id == "p1"), "应注册进 available");
        assert!(!engine.is_project_dirty("p1"), "刚落地应当是干净的");

        // ② 闸门：悬空 @引用 → 拒绝创建，且**什么都不留**
        let err = engine.create_project_from_schema(schema_from(
            "#!tblschema v1\n# @meta id: badref\n\n[hero/HeroBase] table\nid | int | cs | ID\nleader | @Missing | cs | 关联\n"
        )).expect_err("悬空引用应被闸门拦下");
        assert!(err.contains("校验未通过"), "{}", err);
        assert!(err.contains("Missing"), "错误里应当有具体项: {}", err);
        assert!(!dir.join("projects").join("badref").exists(), "不该建目录");
        assert!(engine.find_project("badref").is_none(), "不该留在 projects 里");
        assert!(!engine.available_projects.iter().any(|a| a.id == "badref"),
            "不该留在 available 里——否则树上会挂一个永远建不出来的空壳");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn changing_export_makes_the_project_dirty_but_not_the_schema() {
        // `project.toml` 与 `project.tblschema` 是两个文件，所以要分开问。
        // 曾经 UI 树手工拼聚合、只问 `schema_changed()`，于是"只改了 export 保存"
        // 不亮 🟡——树现在直接问 `Project::is_dirty()`。
        let (dir, mut engine) = engine_with_saved_node("export_dirty", "hero", "HeroBase");
        let pid = engine.active_project_id().unwrap().to_string();
        assert!(!engine.find_project(&pid).unwrap().is_dirty(), "起点应当干净");

        engine.find_project_mut(&pid).unwrap().config.export
            .json.as_mut().unwrap().empty_as = Some(crate::enums::JsonEmptyAs::Omit);

        let p = engine.find_project(&pid).unwrap();
        assert!(p.toml_changed(), "export 变了 → project.toml 要写");
        assert!(p.is_dirty(), "项目该亮 🟡");
        assert!(!p.schema_changed(), "但 project.tblschema 一个字都不用动");
        assert!(engine.is_project_dirty(&pid), "关项目前的「有未保存改动」也应当为真");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn deleting_a_saved_node_is_a_schema_change() {
        // 被删掉的段要从 project.tblschema 里消失，所以删除也是 schema 变更。
        // 而"新建了又没保存就删掉"的节点盘上从来没有过，不该算。
        let (dir, mut engine) = engine_with_saved_node("schema_delete", "hero", "HeroBase");
        let pid = engine.active_project_id().unwrap().to_string();

        engine.delete_node("hero", "HeroBase");
        assert!(engine.find_project(&pid).unwrap().schema_changed(),
            "删掉落过盘的节点 → schema 里那一段要消失");
        assert!(engine.find_project(&pid).unwrap().is_dirty(), "save 要 remove 文件");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rewriting_refs_does_not_touch_a_different_name() {
        // `Hero` 改成 `Unit` 不能碰到 `HeroBase` 的引用——词边界要挡住。
        let (dir, mut engine) = engine_with_refs("rename_refs_boundary");
        let pid = engine.active_project_id().unwrap().to_string();
        engine.execute_action(&ProjectAction::RenameNode {
            project_id: pid, group: "hero".into(),
            old_name: "Hero".into(), new_name: "Unit".into(),
        });
        // 没有叫 Hero 的节点，什么都没改
        assert_eq!(engine.find_table("hero", "HeroBase").unwrap().schema.fields[1].tbl_type, "@HeroBase");
        assert_eq!(engine.find_table("hero", "Skill").unwrap().schema.fields[1].tbl_type, "List<@HeroBase>");

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ─── 「还原到已保存」（@02.26） ───────────────────────────────────

    #[test]
    fn revert_node_puts_every_edited_face_back() {
        // 内容 / 结构 / 身份三样一起改，一次还原要全都回去。
        let (dir, mut engine) = engine_with_saved_node("revert_all_faces", "hero", "HeroBase");

        let t = engine.find_table("hero", "HeroBase").unwrap();
        let (rec0, desc0) = (t.records[0][0].clone(), t.schema.fields[0].desc.clone());

        {
            let t = engine.find_table_mut("hero", "HeroBase").unwrap();
            t.records[0][0] = "9".into();
            t.schema.fields[0].desc = "改了".into();
            t.name = "HeroBase2".into();
            t.display_field = Some("id".into());
            t.update_dirty();
        }
        assert!(engine.find_table("hero", "HeroBase2").unwrap().revertable());

        assert!(engine.revert_node("hero", "HeroBase2"), "应当真的还原了");

        let t = engine.find_table("hero", "HeroBase").unwrap();
        assert_eq!(t.records[0][0], rec0, "内容没回去");
        assert_eq!(t.schema.fields[0].desc, desc0, "结构没回去");
        assert_eq!(t.name, "HeroBase", "名字没回去");
        assert_eq!(t.display_field, None, "# @display 没回去");
        assert!(!t.dirty && !t.schema_changed(), "还原了却还报脏");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reverting_a_rename_moves_no_file_on_save() {
        // 改名本来就只落内存、save 时才搬（C2/@02.26.1）。改回原名再保存，
        // 盘上应当**什么都没动**——连一次多余的 rename 都不该发生。
        let (dir, mut engine) = engine_with_saved_node("revert_rename_noop", "hero", "HeroBase");
        let pid = engine.active_project_id().unwrap().to_string();
        let gdir = config_dir(&dir).join("hero");

        let before: Vec<String> = std::fs::read_dir(&gdir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();

        engine.execute_action(&ProjectAction::RenameNode {
            project_id: pid, group: "hero".into(),
            old_name: "HeroBase".into(), new_name: "Renamed".into(),
        });
        assert!(engine.revert_node("hero", "Renamed"));
        assert!(!engine.find_table("hero", "HeroBase").unwrap().dirty, "还原后该是干净的");

        assert!(engine.save_all(), "保存应成功");

        let mut after: Vec<String> = std::fs::read_dir(&gdir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        let mut before_sorted = before.clone();
        before_sorted.sort();
        after.sort();
        assert_eq!(before_sorted, after, "改回原名后保存不该搬动任何文件");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn revert_node_ignores_a_node_that_was_never_saved() {
        // 盘上从来没有的节点没有「盘上的样子」可还原 —— 这是刻意划的边界，
        // 存在性归右键的新建/删除管。
        let (dir, mut engine) = engine_with_saved_node("revert_never_saved", "hero", "HeroBase");
        let pid = engine.active_project_id().unwrap().to_string();
        engine.execute_action(&ProjectAction::NewTable {
            project_id: pid, group: "hero".into(), name: "Fresh".into(),
        });

        assert!(!engine.find_table("hero", "Fresh").unwrap().revertable(), "没落过盘的节点不该可还原");
        assert!(!engine.revert_node("hero", "Fresh"), "不该动它");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_changed_ignores_data_edits() {
        // 「全部还原」按钮的可用状态。它还原不了数据改动，就不该为数据改动亮起来。
        let (dir, mut engine) = engine_with_saved_node("settings_changed", "hero", "HeroBase");
        let pid = engine.active_project_id().unwrap().to_string();
        assert!(!engine.find_project(&pid).unwrap().settings_changed(), "起点该是干净的");

        {
            let t = engine.find_table_mut("hero", "HeroBase").unwrap();
            t.records[0][0] = "9".into();
            t.update_dirty();
        }
        assert!(!engine.find_project(&pid).unwrap().settings_changed(),
            "只改了数据，不该点亮「全部还原」");
        assert!(engine.find_project(&pid).unwrap().is_dirty(), "但它确实脏了");

        engine.find_project_mut(&pid).unwrap().schema.meta.name = "别的名字".into();
        assert!(engine.find_project(&pid).unwrap().settings_changed(), "改了 meta 该亮");

        engine.find_project_mut(&pid).unwrap().revert_settings();
        assert!(!engine.find_project(&pid).unwrap().settings_changed(), "还原后该自动回到干净");

        let _ = std::fs::remove_dir_all(&dir);
    }
}

