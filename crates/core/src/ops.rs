use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use crate::history::{CellSnapshot, EditHistory, EditOp, Transaction};
use crate::model::*;
use crate::validate::*;

/// 把一条 ValidationError 格式化为日志行（统一 B 格式：`位置:[内容] -> 原因`）：
/// - Table 表头：`[验证] group/node 表头第N行X列:[field] -> message`
///   N = 1 desc / 2 export / 3 type / 4 field（与 UI 表头从上到下一致）
/// - Constant/Enum 表头：`[验证] group/node 表头X列:[field] -> message`
/// - 数据行：`[验证] group/node X<row>:[value] -> message`，value 过长截断为 `xxx...`
pub fn format_validation_log(group: &str, node: &str, err: &ValidationError) -> String {
    let tag = match err.severity {
        Severity::Error => "[验证]",
        Severity::Warning => "[警告]",
        Severity::Info => "[信息]",
    };
    if err.code == ValidationCode::NodeNameDuplicate {
        // 节点名是项目级错误，没有表头/单元格位置，不要伪装成“表头A列”。
        return format!("{} {}/{} -> {}", tag, group, node, err.message);
    }
    if err.is_schema() {
        let col = col_letter(err.col);
        let field = if err.field.is_empty() { "?".to_string() } else { err.field.clone() };
        match err.header_row {
            Some(hr) => {
                let n = hr as usize;
                format!("{} {}/{} 表头第{}行{}列:[{}] -> {}", tag, group, node, n, col, field, err.message)
            }
            None => {
                format!("{} {}/{} 表头{}列:[{}] -> {}", tag, group, node, col, field, err.message)
            }
        }
    } else {
        let pos = format!("{}{}", col_letter(err.col), err.row + 1);
        let display = truncate_display(&err.value, 16);
        format!("{} {}/{} {}:[{}] -> {}", tag, group, node, pos, display, err.message)
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
    // 组名唯一性按忽略大小写判定（与 validate_group_name 一致）。
    let set: std::collections::HashSet<String> =
        existing.into_iter().map(str::to_lowercase).collect();
    let first = format!("{}_copy", base);
    if !set.contains(&first.to_lowercase()) { return first; }
    for n in 2..1000 {
        let candidate = format!("{}_copy{}", base, n);
        if !set.contains(&candidate.to_lowercase()) { return candidate; }
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

/// 项目内全部未删除节点名（Table / Constant / Enum 平铺命名空间）。
fn all_project_node_names(project: &Project) -> Vec<String> {
    let mut out = Vec::new();
    for g in &project.groups {
        out.extend(g.tables.iter().filter(|t| !t.deleted).map(|t| t.name.clone()));
        out.extend(g.constants.iter().filter(|c| !c.deleted).map(|c| c.name.clone()));
        out.extend(g.enums.iter().filter(|e| !e.deleted).map(|e| e.name.clone()));
    }
    out
}

/// 节点名冲突判定：项目级唯一，忽略大小写（与输入校验一致）。
fn node_name_taken(names: &[String], candidate: &str) -> bool {
    names.iter().any(|n| n.eq_ignore_ascii_case(candidate))
}

/// 在项目级节点名集合里取一个不冲突的名字，并把它加入集合。
/// 先试 `<base>_copy`，再 `_copy2`…，与组重名规则一致。
fn dedupe_node_name(base: &str, names: &mut Vec<String>) -> String {
    if !node_name_taken(names, base) {
        names.push(base.to_string());
        return base.to_string();
    }
    for n in 1..1000 {
        let candidate = if n == 1 {
            format!("{}_copy", base)
        } else {
            format!("{}_copy{}", base, n)
        };
        if !node_name_taken(names, &candidate) {
            names.push(candidate.clone());
            return candidate;
        }
    }
    let fallback = format!("{}_copy_{}", base, chrono::Local::now().timestamp_millis());
    names.push(fallback.clone());
    fallback
}

/// 把组内所有 `@Old` 类型引用 / `ref_exists(Old, ...)` 规则改写到映射后的名字。
fn rewrite_group_refs(group: &mut Group, mapping: &HashMap<String, String>) {
    if mapping.is_empty() {
        return;
    }
    for t in &mut group.tables {
        for f in &mut t.schema.fields {
            for (old, new) in mapping {
                if let Some(txt) = rename_ref_in_type(&f.tbl_type, old, new) {
                    f.tbl_type = txt;
                }
            }
        }
        rewrite_rule_refs(&mut t.rules, mapping);
    }
    for c in &mut group.constants {
        for e in &mut c.entries {
            for (old, new) in mapping {
                if let Some(txt) = rename_ref_in_type(&e.tbl_type, old, new) {
                    e.tbl_type = txt;
                }
            }
        }
    }
    for e in &mut group.enums {
        rewrite_rule_refs(&mut e.rules, mapping);
    }
}

fn rewrite_node_body_refs(body: &mut NodeBody, mapping: &HashMap<String, String>) {
    if mapping.is_empty() {
        return;
    }
    match body {
        NodeBody::Table(t) => {
            for f in &mut t.schema.fields {
                for (old, new) in mapping {
                    if let Some(txt) = rename_ref_in_type(&f.tbl_type, old, new) {
                        f.tbl_type = txt;
                    }
                }
            }
            rewrite_rule_refs(&mut t.rules, mapping);
        }
        NodeBody::Constant(c) => {
            for e in &mut c.entries {
                for (old, new) in mapping {
                    if let Some(txt) = rename_ref_in_type(&e.tbl_type, old, new) {
                        e.tbl_type = txt;
                    }
                }
            }
        }
        NodeBody::Enum(e) => rewrite_rule_refs(&mut e.rules, mapping),
    }
}

fn rewrite_rule_refs(rules: &mut [Rule], mapping: &HashMap<String, String>) {
    for r in rules {
        if let RuleKind::RefExists { target_node, .. } = &mut r.kind {
            if let Some(new) = mapping.get(target_node) {
                *target_node = new.clone();
            }
        }
    }
}

/// 取一个节点的完整快照，给撤销栈用。找不到返回 None。
fn capture_node_body(project: &Project, group: &str, name: &str, kind: NodeKind) -> Option<NodeBody> {
    let g = project.groups.iter().find(|g| g.name == group)?;
    match kind {
        NodeKind::Table => g.tables.iter().find(|t| t.name == name).cloned().map(NodeBody::Table),
        NodeKind::Constant => g.constants.iter().find(|c| c.name == name).cloned().map(NodeBody::Constant),
        NodeKind::Enum => g.enums.iter().find(|e| e.name == name).cloned().map(NodeBody::Enum),
    }
}

/// 取 Table 单元格的 `(值, 行数, 该行长度)`。行/列形状要一起存：
/// 往空白行输入会自动补行，撤销时必须把补出来的空行也收回去。
fn capture_table_cell(
    project: &Project,
    group: &str,
    name: &str,
    row: usize,
    col: usize,
) -> Option<(String, usize, usize)> {
    let g = project.groups.iter().find(|g| g.name == group)?;
    let t = g.tables.iter().find(|t| t.name == name)?;
    let value = t
        .records
        .get(row)
        .and_then(|r| r.get(col))
        .cloned()
        .unwrap_or_default();
    Some((value, t.records.len(), t.records.get(row).map_or(0, Vec::len)))
}

/// 把节点快照写回项目。节点级事务不改名，所以按事务里的名字定位。
fn restore_node_body(project: &mut Project, group: &str, kind: NodeKind, name: &str, body: &NodeBody) {
    let Some(g) = project.groups.iter_mut().find(|g| g.name == group) else { return };
    match (kind, body) {
        (NodeKind::Table, NodeBody::Table(snapshot)) => {
            if let Some(slot) = g.tables.iter_mut().find(|t| t.name == name) {
                *slot = snapshot.clone();
            } else {
                g.tables.push(snapshot.clone());
            }
            if let Some(t) = g.tables.iter_mut().find(|t| t.name == name) {
                t.update_dirty();
            }
        }
        (NodeKind::Constant, NodeBody::Constant(snapshot)) => {
            if let Some(slot) = g.constants.iter_mut().find(|c| c.name == name) {
                *slot = snapshot.clone();
            } else {
                g.constants.push(snapshot.clone());
            }
            if let Some(c) = g.constants.iter_mut().find(|c| c.name == name) {
                c.update_dirty();
            }
        }
        (NodeKind::Enum, NodeBody::Enum(snapshot)) => {
            if let Some(slot) = g.enums.iter_mut().find(|e| e.name == name) {
                *slot = snapshot.clone();
            } else {
                g.enums.push(snapshot.clone());
            }
            if let Some(e) = g.enums.iter_mut().find(|e| e.name == name) {
                e.update_dirty();
            }
        }
        _ => return,
    }
    sync_schema_from_groups(project);
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
            n += rename_rule_targets(&mut t.rules, old, new);
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
        // 枚举没有类型串，但可能有 ref_exists 规则（错误码 ↔ 多语言就是这种）。
        for e in &mut g.enums {
            if rename_rule_targets(&mut e.rules, old, new) > 0 {
                e.update_dirty();
            }
        }
    }
    n
}

/// 节点改名后，把规则里 `ref_exists(Old.列)` 的目标节点改写成 `New`。返回改写条数。
fn rename_rule_targets(rules: &mut [Rule], old: &str, new: &str) -> usize {
    let mut n = 0;
    for r in rules {
        if let RuleKind::RefExists { target_node, .. } = &mut r.kind {
            if target_node == old {
                *target_node = new.to_string();
                n += 1;
            }
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
        // 记住改名前盘上的名字：组改名块之后 `original_name` 就被就地更新了，
        // 而工作簿文件还叫旧名（目录搬完，里面的文件不会跟着改）。
        let old_group_name = group.original_name.clone();

        // Xlsx 组：根级 `<group>.xlsx`，没有组目录。改名只搬工作簿文件。
        if group.is_xlsx() {
            group.dir = data_dir.clone();
            let old_book = data_dir.join(format!("{}.xlsx", old_group_name));
            let new_book = data_dir.join(format!("{}.xlsx", group.name));
            let mut final_book = new_book.clone();
            if old_group_name != group.name && old_book.is_file() {
                match std::fs::rename(&old_book, &new_book) {
                    Ok(()) => {}
                    Err(e) => {
                        warnings.push(format!(
                            "重命名工作簿 {} → {} 失败，保留原名: {}",
                            old_book.display(), new_book.display(), e
                        ));
                        group.name = old_group_name.clone();
                        final_book = old_book;
                    }
                }
            }
            for t in &mut group.tables { t.path = final_book.clone(); }
            for c in &mut group.constants { c.path = final_book.clone(); }
            for e in &mut group.enums { e.path = final_book.clone(); }
            group.original_name = group.name.clone();
            continue;
        }

        // ── 组目录（仅 Tbl 组）──
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
/// 写盘。返回 `(写入文件数, 删除文件数, 警告, 是否失败)`。
///
/// 第四个元素是给「xlsx 组保存冲突 / 被 Excel 占用」这类**整组写不进去**的情况用的：
/// 调用方据此让 `save_all` 返回 false，别对外声称"已保存"。
fn save_project_files(project: &mut Project) -> (usize, usize, Vec<String>, bool) {
    let mut count = 0;
    let mut deleted = 0;
    let mut failed = false;
    let mut write_failures: Vec<String> = Vec::new();
    // 第一次落盘：盘上还什么都没有，schema 必须无条件写一份
    let first_save = project.state.is_pending();
    // 第一步：克隆出来但未落盘的项目，先建 root 目录 + 让 schema 一并落地
    if first_save {
        if let Err(e) = std::fs::create_dir_all(&project.project_root) {
            eprintln!("warn: create project root failed: {} ({})", project.project_root.display(), e);
            return (0, 0, Vec::new(), true);
        }
        project.state = crate::model::ProjectState::Loaded;

        // 未落盘项目首次保存时，根据当前 project_root 重新推演所有路径
        // （应对"项目设置"修改 id 的场景：id 改了、project_root 更新了，但子节点路径还是旧的）
        let new_data_dir = project.data_dir();
        for group in &mut project.groups {
            if group.is_xlsx() {
                // Xlsx 组：工作簿直接在 config 根，没有组目录。
                group.dir = new_data_dir.clone();
                let book = group.book_path();
                for table in &mut group.tables { table.path = book.clone(); }
                for constant in &mut group.constants { constant.path = book.clone(); }
                for enum_def in &mut group.enums { enum_def.path = book.clone(); }
                continue;
            }
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
    // Xlsx 组的**预检**：锁文件 / 外部改动 / sheet 名长度，全部在动盘之前问完。
    // 否则会出现「组改名已经搬了目录、schema 还没来得及写」——下次加载时组名对不上
    // schema section，那个组的节点会静默消失。
    {
        let mut msgs = Vec::new();
        for group in &project.groups {
            if group.is_xlsx() && xlsx_group_needs_write(group) {
                if let Some(m) = xlsx_preflight(group) {
                    msgs.push(m);
                }
            }
        }
        if !msgs.is_empty() {
            if first_save {
                project.state = crate::model::ProjectState::Pending;
            }
            return (0, 0, msgs, true);
        }
    }
    // `project.toml` 是**可选的 `[export]` 覆盖段**：只在用户真的改过 export 时才写。
    // 没改过就不该有文件——export 继续从工作区 `tablet.toml` 继承，改全局能传播。
    // 写的内容是项目**当前的** `config.export`（曾经这里写的是 `ExportConfig::default()`，
    // 于是"新建项目首次保存"会把从全局拷来的配置静默重置掉，@02.26.3）。
    if project.toml_changed() {
        let path = project.project_root.join(crate::project::PROJECT_TOML_FILE);
        match crate::project::project_toml_text(&project.config.export, &project.config.validate) {
            Ok(txt) => match std::fs::write(&path, txt) {
                Ok(()) => {
                    project.original_export = project.config.export.clone();
                    project.original_validate = project.config.validate.clone();
                    count += 1;
                }
                Err(e) => {
                    failed = true;
                    let msg = format!("[保存失败] 写 project.toml 失败: {} ({})", path.display(), e);
                    eprintln!("warn: {}", msg);
                    write_failures.push(msg);
                }
            },
            Err(e) => {
                failed = true;
                let msg = format!("[保存失败] 生成 project.toml 内容失败: {} ({})", path.display(), e);
                eprintln!("warn: {}", msg);
                write_failures.push(msg);
            }
        }
    }
    // schema 文件只在**真的会不一样**时才写：改一个格子的值不该动它。
    //
    // **判据是「和盘上的字不一样」，不是内存里的脏标。** `schema.sections` 是 groups 的
    // 投影，手改 `.tbl`（加一列 / 加一条）不会让任何 `original_*` 变化，于是
    // `schema_changed()` 为假 —— 旧代码就此跳过，`project.tblschema` 永远停在上一次
    // 结构变更时的样子。CLI 的工作流正是「改 .tbl → workspace save」，那条路必须把投影
    // 刷新掉，否则下一个 `project new --from-file` / `util scaffold` 拿到的还是旧结构。
    project.resync_schema();
    let schema_txt = crate::tblschema::serialize_tblschema(&project.schema);
    let schema_path = project.project_root.join(crate::project::PROJECT_SCHEMA_FILE);
    let schema_on_disk = std::fs::read_to_string(&schema_path).ok();
    let schema_needs_write = first_save
        || project.schema_changed()
        || schema_on_disk.as_deref() != Some(schema_txt.as_str());
    // **真正的写放在组循环之后**：schema 是 groups 的投影，如果某个组没写进去
    // （xlsx 被 Excel 占用 / 外部改过），先把 schema 落盘就会造出「schema 里有这个节点、
    // 盘上却没有它的数据」——项目当场打不开。要么一起成功，要么一起不写。
    let txt = schema_txt;
    let path = schema_path;
    // 改名先落位（盘上搬完），再走下面的写入循环——两边都靠 original_name 对齐
    let mut warnings = apply_pending_renames(project);
    // 「节点被删光」的组要在盘上和内存里都消失；**「本来就是空的新组」不能跟着殉葬**。
    // 两者在写入循环结束时看起来一样（三个 vec 都空），所以先记下它本来有没有节点。
    // CLI 是每条命令一个进程：`schema add-group` 保存后若把空组删掉，下一条
    // `schema add-table --group <新组>` 就查无此组 —— 这条链必须能跨进程走通。
    let mut emptied: Vec<String> = Vec::new();
    for group in &mut project.groups {
        // 「本来就有节点」（哪怕此刻已被标删）才可能在这次保存里变成"删空"。
        // 用 `!is_empty()` 而不是 `any(!deleted)`：`delete_group` 是先给所有节点打
        // `deleted` 再保存的，用后者会把"刚删光"误判成"本来就是空"。
        let had_any_nodes = !group.tables.is_empty()
            || !group.constants.is_empty()
            || !group.enums.is_empty();
        if group.is_new {
            let _ = std::fs::create_dir_all(&group.dir);
        }
        // Xlsx 组走自己的工作簿写入器：一次会话写完整组，脏的才写。
        // 注意**不能**落到下面的 .tbl 写法里——那会去 `dir/<Node>.tbl` 找不存在的文件。
        if group.is_xlsx() {
            let outcome = save_xlsx_group(group);
            count += outcome.written;
            deleted += outcome.deleted;
            warnings.extend(outcome.warnings);
            failed |= outcome.failed;
            if outcome.emptied {
                emptied.push(group.name.clone());
            }
            continue;
        }
        for table in &mut group.tables {
            if table.deleted {
                if table.saved {
                    let _ = std::fs::remove_file(&table.path);
                }
                deleted += 1;
            } else if table.dirty {
                let content = crate::tbl::serialize_table(table);
                match std::fs::write(&table.path, &content) {
                    Ok(()) => {
                        table.original_records = table.records.clone();
                        table.original_fields = table.schema.fields.clone();
                        table.original_display_field = table.display_field.clone();
                        table.original_rules = table.rules.clone();
                        // 落盘后「盘上叫什么」才追上新名字
                        table.original_name = table.name.clone();
                        table.dirty = false;
                        table.saved = true;
                        count += 1;
                    }
                    Err(e) => {
                        failed = true;
                        write_failures.push(format!(
                            "[保存失败] 写 {} 失败: {}",
                            table.path.display(), e
                        ));
                    }
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
                match std::fs::write(&constant.path, &content) {
                    Ok(()) => {
                        constant.original_entries = constant.entries.clone();
                        constant.original_name = constant.name.clone();
                        constant.dirty = false;
                        constant.saved = true;
                        count += 1;
                    }
                    Err(e) => {
                        failed = true;
                        write_failures.push(format!(
                            "[保存失败] 写 {} 失败: {}",
                            constant.path.display(), e
                        ));
                    }
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
                match std::fs::write(&enum_def.path, &content) {
                    Ok(()) => {
                        enum_def.original_entries = enum_def.entries.clone();
                        enum_def.original_name = enum_def.name.clone();
                        enum_def.original_display_field = enum_def.display_field.clone();
                        enum_def.original_rules = enum_def.rules.clone();
                        enum_def.dirty = false;
                        enum_def.saved = true;
                        count += 1;
                    }
                    Err(e) => {
                        failed = true;
                        write_failures.push(format!(
                            "[保存失败] 写 {} 失败: {}",
                            enum_def.path.display(), e
                        ));
                    }
                }
            }
        }
        group.tables.retain(|t| !t.deleted);
        group.constants.retain(|c| !c.deleted);
        group.enums.retain(|e| !e.deleted);
        let now_empty = group.tables.is_empty() && group.constants.is_empty() && group.enums.is_empty();
        if had_any_nodes && now_empty {
            // 是"删空"不是"本来就空"：盘上目录和内存里的组一起去掉
            if group.dir.is_dir() {
                let _ = std::fs::remove_dir_all(&group.dir);
            }
            emptied.push(group.name.clone());
        }
        group.is_new = false;
    }
    project.groups.retain(|g| !emptied.contains(&g.name));

    // 组都写成功了才写 schema 投影（见上面的注释）。
    if !failed && schema_needs_write {
        match std::fs::write(&path, txt) {
            Ok(()) => {
                // 落盘后「盘上是什么」才追上内存
                project.original_meta = project.schema.meta.clone();
                project.original_separators = project.schema.separators.clone();
                count += 1;
            }
            Err(e) => {
                failed = true;
                write_failures.push(format!(
                    "[保存失败] 写 {} 失败: {}",
                    path.display(), e
                ));
            }
        }
    }
    if failed && first_save {
        // 首次保存没走完：盘上还没有完整的项目，别让内存把它当成已落盘。
        project.state = crate::model::ProjectState::Pending;
    }
    warnings.extend(write_failures);
    (count, deleted, warnings, failed)
}

/// 这个 xlsx 组这次保存要不要动文件。
fn xlsx_group_needs_write(group: &crate::model::Group) -> bool {
    group.is_new
        || group.tables.iter().any(|t| !t.deleted && t.dirty || t.deleted)
        || group.constants.iter().any(|c| !c.deleted && c.dirty || c.deleted)
        || group.enums.iter().any(|e| !e.deleted && e.dirty || e.deleted)
}

/// 动盘之前的确定性检查：被 Excel 占用 / 外部改过 / sheet 名超长。
/// 返回 `Some(原因)` = 这次不能写。
fn xlsx_preflight(group: &crate::model::Group) -> Option<String> {
    let path = group.book_path();

    // Excel 正开着时会留 `~$xxx.xlsx`，这会儿写进去只会写坏。
    let lock = group.dir.join(format!("~${}.xlsx", group.name));
    if lock.exists() {
        return Some(format!(
            "[{}] 保存失败：{} 正被 Excel 打开，请先关闭 Excel",
            group.name, path.display()
        ));
    }

    // 外部改过：加载时的 (mtime, size) 对不上就不写，交给用户决定重新加载还是另存。
    if !group.is_new {
        if let Some(prev) = group.book_stamp {
            if crate::project::file_stamp(&path) != Some(prev) {
                return Some(format!(
                    "[{}] 保存失败：{} 已被外部修改，请重新加载后再改（或先备份再覆盖）",
                    group.name, path.display()
                ));
            }
        }
    }

    // 名字是 sheet 名的约束：段名规则不含非法字符，但可能超 31 字符。
    for name in group.tables.iter().filter(|t| !t.deleted).map(|t| &t.name)
        .chain(group.constants.iter().filter(|c| !c.deleted).map(|c| &c.name))
        .chain(group.enums.iter().filter(|e| !e.deleted).map(|e| &e.name))
    {
        if name.chars().count() > 31 {
            return Some(format!(
                "[{}] 保存失败：节点名 {:?} 超过 Excel sheet 名上限 31 字符，请改名",
                group.name, name
            ));
        }
    }
    None
}

/// Xlsx 组的写盘结果。
struct XlsxSaveOutcome {
    written: usize,
    deleted: usize,
    warnings: Vec<String>,
    /// 整组没写进去（被 Excel 占用 / 外部改过 / 写入失败）。调用方要让 `save_all` 返回 false。
    failed: bool,
    /// 节点被删光，组本身也该消失。
    emptied: bool,
}

impl XlsxSaveOutcome {
    fn ok() -> Self {
        Self { written: 0, deleted: 0, warnings: Vec::new(), failed: false, emptied: false }
    }
}

/// 把 Xlsx 组写回 `<group>.xlsx`（@02.27）。
///
/// 三条铁律，都来自对字节稳定性的实测：
/// 1. **不脏就不碰文件**。umya 每次写盘都会重打 zip + 打当前时间戳，字节必变；
///    "顺手保存"会让 git 每次多一个改动。
/// 2. **一次会话写完整组**。每开一次文件就重写整个容器，逐节点写等于反复重打。
/// 3. **只写脏 sheet**，其余 sheet 走 `lazy_read` 的原始字节直通（实测 CRC 不变）。
fn save_xlsx_group(group: &mut crate::model::Group) -> XlsxSaveOutcome {
    let mut out = XlsxSaveOutcome::ok();
    let path = group.book_path();

    let had_any_nodes = !group.tables.is_empty()
        || !group.constants.is_empty()
        || !group.enums.is_empty();
    let remaining = group.tables.iter().filter(|t| !t.deleted).count()
        + group.constants.iter().filter(|c| !c.deleted).count()
        + group.enums.iter().filter(|e| !e.deleted).count();

    if !xlsx_group_needs_write(group) {
        return out; // 干净 = 一个字节都不碰
    }
    if let Some(msg) = xlsx_preflight(group) {
        out.failed = true;
        out.warnings.push(msg);
        return out;
    }

    // 删空：整组消失（与 .tbl 组的语义一致）。不写一个零 sheet 的 workbook。
    if had_any_nodes && remaining == 0 {
        if path.is_file() {
            let _ = std::fs::remove_file(&path);
            out.deleted += 1;
        }
        // Xlsx 组的 dir 是 config 根，绝不能 remove_dir_all；只删工作簿本身。
        out.emptied = true;
        return out;
    }

    let existed = path.is_file();
    let mut book = if existed {
        match umya_spreadsheet::reader::xlsx::lazy_read(&path) {
            Ok(b) => b,
            Err(e) => {
                out.failed = true;
                out.warnings.push(format!("[{}] 打开 {} 失败: {}", group.name, path.display(), e));
                return out;
            }
        }
    } else {
        umya_spreadsheet::new_file()
    };

    // 空组也要能落盘（xlsx 至少要有一个 sheet），但那条占位 sheet 不该一直跟着：
    // 组里一旦有节点，就把占位表删掉。占位表用保留名 `_empty`——段名必须大写开头，
    // 所以它不可能和配置表撞名。
    //
    // **必须等新 sheet 建出来之后再删**：umya 不允许删掉最后一个 sheet，
    // 先删后建会静默失败（实测：空 Sheet1 一直跟着走）。
    const EMPTY_SHEET: &str = "_empty";
    let node_names: Vec<String> = group.tables.iter().filter(|t| !t.deleted).map(|t| t.name.clone())
        .chain(group.constants.iter().filter(|c| !c.deleted).map(|c| c.name.clone()))
        .chain(group.enums.iter().filter(|e| !e.deleted).map(|e| e.name.clone()))
        .collect();

    let mut placeholder: Option<String> = None;
    if !existed {
        // `new_file()` 自带一个 Sheet1
        let default = book.sheet_collection_no_check().first().map(|s| s.name().to_string());
        if let Some(d) = default {
            if node_names.is_empty() {
                if let Ok(ws) = book.sheet_by_name_mut(&d) {
                    ws.set_name(EMPTY_SHEET);
                }
            } else {
                placeholder = Some(d);
            }
        }
    } else if !node_names.is_empty()
        && book.sheet_collection_no_check().iter().any(|s| s.name() == EMPTY_SHEET)
    {
        placeholder = Some(EMPTY_SHEET.to_string());
    }

    // ── 新节点的名字不能撞上策划的私有 sheet ──
    // （新节点 `saved == false`；盘上已有同名 sheet 说明那是策划自己加的表，
    //   直接写进去会把人家的内容盖掉。）
    let plan: Vec<(String, String, bool)> = group.tables.iter().filter(|t| !t.deleted)
        .map(|t| (t.original_name.clone(), t.name.clone(), t.saved))
        .chain(group.constants.iter().filter(|c| !c.deleted)
            .map(|c| (c.original_name.clone(), c.name.clone(), c.saved)))
        .chain(group.enums.iter().filter(|e| !e.deleted)
            .map(|e| (e.original_name.clone(), e.name.clone(), e.saved)))
        .collect();
    for (_old, new, saved) in &plan {
        if *saved { continue; }
        if book.sheet_collection_no_check().iter().any(|s| s.name().eq_ignore_ascii_case(new)) {
            out.failed = true;
            out.warnings.push(format!(
                "[{}] 保存失败：工作簿里已有同名 sheet {:?}，但 schema 里没有对应节点；\
                 请先改名或删掉那张表", group.name, new
            ));
            return out;
        }
    }

    // ── 节点改名 = sheet 改名（先改，后面按新名找）──
    for (old, new, _) in &plan {
        if old == new { continue; }
        // 改名撞上别的 sheet 同样不行（Excel 的 sheet 名大小写不敏感）
        let clashes = book.sheet_collection_no_check().iter()
            .any(|s| s.name().eq_ignore_ascii_case(new) && !s.name().eq_ignore_ascii_case(old));
        if clashes {
            out.failed = true;
            out.warnings.push(format!(
                "[{}] 保存失败：sheet 改名 {:?} → {:?} 撞上已有 sheet", group.name, old, new
            ));
            return out;
        }
        if let Ok(ws) = book.sheet_by_name_mut(old) {
            ws.set_name(new);
        }
    }

    // ── 删除节点 = 删 sheet ──
    for (old, deleted) in group.tables.iter().map(|t| (t.original_name.clone(), t.deleted))
        .chain(group.constants.iter().map(|c| (c.original_name.clone(), c.deleted)))
        .chain(group.enums.iter().map(|e| (e.original_name.clone(), e.deleted)))
    {
        if deleted {
            if book.sheet_collection_no_check().iter().any(|s| s.name() == old) {
                if book.remove_sheet_by_name(&old).is_ok() {
                    out.deleted += 1;
                }
            }
        }
    }

    // ── 写脏节点 ──
    for table in group.tables.iter_mut().filter(|t| !t.deleted && t.dirty) {
        let name = table.name.clone();
        let ws = match ensure_sheet(&mut book, &name) {
            Ok(ws) => ws,
            Err(e) => { out.failed = true; out.warnings.push(e); return out; }
        };
        if let Err(e) = write_table_sheet(ws, table) {
            out.failed = true;
            out.warnings.push(format!("[{}] 写 sheet {} 失败: {}", group.name, name, e));
            return out;
        }
        table.original_records = table.records.clone();
        table.original_fields = table.schema.fields.clone();
        table.original_name = table.name.clone();
        table.original_display_field = table.display_field.clone();
        table.original_rules = table.rules.clone();
        table.dirty = false;
        table.saved = true;
        out.written += 1;
    }
    for constant in group.constants.iter_mut().filter(|c| !c.deleted && c.dirty) {
        let name = constant.name.clone();
        let ws = match ensure_sheet(&mut book, &name) {
            Ok(ws) => ws,
            Err(e) => { out.failed = true; out.warnings.push(e); return out; }
        };
        write_constant_sheet(ws, constant);
        constant.original_entries = constant.entries.clone();
        constant.original_name = constant.name.clone();
        constant.dirty = false;
        constant.saved = true;
        out.written += 1;
    }
    for enum_def in group.enums.iter_mut().filter(|e| !e.deleted && e.dirty) {
        let name = enum_def.name.clone();
        let ws = match ensure_sheet(&mut book, &name) {
            Ok(ws) => ws,
            Err(e) => { out.failed = true; out.warnings.push(e); return out; }
        };
        write_enum_sheet(ws, enum_def);
        enum_def.original_entries = enum_def.entries.clone();
        enum_def.original_name = enum_def.name.clone();
        enum_def.original_display_field = enum_def.display_field.clone();
        enum_def.original_rules = enum_def.rules.clone();
        enum_def.dirty = false;
        enum_def.saved = true;
        out.written += 1;
    }

    // 节点 sheet 都建好了，这时候才有资格删那个占位 Sheet1。
    if let Some(p) = placeholder {
        if book.sheet_collection_no_check().len() > 1 {
            let _ = book.remove_sheet_by_name(&p);
            out.deleted += 1;
        }
    }

    // ── 落盘：临时文件 + rename，避免半截 workbook ──
    let tmp = group.dir.join(format!("{}.xlsx.tmp", group.name));
    if let Err(e) = umya_spreadsheet::writer::xlsx::write(&book, &tmp) {
        let _ = std::fs::remove_file(&tmp);
        out.failed = true;
        out.warnings.push(format!("[{}] 写入 {} 失败: {}", group.name, path.display(), e));
        return out;
    }
    if let Err(e) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        out.failed = true;
        out.warnings.push(format!("[{}] 替换 {} 失败: {}", group.name, path.display(), e));
        return out;
    }

    group.tables.retain(|t| !t.deleted);
    group.constants.retain(|c| !c.deleted);
    group.enums.retain(|e| !e.deleted);
    group.original_name = group.name.clone();
    group.is_new = false;
    group.book_stamp = crate::project::file_stamp(&path);

    out
}

/// 找到 sheet；没有就新建。名字大小写按 Excel 口径（不敏感）判重。
fn ensure_sheet<'a>(
    book: &'a mut umya_spreadsheet::Workbook,
    name: &str,
) -> Result<&'a mut umya_spreadsheet::Worksheet, String> {
    let existing = book.sheet_collection_no_check().iter()
        .find(|s| s.name().eq_ignore_ascii_case(name))
        .map(|s| s.name().to_string());
    if let Some(actual) = existing {
        return book.sheet_by_name_mut(&actual)
            .map_err(|e| format!("打开 sheet {} 失败: {}", actual, e));
    }
    book.new_sheet(name).map_err(|e| format!("新建 sheet {} 失败: {}", name, e))?;
    book.sheet_by_name_mut(name)
        .map_err(|e| format!("打开 sheet {} 失败: {}", name, e))
}

/// 把一个 Table 写进 sheet：4 行表头 + 数据，列按**字段名**对齐（策划的私有列原样留着）。
fn write_table_sheet(
    ws: &mut umya_spreadsheet::Worksheet,
    table: &crate::model::Table,
) -> Result<(), String> {
    use crate::excel_sync::vo::{DATA_START, HDR_DESC, HDR_EXPORT, HDR_NAME, HDR_TYPE};

    let fields = &table.schema.fields;
    let (max_col, max_row) = ws.highest_column_and_row();
    let mut next_col = max_col + 1;
    let mut col_of: Vec<u32> = Vec::with_capacity(fields.len());
    for f in fields {
        let found = (1..=max_col).find(|&c| ws.value((c, HDR_NAME)) == f.name);
        let c = match found {
            Some(c) => c,
            None => { let c = next_col; next_col += 1; c }
        };
        col_of.push(c);
    }

    for (i, f) in fields.iter().enumerate() {
        let c = col_of[i];
        ws.cell_mut((c, HDR_DESC)).set_value(f.desc.clone());
        ws.cell_mut((c, HDR_EXPORT)).set_value(f.export.display().to_string());
        ws.cell_mut((c, HDR_TYPE)).set_value(f.tbl_type.clone());
        ws.cell_mut((c, HDR_NAME)).set_value(f.name.clone());
    }

    for (ri, row) in table.records.iter().enumerate() {
        let r = DATA_START + ri as u32;
        for (i, v) in row.iter().enumerate() {
            if let Some(&c) = col_of.get(i) {
                ws.cell_mut((c, r)).set_value(v.clone());
            }
        }
    }

    // 记录变少时清掉 schema 列里的旧数据行；私有列一概不碰。
    let first_stale = DATA_START + table.records.len() as u32;
    if max_row >= first_stale {
        for r in first_stale..=max_row {
            for &c in &col_of {
                if !ws.value((c, r)).is_empty() {
                    ws.cell_mut((c, r)).set_value("");
                }
            }
        }
    }
    Ok(())
}

fn write_constant_sheet(ws: &mut umya_spreadsheet::Worksheet, constant: &crate::model::Constant) {
    const HEADERS: [&str; 5] = ["name", "type", "value", "export", "desc"];
    for (i, h) in HEADERS.iter().enumerate() {
        ws.cell_mut((i as u32 + 1, 1)).set_value(*h);
    }
    for (ri, e) in constant.entries.iter().enumerate() {
        let r = ri as u32 + 2;
        ws.cell_mut((1, r)).set_value(e.name.clone());
        ws.cell_mut((2, r)).set_value(e.tbl_type.clone());
        ws.cell_mut((3, r)).set_value(e.value.clone());
        ws.cell_mut((4, r)).set_value(e.export.display().to_string());
        ws.cell_mut((5, r)).set_value(e.desc.clone());
    }
    clear_stale_rows(ws, constant.entries.len() as u32 + 2, 5);
}

fn write_enum_sheet(ws: &mut umya_spreadsheet::Worksheet, enum_def: &crate::model::EnumDef) {
    const HEADERS: [&str; 3] = ["id", "name", "desc"];
    for (i, h) in HEADERS.iter().enumerate() {
        ws.cell_mut((i as u32 + 1, 1)).set_value(*h);
    }
    for (ri, e) in enum_def.entries.iter().enumerate() {
        let r = ri as u32 + 2;
        ws.cell_mut((1, r)).set_value(e.id.clone());
        ws.cell_mut((2, r)).set_value(e.name.clone());
        ws.cell_mut((3, r)).set_value(e.desc.clone());
    }
    clear_stale_rows(ws, enum_def.entries.len() as u32 + 2, 3);
}

fn clear_stale_rows(ws: &mut umya_spreadsheet::Worksheet, first_stale: u32, cols: u32) {
    let (_, max_row) = ws.highest_column_and_row();
    if max_row < first_stale { return; }
    for r in first_stale..=max_row {
        for c in 1..=cols {
            if !ws.value((c, r)).is_empty() {
                ws.cell_mut((c, r)).set_value("");
            }
        }
    }
}

/// 一个「外部改过」的 xlsx 组。
pub struct XlsxExternalChange {
    pub project_id: String,
    pub group: String,
    /// 内存里是否有未保存改动（有就不能直接重载）。
    pub dirty: bool,
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
    /// 全 Project 的**警告**集（只 Warning），key=(project_id, group, name)。
    /// 与 `validation_errors` 并行维护：警告不进保存闸门 / 树 `!` / 红框，
    /// 只在保存成功时打进日志、在状态栏提示。
    pub validation_warnings: Vec<(String, String, String, ValidationError)>,
    /// 进程内剪贴板：tree 面板复制粘贴的载体。不持久化，关闭/退出随 engine drop。
    pub node_clipboard: Option<NodeClipboard>,
    /// 启动时从 workspace `tablet.toml [separators]` 读到的"程序级默认分隔符"；
    /// 仅用作「新建空项目」时 schema.separators 的初值（从模板/文件复制时取 source.separators）。
    /// 已加载项目的 separators 各自走 schema，运行期不再读这里。
    pub default_separators: crate::types::SeparatorsSection,
    pub logs: Vec<String>,
    /// 最近一次保存失败的原因（xlsx 组被占用 / 外部改过 …）。
    /// 引擎日志是给 GUI 看的；CLI 需要一句能直接打给 stderr 的原因。
    pub last_save_failure: Option<String>,
    /// 每个打开的 Project 一份会话内撤销/重做历史（docs/12）。
    /// 按 project id 存，不按 active_idx：切换 active 不清栈，粘贴/改名必须在目标项目记账。
    histories: HashMap<String, EditHistory>,
    /// 正在执行 undo/redo 时为 true，避免恢复动作又被记成新事务。
    history_applying: bool,
}

/// 表是否依赖 `names` 里任一节点（类型 `@Xxx` 或规则的 `ref_exists`）。
fn table_depends_on(t: &Table, names: &std::collections::HashSet<String>) -> bool {
    t.schema.fields.iter().any(|f| type_refs_any(&f.tbl_type, names))
        || t.rules.iter().any(|r| rule_refs_any(r, names))
}

/// 常量是否依赖 `names` 里任一节点（条目类型 `@Xxx`）。
fn constant_depends_on(c: &Constant, names: &std::collections::HashSet<String>) -> bool {
    c.entries.iter().any(|e| type_refs_any(&e.tbl_type, names))
}

/// 枚举是否依赖 `names` 里任一节点（规则的 `ref_exists`）。
fn enum_depends_on(e: &EnumDef, names: &std::collections::HashSet<String>) -> bool {
    e.rules.iter().any(|r| rule_refs_any(r, names))
}

fn type_refs_any(tbl_type: &str, names: &std::collections::HashSet<String>) -> bool {
    match crate::types::TblType::parse(tbl_type) {
        Some(t) => t.ref_targets().iter().any(|r| names.contains(*r)),
        None => false,
    }
}

fn rule_refs_any(r: &Rule, names: &std::collections::HashSet<String>) -> bool {
    matches!(&r.kind, RuleKind::RefExists { target_node, .. } if names.contains(target_node))
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

/// 粘贴组预览里的一行。
#[derive(Clone, Debug)]
pub struct PasteNodePreview {
    pub kind: NodeKind,
    pub old_name: String,
    pub new_name: String,
    pub conflict: bool,
}

/// 粘贴组前的只读预览：用于「有冲突才弹命名确认」。
#[derive(Clone, Debug)]
pub struct PasteGroupPreview {
    pub group_name: String,
    pub group_conflict: bool,
    pub nodes: Vec<PasteNodePreview>,
    pub has_conflict: bool,
}

/// 跨 project 复制粘贴的节点载荷（深拷贝）。
#[derive(Clone, Debug, PartialEq)]
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
    /// 新建组。`storage` 决定这个组的数据落在 `.tbl` 目录还是单个 `.xlsx`
    /// （一组一种后端，不混存，@02.27）。
    NewGroup    { project_id: String, name: String, storage: crate::model::GroupStorage },
    NewTable    { project_id: String, group: String, name: String, fmt_ver: crate::tbl::TblVersion },
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
            validation_warnings: Vec::new(),
            node_clipboard: None,
            default_separators: crate::types::SeparatorsSection::default(),
            logs: Vec::new(),
            last_save_failure: None,
            histories: HashMap::new(),
            history_applying: false,
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
            validation_warnings: Vec::new(),
            node_clipboard: None,
            default_separators: crate::types::SeparatorsSection::default(),
            logs: Vec::new(),
            last_save_failure: None,
            histories: HashMap::new(),
            history_applying: false,
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
            validation_warnings: Vec::new(),
            node_clipboard: None,
            default_separators: crate::types::SeparatorsSection::default(),
            logs: Vec::new(),
            last_save_failure: None,
            histories: HashMap::new(),
            history_applying: false,
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
        self.histories.remove(project_id);
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
        self.histories.remove(project_id);
        self.validation_errors.retain(|(p, _, _, _, _)| p != project_id);
        self.validation_warnings.retain(|(p, _, _, _)| p != project_id);
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
            validate: crate::model::ValidateConfig::default(),
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
            original_validate: crate::model::ValidateConfig::default(),
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
        self.histories.remove(&project_id);
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

        // 警告（如「多语言表还没建」）不阻创建；只有 Error 才撤回。
        let issues = self.validate_project_issues(&id);
        let policy = ValidateConfig::default(); // 新项目还没有自定义策略
        let errs: Vec<String> = issues.iter()
            .filter(|(_, _, e)| policy.action(e.severity) == crate::model::SeverityAction::Block)
            .map(|(g, n, e)| format_validation_log(g, n, e))
            .collect();
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
        new_project.original_validate = new_project.config.validate.clone();
        new_project.state = crate::model::ProjectState::Pending;

        let new_data_dir = new_project.data_dir();
        for g in &mut new_project.groups {
            g.original_name = g.name.clone();
            g.is_new = true;
            if g.is_xlsx() {
                g.dir = new_data_dir.clone();
                let book = g.book_path();
                for t in &mut g.tables {
                    t.path = book.clone();
                    t.dirty = true;
                    t.original_records.clear();
                    t.original_fields.clear();
                    t.original_rules.clear();
                    t.saved = false;
                    t.original_name = t.name.clone();
                }
                for c in &mut g.constants {
                    c.path = book.clone();
                    c.dirty = true;
                    c.original_entries.clear(); c.saved = false;
                    c.original_name = c.name.clone();
                }
                for e in &mut g.enums {
                    e.path = book.clone();
                    e.dirty = true;
                    e.original_entries.clear(); e.saved = false;
                    e.original_rules.clear();
                    e.original_name = e.name.clone();
                }
                continue;
            }
            g.dir = new_data_dir.join(&g.name);
            for t in &mut g.tables {
                t.path = g.dir.join(format!("{}.tbl", t.name));
                t.dirty = true;
                t.original_records.clear();
                t.original_fields.clear();
                t.original_rules.clear();
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
                e.original_rules.clear();
                e.original_name = e.name.clone();
            }
        }
        // 同步 schema.sections 与 groups（克隆下来的 sections 已经一致，但保险起见走一次）
        sync_schema_from_groups(&mut new_project);
        let avail = AvailableProject::from_project(&new_project);
        self.available_projects.push(avail);
        self.available_projects.sort_by(|a, b| a.id.cmp(&b.id));
        self.projects.push(new_project);
        self.histories.remove(new_id);
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

    // ── 会话内撤销/重做（docs/12）────────────────────────────────────────

    /// active Project 是否还能撤销 / 重做。
    pub fn can_undo(&self) -> bool {
        self.active_project_id()
            .and_then(|pid| self.histories.get(pid))
            .is_some_and(EditHistory::can_undo)
    }

    pub fn can_redo(&self) -> bool {
        self.active_project_id()
            .and_then(|pid| self.histories.get(pid))
            .is_some_and(EditHistory::can_redo)
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.active_project_id()
            .and_then(|pid| self.histories.get(pid))
            .and_then(EditHistory::undo_label)
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.active_project_id()
            .and_then(|pid| self.histories.get(pid))
            .and_then(EditHistory::redo_label)
    }

    pub fn history_len(&self, project_id: &str) -> (usize, usize) {
        self.histories
            .get(project_id)
            .map(|h| (h.undo_len(), h.redo_len()))
            .unwrap_or((0, 0))
    }

    /// 把指定 Project 的历史截断到 `undo_len` 步，并清空 redo。
    /// 模态编辑面板打开时记下长度，取消时调用，避免把面板内的临时编辑留在历史里。
    pub fn truncate_history(&mut self, project_id: &str, undo_len: usize) {
        if let Some(h) = self.histories.get_mut(project_id) {
            h.truncate_undo(undo_len);
        }
    }

    /// 清空指定 Project 的撤销/重做栈（保存成功、关闭、重载时调用）。
    pub fn clear_history(&mut self, project_id: &str) {
        if let Some(h) = self.histories.get_mut(project_id) {
            h.clear();
        }
    }

    pub fn clear_all_histories(&mut self) {
        self.histories.clear();
    }

    /// 撤销 active Project 的上一步。返回是否真的撤销了。
    pub fn undo(&mut self) -> bool {
        let Some(pid) = self.active_project_id().map(str::to_string) else {
            return false;
        };
        let Some(tx) = self.histories.get_mut(&pid).and_then(EditHistory::pop_undo) else {
            return false;
        };
        self.history_applying = true;
        for op in tx.ops.iter().rev() {
            self.apply_edit_op(op, true);
        }
        self.history_applying = false;
        if let Some(h) = self.histories.get_mut(&pid) {
            h.push_redo(tx.clone());
        }
        self.revalidate_all_projects();
        self.ui_log(format!("[撤销] {}（{}）", tx.label, pid));
        true
    }

    /// 重做 active Project 的下一步。返回是否真的重做了。
    pub fn redo(&mut self) -> bool {
        let Some(pid) = self.active_project_id().map(str::to_string) else {
            return false;
        };
        let Some(tx) = self.histories.get_mut(&pid).and_then(EditHistory::pop_redo) else {
            return false;
        };
        self.history_applying = true;
        for op in tx.ops.iter() {
            self.apply_edit_op(op, false);
        }
        self.history_applying = false;
        if let Some(h) = self.histories.get_mut(&pid) {
            h.push_undo_after_redo(tx.clone());
        }
        self.revalidate_all_projects();
        self.ui_log(format!("[重做] {}（{}）", tx.label, pid));
        true
    }

    fn push_transaction(&mut self, project_id: &str, label: &str, ops: Vec<EditOp>) {
        if self.history_applying || ops.is_empty() {
            return;
        }
        self.histories
            .entry(project_id.to_string())
            .or_default()
            .push(Transaction::new(label, ops));
    }

    /// 包住一次「会改 Project 内容」的操作：比较前后整个 Project，变了才记一条事务。
    /// 用于节点/组改名、粘贴、组增删等结构操作；高频单元格编辑不要走这里。
    fn with_project_tx<R>(
        &mut self,
        project_id: &str,
        label: &str,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        if self.history_applying {
            return f(self);
        }
        let before = self.find_project(project_id).cloned();
        let out = f(self);
        let after = self.find_project(project_id).cloned();
        if let (Some(before), Some(after)) = (before, after) {
            if before != after {
                self.push_transaction(
                    project_id,
                    label,
                    vec![EditOp::Project {
                        project_id: project_id.to_string(),
                        before,
                        after,
                    }],
                );
            }
        }
        out
    }

    /// 包住一次「只改单个节点」的操作：只存该节点的前后快照。
    fn with_node_tx<R>(
        &mut self,
        group: &str,
        name: &str,
        kind: NodeKind,
        label: &str,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        if self.history_applying {
            return f(self);
        }
        let pid = self.active_project_id().map(str::to_string);
        let before = pid
            .as_deref()
            .and_then(|pid| self.find_project(pid))
            .and_then(|p| capture_node_body(p, group, name, kind));
        let out = f(self);
        if let (Some(pid), Some(before)) = (pid, before) {
            if let Some(after) = self
                .find_project(&pid)
                .and_then(|p| capture_node_body(p, group, name, kind))
            {
                if before != after {
                    self.push_transaction(
                        &pid,
                        label,
                        vec![EditOp::Node {
                            project: pid.clone(),
                            group: group.to_string(),
                            kind,
                            name: name.to_string(),
                            before,
                            after,
                        }],
                    );
                }
            }
        }
        out
    }

    /// 包住一次 Table 单元格写入：存前后值 + 行/列形状。
    fn with_table_cell_tx<R>(
        &mut self,
        group: &str,
        name: &str,
        row: usize,
        col: usize,
        label: &str,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        if self.history_applying {
            return f(self);
        }
        let pid = self.active_project_id().map(str::to_string);
        let before = pid
            .as_deref()
            .and_then(|pid| self.find_project(pid))
            .and_then(|p| capture_table_cell(p, group, name, row, col));
        let out = f(self);
        if let (Some(pid), Some(before)) = (pid, before) {
            if let Some(after) = self
                .find_project(&pid)
                .and_then(|p| capture_table_cell(p, group, name, row, col))
            {
                if before != after {
                    self.push_transaction(
                        &pid,
                        label,
                        vec![EditOp::Cell(CellSnapshot {
                            project: pid.clone(),
                            group: group.to_string(),
                            node: name.to_string(),
                            row,
                            col,
                            before: before.0,
                            after: after.0,
                            before_rows: before.1,
                            after_rows: after.1,
                            before_row_len: before.2,
                            after_row_len: after.2,
                        })],
                    );
                }
            }
        }
        out
    }

    fn apply_edit_op(&mut self, op: &EditOp, undo: bool) {
        match op {
            EditOp::Cell(cell) => self.apply_cell_snapshot(cell, undo),
            EditOp::Node { project, group, kind, name, before, after } => {
                let body = if undo { before } else { after };
                if let Some(p) = self.find_project_mut(project) {
                    restore_node_body(p, group, *kind, name, body);
                }
            }
            EditOp::Project { project_id, before, after } => {
                let snapshot = if undo { before } else { after };
                if let Some(idx) = self.projects.iter().position(|p| p.schema.meta.id == *project_id) {
                    self.projects[idx] = snapshot.clone();
                }
            }
        }
    }

    fn apply_cell_snapshot(&mut self, cell: &CellSnapshot, undo: bool) {
        let Some(p) = self.find_project_mut(&cell.project) else { return };
        let Some(g) = p.groups.iter_mut().find(|g| g.name == cell.group) else { return };
        let Some(t) = g.tables.iter_mut().find(|t| t.name == cell.node) else { return };
        let (value, target_rows, target_row_len) = if undo {
            (&cell.before, cell.before_rows, cell.before_row_len)
        } else {
            (&cell.after, cell.after_rows, cell.after_row_len)
        };
        if t.records.len() > target_rows {
            t.records.truncate(target_rows);
        }
        while t.records.len() < target_rows {
            t.records.push(vec![String::new(); t.schema.fields.len()]);
        }
        if cell.row < target_rows {
            let record = &mut t.records[cell.row];
            if record.len() > target_row_len {
                record.truncate(target_row_len);
            }
            while record.len() < target_row_len {
                record.push(String::new());
            }
            if cell.col < target_row_len {
                record[cell.col] = value.clone();
            }
        }
        t.update_dirty();
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

    /// 取节点的模式与规则（规则面板 / 校验用）。节点不存在返回 None。
    pub fn node_rules(&self, group: &str, name: &str) -> Option<(NodeMode, Vec<Rule>)> {
        let g = self.project().groups.iter().find(|g| g.name == group)?;
        if let Some(t) = g.tables.iter().find(|t| t.name == name) {
            return Some((NodeMode::Table, t.rules.clone()));
        }
        // 常量不支持规则（@01.8.8）
        if let Some(e) = g.enums.iter().find(|e| e.name == name) {
            return Some((NodeMode::Enum, e.rules.clone()));
        }
        None
    }

    /// 写回节点规则（顺序即优先级）。返回是否找到节点。
    pub fn set_node_rules(&mut self, group: &str, name: &str, rules: Vec<Rule>) -> bool {
        let Some(pid) = self.active_project_id().map(str::to_string) else {
            return false;
        };
        let mut did = false;
        self.with_project_tx(&pid, "修改校验规则", |engine| {
            let g = match engine.project_mut().groups.iter_mut().find(|g| g.name == group) {
                Some(g) => g,
                None => return,
            };
            if let Some(t) = g.tables.iter_mut().find(|t| t.name == name) {
                t.rules = rules;
                t.update_dirty();
                did = true;
                return;
            }
            if let Some(e) = g.enums.iter_mut().find(|e| e.name == name) {
                e.rules = rules;
                e.update_dirty();
                did = true;
            }
        });
        did
    }

    /// 设置 Table / Enum 的「被引用展示列」（@10）。`None` = 回到默认。
    /// 走节点级事务：这是 project.tblschema 里的属性，应该能撤销。
    pub fn set_display_field(
        &mut self,
        group: &str,
        name: &str,
        kind: NodeKind,
        value: Option<String>,
    ) -> bool {
        if !matches!(kind, NodeKind::Table | NodeKind::Enum) {
            return false;
        }
        let mut did = false;
        self.with_node_tx(group, name, kind, "设置被引用展示列", |engine| {
            let Some(g) = engine.project_mut().groups.iter_mut().find(|g| g.name == group) else {
                return;
            };
            match kind {
                NodeKind::Table => {
                    if let Some(t) = g.tables.iter_mut().find(|t| t.name == name) {
                        if t.display_field != value {
                            t.display_field = value;
                            t.update_dirty();
                            did = true;
                        }
                    }
                }
                NodeKind::Enum => {
                    if let Some(e) = g.enums.iter_mut().find(|e| e.name == name) {
                        if e.display_field != value {
                            e.display_field = value;
                            e.update_dirty();
                            did = true;
                        }
                    }
                }
                NodeKind::Constant => {}
            }
        });
        if did {
            self.project_mut().resync_schema();
        }
        did
    }

    /// 在 active Project 上执行一次「任意改组内数据」的用户动作，并记一条 Project 快照。
    /// 给 schema 导入这类一次改多个节点的入口用；调用方不要在闭包里再调本引擎的
    /// 其他可撤销方法，否则会嵌套成两条事务。
    pub fn edit_active_project<R>(
        &mut self,
        label: &str,
        f: impl FnOnce(&mut Project) -> R,
    ) -> Option<R> {
        let pid = self.active_project_id().map(str::to_string)?;
        Some(self.with_project_tx(&pid, label, |engine| f(engine.project_mut())))
    }

    /// 同 [`edit_active_project`](Self::edit_active_project)，但显式指定项目。
    /// 给 Excel 回写这类「目标项目不一定是 active」的入口用。
    pub fn edit_project<R>(
        &mut self,
        project_id: &str,
        label: &str,
        f: impl FnOnce(&mut Project) -> R,
    ) -> Option<R> {
        if !self.is_opened(project_id) {
            return None;
        }
        Some(self.with_project_tx(project_id, label, |engine| {
            f(engine.find_project_mut(project_id).expect("opened checked"))
        }))
    }

    /// 节点可挂规则的列名（顺序与字段声明一致）。
    pub fn node_columns(&self, group: &str, name: &str) -> Vec<String> {
        let g = match self.project().groups.iter().find(|g| g.name == group) {
            Some(g) => g,
            None => return Vec::new(),
        };
        if let Some(t) = g.tables.iter().find(|t| t.name == name) {
            return t.schema.fields.iter().map(|f| f.name.clone()).collect();
        }
        // 常量不支持规则（@01.8.8）
        if g.enums.iter().any(|e| e.name == name) {
            return ENUM_RULE_COLUMNS.iter().map(|s| s.to_string()).collect();
        }
        Vec::new()
    }

    /// 节点各列的值类（规则可用性判断）。`None` = 该列类型不支持规则。
    pub fn node_column_classes(&self, group: &str, name: &str) -> Vec<(String, Option<ValueClass>)> {
        let g = match self.project().groups.iter().find(|g| g.name == group) {
            Some(g) => g,
            None => return Vec::new(),
        };
        if let Some(t) = g.tables.iter().find(|t| t.name == name) {
            return table_column_specs(t);
        }
        if g.constants.iter().any(|c| c.name == name) {
            // 常量不支持规则（@01.8.8）
            return Vec::new();
        }
        if g.enums.iter().any(|e| e.name == name) {
            return enum_column_specs();
        }
        Vec::new()
    }

    /// 按节点名取列规格（节点名项目内唯一）。目标节点下拉用。
    pub fn node_column_classes_by_name(&self, name: &str) -> Vec<(String, Option<ValueClass>)> {
        for g in &self.project().groups {
            if g.tables.iter().any(|t| t.name == name)
                || g.constants.iter().any(|c| c.name == name)
                || g.enums.iter().any(|e| e.name == name) {
                return self.node_column_classes(&g.name, name);
            }
        }
        Vec::new()
    }

    /// 项目内可作为 `ref_exists` 目标的节点名（Table / Enum，不含已删）。
    pub fn rule_target_nodes(&self) -> Vec<String> {
        let mut out = Vec::new();
        for g in &self.project().groups {
            for t in &g.tables { if !t.deleted { out.push(t.name.clone()); } }
            for e in &g.enums { if !e.deleted { out.push(e.name.clone()); } }
        }
        out
    }

    pub fn mark_enum_dirty(&mut self, group: &str, name: &str) {
        if let Some(e) = self.find_enum_mut(group, name) {
            e.update_dirty();
        }
    }

    pub fn set_enum_cell(&mut self, group: &str, name: &str, row: usize, col: usize, val: &str) {
        self.with_node_tx(group, name, NodeKind::Enum, "输入单元格", |engine| {
            if let Some(g) = engine.project_mut().groups.iter_mut().find(|g| g.name == group) {
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
        });
    }

    pub fn commit_enum_cell(&mut self, group: &str, name: &str, row: usize, col: usize, val: String) {
        self.with_node_tx(group, name, NodeKind::Enum, "输入单元格", |engine| {
            if let Some(g) = engine.project_mut().groups.iter_mut().find(|g| g.name == group) {
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
        });
    }

    pub fn clear_enum_cells(&mut self, group: &str, name: &str, cells: &[(usize, usize)]) {
        self.with_node_tx(group, name, NodeKind::Enum, "清空单元格", |engine| {
            if let Some(g) = engine.project_mut().groups.iter_mut().find(|g| g.name == group) {
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
        });
    }

    pub fn paste_enum_data(&mut self, group: &str, name: &str, start_row: usize, start_col: usize, grid: &[Vec<String>]) {
        if grid.is_empty() { return; }
        self.with_node_tx(group, name, NodeKind::Enum, "粘贴数据", |engine| {
            if let Some(g) = engine.project_mut().groups.iter_mut().find(|g| g.name == group) {
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
            engine.ui_log(format!("粘贴 {}行 数据", grid.len()));
        });
    }

    pub fn insert_enum_row(&mut self, group: &str, name: &str, at: usize) {
        self.with_node_tx(group, name, NodeKind::Enum, "插入行", |engine| {
            if let Some(g) = engine.project_mut().groups.iter_mut().find(|g| g.name == group) {
                if let Some(en) = g.enums.iter_mut().find(|e| e.name == name) {
                    let at = at.min(en.entries.len());
                    en.entries.insert(at, EnumEntry::default());
                    en.update_dirty();
                }
            }
        });
    }

    pub fn delete_enum_row(&mut self, group: &str, name: &str, at: usize) {
        self.with_node_tx(group, name, NodeKind::Enum, "删除行", |engine| {
            if let Some(g) = engine.project_mut().groups.iter_mut().find(|g| g.name == group) {
                if let Some(en) = g.enums.iter_mut().find(|e| e.name == name) {
                    if at < en.entries.len() {
                        en.entries.remove(at);
                        en.update_dirty();
                    }
                }
            }
        });
    }

    pub fn delete_enum_rows(&mut self, group: &str, name: &str, start: usize, end: usize) {
        self.with_node_tx(group, name, NodeKind::Enum, "删除行", |engine| {
            if let Some(g) = engine.project_mut().groups.iter_mut().find(|g| g.name == group) {
                if let Some(en) = g.enums.iter_mut().find(|e| e.name == name) {
                    let end = end.min(en.entries.len());
                    if start < end {
                        en.entries.drain(start..end);
                        en.update_dirty();
                        engine.ui_log(format!("已删除 {} 行", end - start));
                    }
                }
            }
        });
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
        self.write_table_cell(group, name, row, col, val);
    }

    /// 表格单元格写入的唯一实现：`set_*` / `commit_*` 都走这里，
    /// 这样撤销栈只需要在一个地方记录 `CellSnapshot`。
    fn write_table_cell(&mut self, group: &str, name: &str, row: usize, col: usize, val: &str) {
        self.with_table_cell_tx(group, name, row, col, "输入单元格", |engine| {
            if let Some(g) = engine.project_mut().groups.iter_mut().find(|g| g.name == group) {
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
        });
    }

    pub fn insert_constant_row(&mut self, group: &str, name: &str, at: usize) {
        self.with_node_tx(group, name, NodeKind::Constant, "插入行", |engine| {
            if let Some(g) = engine.project_mut().groups.iter_mut().find(|g| g.name == group) {
                if let Some(c) = g.constants.iter_mut().find(|c| c.name == name) {
                    let at = at.min(c.entries.len());
                    c.entries.insert(at, ConstEntry {
                        name: String::new(), tbl_type: "str".to_string(),
                        value: String::new(), export: Export::ClientServer, desc: String::new(),
                    });
                    c.update_dirty();
                }
            }
        });
    }

    pub fn delete_constant_row(&mut self, group: &str, name: &str, at: usize) {
        self.with_node_tx(group, name, NodeKind::Constant, "删除行", |engine| {
            if let Some(g) = engine.project_mut().groups.iter_mut().find(|g| g.name == group) {
                if let Some(c) = g.constants.iter_mut().find(|c| c.name == name) {
                    if at < c.entries.len() {
                        c.entries.remove(at);
                        c.update_dirty();
                    }
                }
            }
        });
    }

    pub fn set_constant_cell(&mut self, group: &str, name: &str, row: usize, col: usize, val: &str) {
        self.with_node_tx(group, name, NodeKind::Constant, "输入单元格", |engine| {
            if let Some(g) = engine.project_mut().groups.iter_mut().find(|g| g.name == group) {
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
        });
    }

    pub fn commit_table_cell(&mut self, group: &str, name: &str, row: usize, col: usize, val: String) {
        self.write_table_cell(group, name, row, col, &val);
    }

    pub fn commit_constant_cell(&mut self, group: &str, name: &str, row: usize, col: usize, val: String) {
        self.with_node_tx(group, name, NodeKind::Constant, "输入单元格", |engine| {
            if let Some(g) = engine.project_mut().groups.iter_mut().find(|g| g.name == group) {
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
        });
    }

    /// 提交表头编辑。
    /// `header_row` 是 0-based UI 行号（与 `TableHeaderRow::row()` 等价）：
    ///   0=desc, 1=export, 2=type, 3=field —— 顺序与 UI 表头从上到下一致，也与 .tbl 序列化顺序一致。
    pub fn commit_header_edit(&mut self, group: &str, name: &str, header_row: usize, col: usize, val: String) {
        let mut keyword_err = None;
        self.with_node_tx(group, name, NodeKind::Table, "修改表头", |engine| {
            if let Some(g) = engine.project_mut().groups.iter_mut().find(|g| g.name == group) {
                if let Some(t) = g.tables.iter_mut().find(|t| t.name == name) {
                    let mut renamed_from: Option<String> = None;
                    if let Some(field) = t.schema.fields.get_mut(col) {
                        match header_row {
                            0 => field.desc = val.clone(),
                            1 => field.export = Export::from_input(&val),
                            2 => field.tbl_type = val.clone(),
                            3 => {
                                let v = val.trim().to_string();
                                if !is_reserved_keyword(&v) {
                                    renamed_from = Some(std::mem::replace(&mut field.name, v));
                                } else {
                                    keyword_err = Some(val.clone());
                                }
                            }
                            _ => {}
                        }
                    }
                    // 字段改名 → 本表规则里的列名跟着改，否则规则会指向一个不存在的列。
                    if let Some(old) = renamed_from {
                        let now = t.schema.fields[col].name.clone();
                        if old != now {
                            for r in t.rules.iter_mut() {
                                if r.column == old { r.column = now.clone(); }
                            }
                        }
                    }
                    t.update_dirty();
                }
            }
            engine.project_mut().resync_schema();
            if let Some(kw) = keyword_err.take() {
                engine.ui_log(format!("字段名 '{}' 是保留关键字，不允许使用", kw));
            }
        });
    }

    pub fn insert_row(&mut self, group: &str, name: &str, at: usize) {
        self.with_node_tx(group, name, NodeKind::Table, "插入行", |engine| {
            if let Some(g) = engine.project_mut().groups.iter_mut().find(|g| g.name == group) {
                if let Some(t) = g.tables.iter_mut().find(|t| t.name == name) {
                    let cols = t.schema.fields.len();
                    let row = vec![String::new(); cols];
                    let at = at.min(t.records.len());
                    t.records.insert(at, row);
                    t.update_dirty();
                }
            }
        });
    }

    pub fn delete_row(&mut self, group: &str, name: &str, at: usize) {
        self.with_node_tx(group, name, NodeKind::Table, "删除行", |engine| {
            if let Some(g) = engine.project_mut().groups.iter_mut().find(|g| g.name == group) {
                if let Some(t) = g.tables.iter_mut().find(|t| t.name == name) {
                    if at < t.records.len() {
                        t.records.remove(at);
                        t.update_dirty();
                    }
                }
            }
        });
    }

    pub fn delete_rows(&mut self, group: &str, name: &str, start: usize, end: usize) {
        self.with_node_tx(group, name, NodeKind::Table, "删除行", |engine| {
            if let Some(g) = engine.project_mut().groups.iter_mut().find(|g| g.name == group) {
                if let Some(t) = g.tables.iter_mut().find(|t| t.name == name) {
                    let end = end.min(t.records.len());
                    if start < end {
                        t.records.drain(start..end);
                        t.update_dirty();
                        engine.ui_log(format!("已删除 {} 行", end - start));
                    }
                }
            }
        });
    }

    pub fn insert_column(&mut self, group: &str, name: &str, at: usize) {
        // 第 0 列是系统 id 列，不能在它左边插列（UI 已隐藏入口，这里是兜底）。
        if at == 0 {
            self.warn_log("id 列左侧不能插入列".to_string());
            return;
        }
        self.with_node_tx(group, name, NodeKind::Table, "插入列", |engine| {
            if let Some(g) = engine.project_mut().groups.iter_mut().find(|g| g.name == group) {
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
            engine.project_mut().resync_schema();
        });
    }

    pub fn delete_column(&mut self, group: &str, name: &str, at: usize) {
        // id 列不能删：删了表就没有主键了（校验也会拦，但别让它先落进内存）。
        if at == 0 {
            self.warn_log("id 列不能删除".to_string());
            return;
        }
        self.with_node_tx(group, name, NodeKind::Table, "删除列", |engine| {
            if let Some(g) = engine.project_mut().groups.iter_mut().find(|g| g.name == group) {
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
            engine.project_mut().resync_schema();
        });
    }

    pub fn paste_table_data(&mut self, group: &str, name: &str, start_row: usize, start_col: usize, grid: &[Vec<String>]) {
        if grid.is_empty() { return; }
        self.with_node_tx(group, name, NodeKind::Table, "粘贴数据", |engine| {
            if let Some(g) = engine.project_mut().groups.iter_mut().find(|g| g.name == group) {
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
            engine.ui_log(format!("粘贴 {}行 数据", grid.len()));
        });
    }

    pub fn paste_constant_data(&mut self, group: &str, name: &str, start_row: usize, start_col: usize, grid: &[Vec<String>]) {
        if grid.is_empty() { return; }
        self.with_node_tx(group, name, NodeKind::Constant, "粘贴数据", |engine| {
            if let Some(g) = engine.project_mut().groups.iter_mut().find(|g| g.name == group) {
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
            engine.ui_log(format!("粘贴 {}行 数据", grid.len()));
        });
    }

    pub fn clear_table_cells(&mut self, group: &str, name: &str, cells: &[(usize, usize)]) {
        self.with_node_tx(group, name, NodeKind::Table, "清空单元格", |engine| {
            if let Some(g) = engine.project_mut().groups.iter_mut().find(|g| g.name == group) {
                if let Some(t) = g.tables.iter_mut().find(|t| t.name == name) {
                    for &(row, col) in cells {
                        if let Some(record) = t.records.get_mut(row) {
                            if let Some(cell) = record.get_mut(col) { cell.clear(); }
                        }
                    }
                    t.update_dirty();
                }
            }
        });
    }

    pub fn clear_constant_cells(&mut self, group: &str, name: &str, cells: &[(usize, usize)]) {
        self.with_node_tx(group, name, NodeKind::Constant, "清空单元格", |engine| {
            if let Some(g) = engine.project_mut().groups.iter_mut().find(|g| g.name == group) {
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
        });
    }

    // --- PLACEHOLDER_SAVE ---

    /// 结构化校验结果：`(group, node, 错误)`。
    ///
    /// 与 [`validate`](Self::validate) 跑同一套校验，只是不预先拼成字符串——
    /// CLI 要拿 `header_row` / `field` / `message` 做定位与过滤，拼字符串的版本拿不到。
    pub fn validate_errors(&self) -> Vec<(String, String, ValidationError)> {
        let mut out = Vec::new();
        let policy = self.project().config.validate.clone();
        let sep = &self.project().schema.separators;
        let refs = RefIndex::build(&self.project().groups);
        let cidx = RuleIndex::build(&self.project().groups);
        let allow_ref = self.global_config.ui.as_ref()
            .map_or(true, |u| u.constant_ref_allowed);
        for group in &self.project().groups {
            for table in &group.tables {
                if table.deleted { continue; }
                for err in validate_table(table, sep, Some(&refs), Some(&cidx)) {
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
                for err in validate_enum(enum_def, Some(&cidx)) {
                    out.push((group.name.clone(), enum_def.name.clone(), err));
                }
            }
        }
        out.extend(validate_project_node_names(&self.project().groups));
        // 被策略忽略的级别不产生任何结果
        out.retain(|(_, _, e)| policy.action(e.severity) != crate::model::SeverityAction::Ignore);
        out
    }

    /// 当前 active project 的校验策略。
    pub fn validate_policy(&self) -> crate::model::ValidateConfig {
        self.active().map(|p| p.config.validate.clone()).unwrap_or_default()
    }

    /// 校验并渲染成 GUI 日志同款文本（`format_validation_log`）。
    pub fn validate(&self) -> Vec<String> {
        self.validate_errors().iter()
            .map(|(group, node, err)| format_validation_log(group, node, err))
            .collect()
    }

    /// 重算一个节点，并**连带重算依赖它结果的节点**。
    ///
    /// 依赖来源（@01.8.10 / @01.10.7）：
    /// - 字段 / 常量值的类型里写了 `@本节点`；
    /// - 规则的 `ref_exists(本节点.列)`。
    ///
    /// 用固定点迭代，自引用与环（A→B→A）都能收敛。
    pub fn revalidate(&mut self, group: &str, name: &str) {
        let Some(pid) = self.active_project_id().map(str::to_string) else { return; };
        let _ = group;
        let targets = self.dependency_closure(name);
        let sep = self.project().schema.separators.clone();
        let refs = RefIndex::build(&self.project().groups);
        let cidx = RuleIndex::build(&self.project().groups);
        let allow_ref = self.global_config.ui.as_ref()
            .map_or(true, |u| u.constant_ref_allowed);
        let policy = self.project().config.validate.clone();
        for (g, n) in targets {
            self.revalidate_exact(&pid, &g, &n, &sep, &refs, &cidx, allow_ref, &policy);
        }
    }

    /// 改了 `changed` 之后，校验结果会变的节点（含自身）。
    fn dependency_closure(&self, changed: &str) -> Vec<(String, String)> {
        use std::collections::HashSet;
        let mut names: HashSet<String> = HashSet::new();
        names.insert(changed.to_string());
        loop {
            let mut grew = false;
            for g in &self.project().groups {
                for t in &g.tables {
                    if t.deleted || names.contains(&t.name) { continue; }
                    if table_depends_on(t, &names) {
                        names.insert(t.name.clone());
                        grew = true;
                    }
                }
                for c in &g.constants {
                    if c.deleted || names.contains(&c.name) { continue; }
                    if constant_depends_on(c, &names) {
                        names.insert(c.name.clone());
                        grew = true;
                    }
                }
                for e in &g.enums {
                    if e.deleted || names.contains(&e.name) { continue; }
                    if enum_depends_on(e, &names) {
                        names.insert(e.name.clone());
                        grew = true;
                    }
                }
            }
            if !grew { break; }
        }
        let mut out = Vec::new();
        for g in &self.project().groups {
            for t in &g.tables {
                if !t.deleted && names.contains(&t.name) { out.push((g.name.clone(), t.name.clone())); }
            }
            for c in &g.constants {
                if !c.deleted && names.contains(&c.name) { out.push((g.name.clone(), c.name.clone())); }
            }
            for e in &g.enums {
                if !e.deleted && names.contains(&e.name) { out.push((g.name.clone(), e.name.clone())); }
            }
        }
        out
    }

    /// 精确重算一个节点的错误 / 警告索引（依赖集合与全量重算都走这里）。
    fn revalidate_exact(
        &mut self,
        pid: &str,
        group: &str,
        name: &str,
        sep: &crate::types::SeparatorsSection,
        refs: &RefIndex,
        cidx: &RuleIndex,
        allow_ref: bool,
        policy: &ValidateConfig,
    ) {
        self.validation_errors.retain(|(p, g, n, _, _)| p != pid || g != group || n != name);
        self.validation_warnings.retain(|(p, g, n, _)| p != pid || g != group || n != name);
        let mut issues: Vec<ValidationError> = Vec::new();
        {
            let Some(g) = self.project().groups.iter().find(|g| g.name == group) else { return };
            if let Some(table) = g.tables.iter().find(|t| t.name == name) {
                if !table.deleted {
                    issues.extend(validate_table(table, sep, Some(refs), Some(cidx)));
                }
            }
            if let Some(constant) = g.constants.iter().find(|c| c.name == name) {
                if !constant.deleted {
                    issues.extend(validate_constant(constant, sep, allow_ref, Some(refs)));
                }
            }
            if let Some(enum_def) = g.enums.iter().find(|e| e.name == name) {
                if !enum_def.deleted {
                    issues.extend(validate_enum(enum_def, Some(cidx)));
                }
            }
        }
        // 节点名是项目级平铺命名空间：每次重算本节点时也刷新它的重名错误。
        issues.extend(
            validate_project_node_names(&self.project().groups)
                .into_iter()
                .filter(|(g, n, _)| g == group && n == name)
                .map(|(_, _, e)| e),
        );
        for err in issues {
            match policy.action(err.severity) {
                crate::model::SeverityAction::Block => {
                    self.validation_errors.insert(
                        (pid.to_string(), group.to_string(), name.to_string(), err.row, err.col)
                    );
                }
                crate::model::SeverityAction::Report => {
                    self.validation_warnings.push(
                        (pid.to_string(), group.to_string(), name.to_string(), err)
                    );
                }
                crate::model::SeverityAction::Ignore => {}
            }
        }
    }

    /// 重算 active project 的所有节点错误；不影响其它 Project 的错误索引。
    pub fn revalidate_all(&mut self) {
        let Some(pid) = self.active_project_id().map(str::to_string) else { return; };
        self.validation_errors.retain(|(p, _, _, _, _)| p != &pid);
        self.validation_warnings.retain(|(p, _, _, _)| p != &pid);
        let groups: Vec<_> = self.project().groups.iter()
            .map(|g| (
                g.name.clone(),
                g.tables.iter().filter(|t| !t.deleted).map(|t| t.name.clone()).collect::<Vec<_>>(),
                g.constants.iter().filter(|c| !c.deleted).map(|c| c.name.clone()).collect::<Vec<_>>(),
                g.enums.iter().filter(|e| !e.deleted).map(|e| e.name.clone()).collect::<Vec<_>>(),
            ))
            .collect();
        let sep = self.project().schema.separators.clone();
        let refs = RefIndex::build(&self.project().groups);
        let cidx = RuleIndex::build(&self.project().groups);
        let allow_ref = self.global_config.ui.as_ref()
            .map_or(true, |u| u.constant_ref_allowed);
        let policy = self.project().config.validate.clone();
        for (gname, tables, constants, enums) in &groups {
            for tname in tables {
                self.revalidate_exact(&pid, gname, tname, &sep, &refs, &cidx, allow_ref, &policy);
            }
            for cname in constants {
                self.revalidate_exact(&pid, gname, cname, &sep, &refs, &cidx, allow_ref, &policy);
            }
            for ename in enums {
                self.revalidate_exact(&pid, gname, ename, &sep, &refs, &cidx, allow_ref, &policy);
            }
        }
    }

    /// 全 Project 重算：扫描每个 Project 的所有节点。启动加载、切换 active、批量验证用。
    /// 临时切换 active_idx 以复用现有 active-scope 逻辑，最后恢复。
    pub fn revalidate_all_projects(&mut self) {
        self.validation_errors.clear();
        self.validation_warnings.clear();
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
        self.log_pending_warnings(&pid);
        self.last_save_failure = None;
        let (count, deleted, warnings, failed) = save_project_files(self.project_mut());
        for w in &warnings { self.warn_log(w.clone()); }
        if failed {
            self.last_save_failure = warnings.iter().find(|w| w.contains("保存失败")).cloned();
            self.logs.push("[保存失败] 有组没能写进磁盘（见上方原因）".to_string());
            return false;
        }
        if count > 0 || deleted > 0 {
            self.ui_log(format!("已保存 {} 个文件, 删除 {} 个", count, deleted));
        } else {
            self.ui_log("无修改需要保存".to_string());
        }
        let had_history = self.history_len(&pid) != (0, 0);
        self.clear_history(&pid);
        if had_history {
            self.ui_log(format!("[{}] 已清空撤销/重做历史", pid));
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
        let pids: Vec<String> = self.projects.iter().map(|p| p.schema.meta.id.clone()).collect();
        for pid in &pids { self.log_pending_warnings(pid); }
        let mut total_count = 0;
        let mut total_deleted = 0;
        let mut warnings = Vec::new();
        let mut failed = false;
        let mut saved_pids = Vec::new();
        self.last_save_failure = None;
        for project in &mut self.projects {
            let pid = project.schema.meta.id.clone();
            let (c, d, w, f) = save_project_files(project);
            total_count += c;
            total_deleted += d;
            warnings.extend(w);
            failed |= f;
            if !f {
                saved_pids.push(pid);
            }
        }
        for w in &warnings { self.warn_log(w.clone()); }
        for pid in &saved_pids {
            let had_history = self.history_len(pid) != (0, 0);
            self.clear_history(pid);
            if had_history {
                self.ui_log(format!("[{}] 已清空撤销/重做历史", pid));
            }
        }
        if failed {
            self.last_save_failure = warnings.iter().find(|w| w.contains("保存失败")).cloned();
            self.logs.push("[保存失败] 有组没能写进磁盘（见上方原因）".to_string());
        }
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
                self.clear_all_histories();
                // reload 后采纳新解析的全局配置与默认分隔符，避免整写/后续读用 stale 副本
                self.global_config = new_engine.global_config;
                self.default_separators = new_engine.default_separators;
                self.validation_errors.clear();
                self.validation_warnings.clear();
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
        let Some(pid) = self.active_project_id().map(str::to_string) else { return; };
        self.with_project_tx(&pid, "删除 Group", |engine| {
            if let Some(g) = engine.project().groups.iter().find(|g| g.name == group_name) {
                if g.is_new {
                    engine.project_mut().groups.retain(|g| g.name != group_name);
                    engine.ui_log(format!("已移除新建 Group: {}", group_name));
                } else {
                    if let Some(g) = engine.project_mut().groups.iter_mut().find(|g| g.name == group_name) {
                        for t in &mut g.tables { t.deleted = true; }
                        for c in &mut g.constants { c.deleted = true; }
                        for e in &mut g.enums { e.deleted = true; }
                    }
                    engine.ui_log(format!("已标记删除 Group: {}", group_name));
                }
            }
            sync_schema_from_groups(engine.project_mut());
            // 删除/标删后，该 group 下所有节点的 validation_errors 不应再被 UI 聚合
            engine.validation_errors.retain(|(p, g, _, _, _)| !(p == &pid && g == group_name));
            engine.validation_warnings.retain(|(p, g, _, _)| !(p == &pid && g == group_name));
        });
    }

    pub fn delete_node(&mut self, group_name: &str, node_name: &str) {
        let Some(pid) = self.active_project_id().map(str::to_string) else { return; };
        self.with_project_tx(&pid, "删除节点", |engine| {
            if let Some(g) = engine.project_mut().groups.iter_mut().find(|g| g.name == group_name) {
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
            sync_schema_from_groups(engine.project_mut());
            engine.validation_errors.retain(|(p, g, n, _, _)| !(p == &pid && g == group_name && n == node_name));
            engine.validation_warnings.retain(|(p, g, n, _)| !(p == &pid && g == group_name && n == node_name));
            engine.ui_log(format!("已标记删除: {}/{}", group_name, node_name));
        });
    }

    /// 把某个节点还原成盘上的样子（「还原到已保存」，@02.26）。
    ///
    /// 只管**编辑**（内容 / 结构 / 身份），**不管存在性**：新建未保存的节点压根没有
    /// 盘上的样子可还原，软删的节点还原内容后仍然是删着的。所以「没有可还原的东西」
    /// 不是错误，只是没动作——返回 false，由调用方决定要不要说话。
    ///
    /// 这是「把 `original_*` 写回状态」这条普通路径的入口。连续撤销已落地，
    /// 本方法被 `with_project_tx` 包成一条普通历史记录：「还原」本身也能被撤销/重做，
    /// 不需要另开旁路（@02.26）。
    pub fn revert_node(&mut self, group_name: &str, node_name: &str) -> bool {
        let Some(pid) = self.active_project_id().map(str::to_string) else {
            return false;
        };
        let mut did = false;
        self.with_project_tx(&pid, "还原到已保存", |engine| {
            if let Some(g) = engine.project_mut().groups.iter_mut().find(|g| g.name == group_name) {
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
            if did {
                // 表头 / 表名 / # @display 都可能变，schema 投影要重建
                sync_schema_from_groups(engine.project_mut());
                // 内容换了，这个节点的校验结果得按新内容重算；撤销/重做后也会统一再算。
                engine.revalidate_all();
                engine.ui_log(format!("已还原到上次保存: {}/{}", group_name, node_name));
            }
        });
        did
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
    /// `fmt_ver` = Some(v) 时把粘贴出来的表改成该文件格式（v2/v3 只是序列化差异，
    /// 内存模型相同，不需要搬数据）；None = 保持源文件的格式。
    pub fn paste_node_to(
        &mut self,
        target_project: &str,
        target_group: &str,
        fmt_ver: Option<crate::tbl::TblVersion>,
    ) -> Option<String> {
        self.with_project_tx(target_project, "粘贴节点", |engine| {
            let Some(NodeClipboard::Node { body, .. }) = engine.node_clipboard.clone() else {
                engine.warn_log("剪贴板为空或类型不匹配（需要节点剪贴板）".to_string());
                return None;
            };
            let base = match &body {
                NodeBody::Table(t) => t.name.clone(),
                NodeBody::Constant(c) => c.name.clone(),
                NodeBody::Enum(e) => e.name.clone(),
            };
            // 节点名是项目级平铺命名空间：不能只查目标 Group。
            let mut existing = engine
                .find_project(target_project)
                .map(all_project_node_names)
                .unwrap_or_default();
            let final_name = dedupe_node_name(&base, &mut existing);
            let mut body = body;
            if final_name != base {
                let mut mapping = HashMap::new();
                mapping.insert(base, final_name.clone());
                rewrite_node_body_refs(&mut body, &mapping);
            }
            let project = engine.find_project_mut(target_project)?;
            let g = project.groups.iter_mut().find(|g| g.name == target_group)?;
            let new_path = if g.is_xlsx() { g.book_path() } else { g.dir.join(format!("{}.tbl", final_name)) };
            let kind_label = match body {
                NodeBody::Table(mut t) => {
                    t.name = final_name.clone();
                    t.path = new_path;
                    if let Some(v) = fmt_ver { t.fmt_ver = v; }
                    t.dirty = true;
                    t.original_records.clear(); t.original_fields.clear(); t.saved = false;
                    t.original_rules.clear();
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
                    e.original_rules.clear();
                    e.original_name = final_name.clone();
                    g.enums.push(e);
                    "Enum"
                }
            };
            engine.ui_log(format!(
                "已粘贴 {} 到 {}/{}/{}",
                kind_label, target_project, target_group, final_name
            ));
            if let Some(p) = engine.find_project_mut(target_project) {
                sync_schema_from_groups(p);
            }
            let prev = engine.active_idx;
            if let Some(idx) = engine.projects.iter().position(|p| p.schema.meta.id == target_project) {
                engine.active_idx = Some(idx);
                engine.revalidate(target_group, &final_name);
                engine.active_idx = prev;
            }
            Some(final_name)
        })
    }

    /// 只读预览：计算整组粘贴后的组名 / 子项名，以及是否存在项目级命名冲突。
    /// 不修改任何数据；调用方据此决定直接粘贴还是打开命名确认对话框。
    pub fn preview_paste_group(&self, target_project: &str) -> Option<PasteGroupPreview> {
        let NodeClipboard::Group { snapshot, .. } = self.node_clipboard.clone()? else {
            return None;
        };
        let project = self.find_project(target_project)?;
        let existing_groups: Vec<String> = project.groups.iter().map(|g| g.name.clone()).collect();
        let group_conflict = existing_groups.iter().any(|n| n.eq_ignore_ascii_case(&snapshot.name));
        let group_name = if group_conflict {
            dedupe_name(&snapshot.name, existing_groups.iter().map(String::as_str))
        } else {
            snapshot.name.clone()
        };
        let mut used = all_project_node_names(project);
        let mut nodes = Vec::new();
        let mut any_conflict = group_conflict;
        for t in &snapshot.tables {
            let old = t.name.clone();
            let new = dedupe_node_name(&old, &mut used);
            any_conflict |= new != old;
            nodes.push(PasteNodePreview { kind: NodeKind::Table, old_name: old, new_name: new.clone(), conflict: new != t.name });
        }
        for c in &snapshot.constants {
            let old = c.name.clone();
            let new = dedupe_node_name(&old, &mut used);
            any_conflict |= new != old;
            nodes.push(PasteNodePreview { kind: NodeKind::Constant, old_name: old, new_name: new.clone(), conflict: new != c.name });
        }
        for e in &snapshot.enums {
            let old = e.name.clone();
            let new = dedupe_node_name(&old, &mut used);
            any_conflict |= new != old;
            nodes.push(PasteNodePreview { kind: NodeKind::Enum, old_name: old, new_name: new.clone(), conflict: new != e.name });
        }
        Some(PasteGroupPreview { group_name, group_conflict, nodes, has_conflict: any_conflict })
    }

    /// 粘贴整组到目标 project，作为兄弟 Group。
    /// - group 名字冲突走 `_copy` / `_copy2`
    /// - group.dir 重写到 `<project.data_dir()>/<final_name>`，is_new=true（save 时建目录）
    /// - 每个 child path 重写到新 dir 下，dirty=true，original=""
    /// - 末尾整 project revalidate
    /// 返回新 group 名。
    pub fn paste_group_to(&mut self, target_project: &str) -> Option<String> {
        self.paste_group_as(target_project, None)
    }

    /// 粘贴整组，并**指定目标后端**（`None` = 跟源组一致）。
    ///
    /// 这是「一组一种后端」下唯一能把整组在 `.tbl` / `xlsx` 之间搬家的入口：
    /// 节点在内存里两种后端同型，所以"转换"只是换一个保存写入器（@02.26.8）。
    pub fn paste_group_as(
        &mut self,
        target_project: &str,
        storage: Option<crate::model::GroupStorage>,
    ) -> Option<String> {
        self.paste_group_impl(target_project, storage, None).ok()
    }

    /// 粘贴整组，使用调用方在「粘贴命名」对话框里确认过的组名 / 子项名。
    /// 确认时仍会按项目级唯一性再校验一次，失败不落任何数据。
    pub fn paste_group_as_with_names(
        &mut self,
        target_project: &str,
        storage: Option<crate::model::GroupStorage>,
        group_name: &str,
        node_names: &std::collections::BTreeMap<String, String>,
    ) -> Result<String, String> {
        self.paste_group_impl(target_project, storage, Some((group_name, node_names)))
    }

    fn paste_group_impl(
        &mut self,
        target_project: &str,
        storage: Option<crate::model::GroupStorage>,
        plan: Option<(&str, &std::collections::BTreeMap<String, String>)>,
    ) -> Result<String, String> {
        self.with_project_tx(target_project, "粘贴 Group", |engine| {
            let Some(NodeClipboard::Group { snapshot, .. }) = engine.node_clipboard.clone() else {
                let msg = "剪贴板为空或类型不匹配（需要 Group 剪贴板）".to_string();
                engine.warn_log(msg.clone());
                return Err(msg);
            };
            let project = engine
                .find_project_mut(target_project)
                .ok_or_else(|| format!("Project 不存在: {}", target_project))?;
            let existing_groups: Vec<String> = project.groups.iter().map(|g| g.name.clone()).collect();
            let data_dir = project.data_dir();

            let final_group = match plan {
                Some((name, _)) => {
                    if !crate::validate::is_valid_group_name(name) {
                        return Err("组名只能包含中英文数字下划线".to_string());
                    }
                    if existing_groups.iter().any(|n| n.eq_ignore_ascii_case(name)) {
                        return Err(format!("组名重复（忽略大小写）: {}", name));
                    }
                    name.to_string()
                }
                None => {
                    if existing_groups.iter().any(|n| n.eq_ignore_ascii_case(&snapshot.name)) {
                        dedupe_name(&snapshot.name, existing_groups.iter().map(String::as_str))
                    } else {
                        snapshot.name.clone()
                    }
                }
            };

            let mut new_group = snapshot;
            if let Some(st) = storage {
                new_group.storage = st;
            }
            let new_dir = if new_group.is_xlsx() {
                data_dir
            } else {
                data_dir.join(&final_group)
            };
            new_group.name = final_group.clone();
            new_group.dir = new_dir.clone();
            new_group.is_new = true;
            new_group.book_stamp = None;

            // 子项是项目级平铺命名空间：按项目现有节点去重/校验，并记录旧名 → 新名。
            let mut used_nodes = all_project_node_names(project);
            let mut mapping: HashMap<String, String> = HashMap::new();
            let mut apply_name = |old: &str,
                                  kind: NodeKind,
                                  used: &mut Vec<String>|
             -> Result<String, String> {
                let new = match plan {
                    Some((_, names)) => names.get(old).cloned().unwrap_or_else(|| old.to_string()),
                    None => dedupe_node_name(old, used),
                };
                if plan.is_some() {
                    if !crate::validate::is_valid_node_name(&new) {
                        return Err(format!(
                            "{} {} 的名字不符合 Java 类名规则（大写开头，英文数字下划线）",
                            match kind {
                                NodeKind::Table => "Table",
                                NodeKind::Constant => "Constant",
                                NodeKind::Enum => "Enum",
                            },
                            new
                        ));
                    }
                    if node_name_taken(used, &new) {
                        return Err(format!("节点名已存在（项目内唯一，忽略大小写）: {}", new));
                    }
                    used.push(new.clone());
                }
                if new != old {
                    mapping.insert(old.to_string(), new.clone());
                }
                Ok(new)
            };

            for t in &mut new_group.tables {
                let old = t.name.clone();
                t.name = apply_name(&old, NodeKind::Table, &mut used_nodes)?;
            }
            for c in &mut new_group.constants {
                let old = c.name.clone();
                c.name = apply_name(&old, NodeKind::Constant, &mut used_nodes)?;
            }
            for e in &mut new_group.enums {
                let old = e.name.clone();
                e.name = apply_name(&old, NodeKind::Enum, &mut used_nodes)?;
            }
            rewrite_group_refs(&mut new_group, &mapping);

            let is_xlsx = new_group.is_xlsx();
            let book = new_dir.join(format!("{}.xlsx", new_group.name));
            for t in &mut new_group.tables {
                t.path = if is_xlsx { book.clone() } else { new_dir.join(format!("{}.tbl", t.name)) };
                t.dirty = true;
                t.original_records.clear(); t.original_fields.clear(); t.saved = false;
                t.original_rules.clear();
                t.original_name = t.name.clone();
            }
            for c in &mut new_group.constants {
                c.path = if is_xlsx { book.clone() } else { new_dir.join(format!("{}.tbl", c.name)) };
                c.dirty = true;
                c.original_entries.clear(); c.saved = false;
                c.original_name = c.name.clone();
            }
            for e in &mut new_group.enums {
                e.path = if is_xlsx { book.clone() } else { new_dir.join(format!("{}.tbl", e.name)) };
                e.dirty = true;
                e.original_entries.clear(); e.saved = false;
                e.original_rules.clear();
                e.original_name = e.name.clone();
            }
            let summary = format!(
                "T{}+C{}+E{}",
                new_group.tables.len(),
                new_group.constants.len(),
                new_group.enums.len(),
            );
            new_group.original_name = final_group.clone();
            project.groups.push(new_group);
            sync_schema_from_groups(project);
            engine.ui_log(format!("已粘贴 Group 到 {}/{}（{}）", target_project, final_group, summary));
            let prev = engine.active_idx;
            if let Some(idx) = engine.projects.iter().position(|p| p.schema.meta.id == target_project) {
                engine.active_idx = Some(idx);
                engine.revalidate_all();
                engine.active_idx = prev;
            }
            Ok(final_group)
        })
    }

    /// 清空剪贴板（关闭/退出由 drop 自动处理；本接口供未来"清空剪贴板"按钮用）。
    pub fn clipboard_clear(&mut self) {
        self.node_clipboard = None;
    }

    // --- PLACEHOLDER_ACTIONS ---

    /// 树面板的结构动作入口。除「删除 Project」外都记一条 Project 快照事务；
    /// DeleteProject 是项目管理动作，不进撤销栈。
    pub fn execute_action(&mut self, action: &ProjectAction) {
        if let ProjectAction::DeleteProject { project_id } = action {
            self.execute_action_inner(action);
            self.histories.remove(project_id);
            return;
        }
        let (project_id, label) = match action {
            ProjectAction::NewGroup { project_id, .. } => (project_id, "新建 Group"),
            ProjectAction::NewTable { project_id, .. } => (project_id, "新建 Table"),
            ProjectAction::NewConstant { project_id, .. } => (project_id, "新建 Constant"),
            ProjectAction::NewEnum { project_id, .. } => (project_id, "新建 Enum"),
            ProjectAction::RenameGroup { project_id, .. } => (project_id, "重命名 Group"),
            ProjectAction::RenameNode { project_id, .. } => (project_id, "重命名节点"),
            ProjectAction::DeleteProject { project_id } => (project_id, "删除 Project"),
        };
        let pid = project_id.clone();
        self.with_project_tx(&pid, label, |engine| {
            engine.execute_action_inner(action);
        });
    }

    fn execute_action_inner(&mut self, action: &ProjectAction) {
        match action {
            ProjectAction::NewGroup { project_id, name, storage } => {
                let Some(project) = self.find_project_mut(project_id) else {
                    self.ui_log(format!("Project 不存在: {}", project_id));
                    return;
                };
                let dir = match storage {
                    GroupStorage::Xlsx => project.data_dir(),
                    GroupStorage::Tbl => project.data_dir().join(name),
                };
                project.groups.push(Group {
                    name: name.clone(),
                    original_name: name.clone(),
                    dir,
                    tables: Vec::new(),
                    constants: Vec::new(),
                    enums: Vec::new(),
                    is_new: true,
                    storage: *storage,
                    book_stamp: None,
                });
                sync_schema_from_groups(project);
                self.ui_log(format!("[{}] 新建 Group: {}（{}）", project_id, name, storage.as_str()));
            }
            ProjectAction::NewTable { project_id, group, name, fmt_ver } => {
                let mut ok = false;
                if let Some(project) = self.find_project_mut(project_id) {
                    if let Some(g) = project.groups.iter_mut().find(|g| g.name == *group) {
                        // Xlsx 组没有自己的文件，path 一律指向组工作簿（只用于日志/调试）
                        let path = if g.is_xlsx() { g.book_path() } else { g.dir.join(format!("{}.tbl", name)) };
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
                            fmt_ver: *fmt_ver,
                            display_field: None,
                            original_display_field: None,
                            rules: Vec::new(),
                            original_rules: Vec::new(),
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
                        // Xlsx 组没有自己的文件，path 一律指向组工作簿（只用于日志/调试）
                        let path = if g.is_xlsx() { g.book_path() } else { g.dir.join(format!("{}.tbl", name)) };
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
                        // Xlsx 组没有自己的文件，path 一律指向组工作簿（只用于日志/调试）
                        let path = if g.is_xlsx() { g.book_path() } else { g.dir.join(format!("{}.tbl", name)) };
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
                            rules: Vec::new(),
                            original_rules: Vec::new(),
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
                    if let Some(g) = project.groups.iter_mut().find(|g| g.name == *old_name) {
                        // **只改内存**：盘上还是旧名字，save 时才搬（见 apply_pending_renames）。
                        let is_xlsx = g.is_xlsx();
                        let new_dir = if is_xlsx {
                            config_dir.clone()
                        } else {
                            config_dir.join(new_name)
                        };
                        g.name = new_name.clone();
                        g.dir = new_dir.clone();
                        // Tbl 组每个 .tbl 在 group dir 下；Xlsx 组统一指向根级工作簿。
                        let book = g.book_path();
                        for t in &mut g.tables {
                            t.path = if is_xlsx { book.clone() } else { new_dir.join(format!("{}.tbl", t.name)) };
                        }
                        for c in &mut g.constants {
                            c.path = if is_xlsx { book.clone() } else { new_dir.join(format!("{}.tbl", c.name)) };
                        }
                        for e in &mut g.enums {
                            e.path = if is_xlsx { book.clone() } else { new_dir.join(format!("{}.tbl", e.name)) };
                        }
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
                for w in self.validation_warnings.iter_mut() {
                    if w.0 == *project_id && w.1 == *old_name { w.1 = new_name.clone(); }
                }
                self.ui_log(format!("[{}] 重命名 Group: {} → {}", project_id, old_name, new_name));
            }
            ProjectAction::RenameNode { project_id, group, old_name, new_name } => {
                // 同 RenameGroup：预填后直接确认属常见操作，不是一次真正的改名。
                if old_name == new_name { return; }
                let mut refs_rewritten = 0usize;
                if let Some(project) = self.find_project_mut(project_id) {
                    if let Some(g) = project.groups.iter_mut().find(|g| g.name == *group) {
                        let new_path = if g.is_xlsx() {
                            g.book_path()
                        } else {
                            g.dir.join(format!("{}.tbl", new_name))
                        };
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
                for w in self.validation_warnings.iter_mut() {
                    if w.0 == *project_id && w.1 == *group && w.2 == *old_name {
                        w.2 = new_name.clone();
                    }
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
            ProjectAction::NewTable { project_id, group, name, .. }
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
            self.histories.remove(project_id);
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
        // 清掉 validation_errors / validation_warnings
        self.validation_errors.retain(|(p, _, _, _, _)| p != project_id);
        self.validation_warnings.retain(|(p, _, _, _)| p != project_id);
        self.validation_warnings.retain(|(p, _, _, _)| p != project_id);
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

    /// 工作簿被外部改过、且与内存里的快照分叉的 xlsx 组。
    ///
    /// GUI 用它在窗口空闲时做刷新：干净的直接重载，有未保存改动的只提醒别覆盖。
    pub fn xlsx_external_changes(&self) -> Vec<XlsxExternalChange> {
        let mut out = Vec::new();
        for project in &self.projects {
            for group in &project.groups {
                if !group.is_xlsx() { continue; }
                let Some(prev) = group.book_stamp else { continue };
                if crate::project::file_stamp(&group.book_path()) != Some(prev) {
                    out.push(XlsxExternalChange {
                        project_id: project.schema.meta.id.clone(),
                        group: group.name.clone(),
                        dirty: group.is_dirty(),
                    });
                }
            }
        }
        out
    }

    /// 重新从磁盘读一个 xlsx 组。只用于**干净**的组；有未保存改动时调用方应先问用户。
    pub fn reload_xlsx_group(&mut self, project_id: &str, group_name: &str) -> anyhow::Result<()> {
        let active = self.active_project_id() == Some(project_id);
        let Some(project) = self.find_project_mut(project_id) else {
            anyhow::bail!("Project 不存在: {}", project_id);
        };
        let Some(idx) = project.groups.iter().position(|g| g.name == group_name) else {
            anyhow::bail!("Group 不存在: {}", group_name);
        };
        if !project.groups[idx].is_xlsx() {
            anyhow::bail!("{} 不是 xlsx 组", group_name);
        }
        if project.groups[idx].is_dirty() {
            anyhow::bail!("{} 有未保存改动，拒绝重载", group_name);
        }
        let fresh = crate::project::reload_xlsx_group(&project.groups[idx], &project.schema)?;
        project.groups[idx] = fresh;
        project.resync_schema();
        if active {
            self.revalidate_all();
        }
        // 外部工作簿重载 = 新基线，不是编辑器动作；旧历史不再成立。
        self.clear_history(project_id);
        Ok(())
    }

    /// 保存指定 Project 的所有改动；其它 Project 不动。
    /// 不通过验证则放弃保存。
    pub fn save_project(&mut self, project_id: &str) -> bool {
        // 先全量重算（active scope 不够，因为我们改的 project 不一定是 active）
        self.revalidate_all_projects();
        let pid_errs = self.validation_errors.iter()
            .filter(|(p, _, _, _, _)| p == project_id).count();
        if pid_errs > 0 {
            let errors = self.validate_project(project_id);
            for e in &errors { self.logs.push(e.clone()); }
            self.logs.push(format!("[{}] [保存失败] 共 {} 个验证错误", project_id, pid_errs));
            return false;
        }
        let pid_for_log = project_id.to_string();
        self.log_pending_warnings(&pid_for_log);
        self.last_save_failure = None;
        let saved = {
            let Some(project) = self.find_project_mut(project_id) else {
                self.ui_log(format!("Project 不存在: {}", project_id));
                return false;
            };
            save_project_files(project)
        };
        let (count, deleted, warnings, failed) = saved;
        for w in &warnings { self.warn_log(w.clone()); }
        if failed {
            self.last_save_failure = warnings.iter().find(|w| w.contains("保存失败")).cloned();
            self.logs.push(format!("[{}] [保存失败] 有组没能写进磁盘（见上方原因）", project_id));
            return false;
        }
        if count > 0 || deleted > 0 {
            self.ui_log(format!("[{}] 已保存 {} 个文件, 删除 {} 个", project_id, count, deleted));
        } else {
            self.ui_log(format!("[{}] 无修改需要保存", project_id));
        }
        let had_history = self.history_len(project_id) != (0, 0);
        self.clear_history(project_id);
        if had_history {
            self.ui_log(format!("[{}] 已清空撤销/重做历史", project_id));
        }
        true
    }

    /// 收集指定 Project 全部验证错误的格式化日志（仅打日志，不入索引）。
    /// 结构化版本：调用方要按级别分流（只有 Error 算失败）时用它。
    pub fn validate_project_issues(&self, project_id: &str)
        -> Vec<(String, String, ValidationError)>
    {
        let Some(project) = self.find_project(project_id) else { return Vec::new(); };
        let policy = project.config.validate.clone();
        let mut issues = Vec::new();
        let sep = &project.schema.separators;
        let refs = RefIndex::build(&project.groups);
        let cidx = RuleIndex::build(&project.groups);
        let allow_ref = self.global_config.ui.as_ref()
            .map_or(true, |u| u.constant_ref_allowed);
        for group in &project.groups {
            for table in &group.tables {
                if table.deleted { continue; }
                for err in validate_table(table, sep, Some(&refs), Some(&cidx)) {
                    issues.push((group.name.clone(), table.name.clone(), err));
                }
            }
            for constant in &group.constants {
                if constant.deleted { continue; }
                for err in validate_constant(constant, sep, allow_ref, Some(&refs)) {
                    issues.push((group.name.clone(), constant.name.clone(), err));
                }
            }
            for enum_def in &group.enums {
                if enum_def.deleted { continue; }
                for err in validate_enum(enum_def, Some(&cidx)) {
                    issues.push((group.name.clone(), enum_def.name.clone(), err));
                }
            }
        }
        issues.extend(validate_project_node_names(&project.groups));
        issues.retain(|(_, _, e)| policy.action(e.severity) != crate::model::SeverityAction::Ignore);
        issues
    }

    /// 把某个 Project 当前缓存的警告打进日志。警告不阻断任何流程。
    pub fn log_pending_warnings(&mut self, project_id: &str) {
        let msgs: Vec<String> = self.validation_warnings.iter()
            .filter(|(p, _, _, _)| p == project_id)
            .map(|(_, g, n, e)| format_validation_log(g, n, e))
            .collect();
        for m in msgs { self.warn_log(m); }
    }

    /// 只含 Error 的日志行。保存失败时用它——警告不构成失败原因。
    pub fn validate_error_logs(&self) -> Vec<String> {
        let policy = self.validate_policy();
        self.validate_errors().iter()
            .filter(|(_, _, e)| policy.action(e.severity) == crate::model::SeverityAction::Block)
            .map(|(g, n, e)| format_validation_log(g, n, e))
            .collect()
    }

    /// 兼容旧调用：拼成日志文本（含警告）。
    pub fn validate_project(&self, project_id: &str) -> Vec<String> {
        self.validate_project_issues(project_id).iter()
            .map(|(g, n, e)| format_validation_log(g, n, e))
            .collect()
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
        let original_validate = config.validate.clone();
        Project {
            workdir,
            project_root,
            config,
            schema,
            original_meta: crate::tblschema::SchemaMetadata::default(),
            original_separators: Default::default(),
            original_export,
            original_validate,
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
            storage: crate::model::GroupStorage::Tbl, book_stamp: None,
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

                rules: Vec::new(),
                original_rules: Vec::new(),
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

                rules: Vec::new(),
                original_rules: Vec::new(),
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
            storage: crate::model::GroupStorage::Tbl, book_stamp: None,
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

                rules: Vec::new(),
                original_rules: Vec::new(),
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
    fn cell_edit_undo_redo_restores_value_and_row_shape() {
        let (dir, mut engine) = engine_with_saved_node("undo_cell", "g", "T");
        engine.set_table_cell("g", "T", 1, 0, "2");
        assert_eq!(engine.find_table("g", "T").unwrap().records.len(), 2, "输入空行应自动补行");
        assert!(engine.find_table("g", "T").unwrap().dirty);
        assert!(engine.can_undo());

        assert!(engine.undo());
        let t = engine.find_table("g", "T").unwrap();
        assert_eq!(t.records.len(), 1, "撤销必须收回输入时补出来的空行");
        assert_eq!(t.records[0], vec!["1".to_string()]);
        assert!(!t.dirty, "撤销回盘上状态后应不再脏");
        assert!(!engine.can_undo());
        assert!(engine.can_redo());

        assert!(engine.redo());
        let t = engine.find_table("g", "T").unwrap();
        assert_eq!(t.records.len(), 2);
        assert_eq!(t.records[1], vec!["2".to_string()]);
        assert!(t.dirty);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saving_active_project_clears_its_history() {
        let (dir, mut engine) = engine_with_saved_node("undo_save", "g", "T");
        let pid = engine.active_project_id().unwrap().to_string();
        engine.set_table_cell("g", "T", 0, 0, "9");
        assert!(engine.can_undo());
        assert!(engine.save_project(&pid), "合法数据应能保存");
        assert!(!engine.can_undo(), "保存成功后应清空撤销栈");
        assert!(!engine.can_redo(), "保存成功后应清空重做栈");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn histories_are_per_project_and_save_only_clears_the_target() {
        let (dir, mut engine) = engine_with_saved_node("undo_projects", "g", "T");
        let p1 = engine.active_project_id().unwrap().to_string();
        engine.set_table_cell("g", "T", 0, 0, "9");
        assert!(engine.can_undo(), "p1 应有一条历史");

        let p2 = engine.clone_project_in_memory(&p1, "p2", "P2").expect("clone");
        engine.set_active_by_id(&p2);
        assert!(!engine.can_undo(), "新克隆项目的历史应为空");
        engine.set_table_cell("g", "T", 0, 0, "8");
        assert!(engine.can_undo(), "p2 应有一条历史");

        // 保存 p1：只清 p1 的栈，p2 不动。
        assert!(engine.save_project(&p1));
        engine.set_active_by_id(&p1);
        assert!(!engine.can_undo(), "保存 p1 后其历史应清空");
        engine.set_active_by_id(&p2);
        assert!(engine.can_undo(), "p2 的历史不能被 p1 的保存清掉");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn failed_save_keeps_history() {
        let (dir, mut engine) = engine_with_saved_node("undo_save_fail", "g", "T");
        let pid = engine.active_project_id().unwrap().to_string();
        // id=0 会触发内置 positive_int 校验错误，保存必须失败。
        engine.set_table_cell("g", "T", 0, 0, "0");
        assert!(engine.can_undo());
        assert!(!engine.save_project(&pid), "非法 id 不应保存成功");
        assert!(engine.can_undo(), "保存失败必须保留撤销栈");
        assert!(engine.undo(), "错误内容仍可撤销");
        assert_eq!(engine.find_table("g", "T").unwrap().records[0][0], "1");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn header_and_row_edits_are_undoable() {
        let (dir, mut engine) = engine_with_saved_node("undo_node", "g", "T");
        engine.commit_header_edit("g", "T", 0, 0, "ID2".to_string());
        assert_eq!(engine.find_table("g", "T").unwrap().schema.fields[0].desc, "ID2");
        engine.insert_row("g", "T", 1);
        assert_eq!(engine.find_table("g", "T").unwrap().records.len(), 2);

        assert!(engine.undo(), "撤销插入行");
        assert_eq!(engine.find_table("g", "T").unwrap().records.len(), 1);
        assert!(engine.undo(), "撤销表头编辑");
        assert_eq!(engine.find_table("g", "T").unwrap().schema.fields[0].desc, "ID");
        assert!(engine.redo());
        assert_eq!(engine.find_table("g", "T").unwrap().schema.fields[0].desc, "ID2");
        assert!(engine.redo());
        assert_eq!(engine.find_table("g", "T").unwrap().records.len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn revert_to_saved_is_itself_undoable() {
        let (dir, mut engine) = engine_with_saved_node("undo_revert", "g", "T");
        engine.set_table_cell("g", "T", 0, 0, "9");
        assert!(engine.revert_node("g", "T"), "应还原到已保存");
        assert_eq!(engine.find_table("g", "T").unwrap().records[0][0], "1");
        assert!(!engine.find_table("g", "T").unwrap().dirty);

        assert!(engine.undo(), "还原到已保存也应是一步历史");
        assert_eq!(engine.find_table("g", "T").unwrap().records[0][0], "9");
        assert!(engine.redo());
        assert_eq!(engine.find_table("g", "T").unwrap().records[0][0], "1");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rename_node_is_one_undoable_transaction() {
        let (dir, mut engine) = engine_with_saved_node("undo_rename", "g", "T");
        let pid = engine.active_project_id().unwrap().to_string();
        engine.execute_action(&ProjectAction::RenameNode {
            project_id: pid,
            group: "g".into(),
            old_name: "T".into(),
            new_name: "T2".into(),
        });
        assert!(engine.find_table("g", "T2").is_some());
        assert!(engine.find_table("g", "T").is_none());
        assert!(engine.can_undo());

        assert!(engine.undo());
        assert!(engine.find_table("g", "T").is_some(), "撤销改名应恢复旧名");
        assert!(engine.find_table("g", "T2").is_none());

        assert!(engine.redo());
        assert!(engine.find_table("g", "T2").is_some(), "重做改名应恢复新名");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn paste_node_is_one_undoable_transaction() {
        let (dir, mut engine) = engine_with_saved_node("undo_paste", "g", "T");
        let pid = engine.active_project_id().unwrap().to_string();
        engine.clipboard_copy_node(&pid, "g", "T", NodeKind::Table);
        let pasted = engine.paste_node_to(&pid, "g", None).expect("粘贴成功");
        assert!(engine.find_table("g", &pasted).is_some());
        assert!(engine.can_undo());

        assert!(engine.undo());
        assert!(engine.find_table("g", &pasted).is_none(), "撤销粘贴应移除新节点");

        assert!(engine.redo());
        assert!(engine.find_table("g", &pasted).is_some(), "重做粘贴应恢复节点");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rules_survive_reload_and_warnings_do_not_block_save() {
        let (_dir, mut engine) = engine_with_saved_node("rules_roundtrip", "g", "T");
        // 对 id 挂 unique()：系统已保证唯一 → 只产生一条警告，不阻断保存
        assert!(engine.set_node_rules("g", "T", vec![
            Rule { column: "id".into(), kind: RuleKind::Unique, level: Severity::Error , source: RuleSource::User },
        ]));
        engine.revalidate("g", "T");
        assert!(engine.validation_errors.is_empty(), "警告不该进错误索引");
        assert_eq!(engine.validation_warnings.len(), 1);
        assert!(engine.save_all(), "警告不该阻断保存");

        engine.reload();
        let (_, rules) = engine.node_rules("g", "T").expect("节点还在");
        assert_eq!(rules.len(), 1, "规则必须从 project.tblschema 读回来");
        assert_eq!(rules[0].kind, RuleKind::Unique);
        assert!(!engine.find_table("g", "T").unwrap().dirty, "重新加载后规则不该让节点变脏");
    }

    /// 粘贴可以顺手换格式：v2/v3 只是序列化差异，数据不用搬。
    #[test]
    fn paste_node_can_convert_table_format() {
        let (_dir, mut engine) = engine_with_saved_node("paste_fmt", "g", "T");
        let pid = engine.active_project_id().unwrap().to_string();
        engine.clipboard_copy_node(&pid, "g", "T", NodeKind::Table);

        let converted = engine.paste_node_to(&pid, "g", Some(crate::tbl::TblVersion::V2))
            .expect("粘贴成功");
        let t = engine.find_table("g", &converted).expect("节点存在");
        assert_eq!(t.fmt_ver, crate::tbl::TblVersion::V2, "指定 v2 应转换格式");

        let kept = engine.paste_node_to(&pid, "g", None).expect("再次粘贴");
        let t2 = engine.find_table("g", &kept).expect("节点存在");
        assert_eq!(t2.fmt_ver, crate::tbl::TblVersion::V3, "不指定则保持源格式");
    }

    /// 改目标节点后，只重算目标本身也要连带刷新依赖它的节点（@01.8.10）。
    #[test]
    fn editing_a_target_refreshes_dependent_nodes() {
        let dir = unique_tmp("dep_refresh");
        let mut project = make_project(
            dir.clone(), dir.join("projects").join("p1"), "p1",
        );
        let key_field = FieldDef {
            name: "key".into(), desc: String::new(),
            tbl_type: "str".into(), export: Export::ClientServer,
        };
        let lang = Table {
            name: "Lang".into(), original_name: "Lang".into(),
            path: dir.join("Lang.tbl"),
            schema: TableSchema { fields: vec![id_field(), key_field.clone()] },
            records: vec![vec!["1".into(), "A".into()]],
            dirty: false, deleted: false,
            original_records: vec![vec!["1".into(), "A".into()]],
            original_fields: vec![id_field(), key_field],
            saved: true, fmt_ver: crate::tbl::DEFAULT_TBL_VERSION,
            display_field: None, original_display_field: None,
            rules: Vec::new(), original_rules: Vec::new(),
        };
        let code = EnumDef {
            name: "ErrorCode".into(), original_name: "ErrorCode".into(),
            path: dir.join("ErrorCode.tbl"),
            entries: vec![EnumEntry { id: "10001".into(), name: "A".into(), desc: String::new() }],
            dirty: false, deleted: false,
            original_entries: vec![EnumEntry { id: "10001".into(), name: "A".into(), desc: String::new() }],
            saved: true,
            display_field: None, original_display_field: None,
            rules: vec![Rule {
                column: "name".into(),
                kind: RuleKind::RefExists { target_node: "Lang".into(), target_column: "key".into() },
                level: Severity::Warning,
                source: RuleSource::User,
            }],
            original_rules: Vec::new(),
        };
        project.groups.push(Group {
            name: "g".into(), original_name: "g".into(),
            dir: dir.join("g"),
            tables: vec![lang], constants: Vec::new(), enums: vec![code],
            is_new: false, storage: crate::model::GroupStorage::Tbl, book_stamp: None,
        });

        let mut engine = ProjectEngine::new(project, GlobalConfig::default());
        engine.revalidate_all();
        assert!(engine.validation_warnings.is_empty(), "初始 key=A，绑定成立");

        // 只改 Lang 的数据，然后**只**重算 Lang
        if let Some(t) = engine.find_table_mut("g", "Lang") {
            t.records[0][1] = "B".into();
            t.update_dirty();
        }
        engine.revalidate("g", "Lang");
        assert_eq!(engine.validation_warnings.len(), 1,
            "依赖 Lang.key 的 ErrorCode 必须被连带重算");
    }

    /// 跨存储的引用与规则：tbl 组的表引用 xlsx 组的表，索引与联动都不看存储后端。
    #[test]
    fn cross_storage_refs_and_rules_are_validated() {
        let dir = unique_tmp("cross_storage");
        let mut project = make_project(
            dir.clone(), dir.join("projects").join("p1"), "p1",
        );

        // xlsx 组：目标表 B
        let b = Table {
            name: "B".into(), original_name: "B".into(),
            path: dir.join("xg.xlsx"),
            schema: TableSchema { fields: vec![id_field()] },
            records: vec![vec!["1".into()]],
            dirty: false, deleted: false,
            original_records: vec![vec!["1".into()]],
            original_fields: vec![id_field()],
            saved: true, fmt_ver: crate::tbl::DEFAULT_TBL_VERSION,
            display_field: None, original_display_field: None,
            rules: Vec::new(), original_rules: Vec::new(),
        };
        project.groups.push(Group {
            name: "xg".into(), original_name: "xg".into(),
            dir: dir.join("projects").join("p1").join("config"),
            tables: vec![b], constants: Vec::new(), enums: Vec::new(),
            is_new: false, storage: crate::model::GroupStorage::Xlsx, book_stamp: None,
        });

        // tbl 组：表 A 的 ref 列引用 B，并挂一条 ref_exists(B.id)
        let ref_field = FieldDef {
            name: "ref".into(), desc: String::new(),
            tbl_type: "@B".into(), export: Export::ClientServer,
        };
        let a = Table {
            name: "A".into(), original_name: "A".into(),
            path: dir.join("A.tbl"),
            schema: TableSchema { fields: vec![id_field(), ref_field.clone()] },
            records: vec![vec!["1".into(), "1".into()]],
            dirty: false, deleted: false,
            original_records: vec![vec!["1".into(), "1".into()]],
            original_fields: vec![id_field(), ref_field],
            saved: true, fmt_ver: crate::tbl::DEFAULT_TBL_VERSION,
            display_field: None, original_display_field: None,
            rules: vec![Rule {
                column: "ref".into(),
                kind: RuleKind::RefExists { target_node: "B".into(), target_column: "id".into() },
                level: Severity::Error,
                source: RuleSource::User,
            }],
            original_rules: Vec::new(),
        };
        project.groups.push(Group {
            name: "tg".into(), original_name: "tg".into(),
            dir: dir.join("tg"),
            tables: vec![a], constants: Vec::new(), enums: Vec::new(),
            is_new: false, storage: crate::model::GroupStorage::Tbl, book_stamp: None,
        });

        let mut engine = ProjectEngine::new(project, GlobalConfig::default());
        engine.revalidate_all();
        assert!(engine.validation_errors.is_empty(), "ref=1 能在 xlsx 的 B.id 里找到");

        // 改 xlsx 组的 B.id，然后只重算 B —— A 的 @B 与 ref_exists 都应被连带刷新
        if let Some(t) = engine.find_table_mut("xg", "B") {
            t.records[0][0] = "9".into();
            t.update_dirty();
        }
        engine.revalidate("xg", "B");
        assert!(!engine.validation_errors.is_empty(),
            "跨存储依赖：B 改了，tbl 组的 A 必须重新判定");
    }

    /// 校验不区分存储后端：xlsx 组与 tbl 组走同一套规则（@02.27）。
    #[test]
    fn xlsx_group_uses_the_same_rule_validation() {
        let dir = unique_tmp("xlsx_parity");
        let mut project = make_project(
            dir.clone(), dir.join("projects").join("p1"), "p1",
        );
        let t = Table {
            name: "XT".into(), original_name: "XT".into(),
            path: dir.join("xg.xlsx"),
            schema: TableSchema { fields: vec![id_field()] },
            records: vec![vec!["99".into()]],
            dirty: false, deleted: false,
            original_records: vec![vec!["99".into()]],
            original_fields: vec![id_field()],
            saved: true, fmt_ver: crate::tbl::DEFAULT_TBL_VERSION,
            display_field: None, original_display_field: None,
            rules: vec![Rule {
                column: "id".into(),
                kind: RuleKind::Range { min: "1".into(), max: "10".into() },
                level: Severity::Error,
                source: RuleSource::User,
            }],
            original_rules: Vec::new(),
        };
        project.groups.push(Group {
            name: "xg".into(), original_name: "xg".into(),
            dir: dir.join("projects").join("p1").join("config"),
            tables: vec![t], constants: Vec::new(), enums: Vec::new(),
            is_new: false, storage: crate::model::GroupStorage::Xlsx, book_stamp: None,
        });
        let mut engine = ProjectEngine::new(project, GlobalConfig::default());
        engine.revalidate_all();
        assert_eq!(engine.validation_errors.len(), 1,
            "xlsx 组的规则与 tbl 组走同一套校验");
    }

    #[test]
    fn validation_policy_controls_blocking() {
        let (_dir, mut engine) = engine_with_saved_node("policy_block", "g", "T");
        engine.set_node_rules("g", "T", vec![
            Rule {
                column: "id".into(),
                kind: RuleKind::RefExists { target_node: "Lang".into(), target_column: "key".into() },
                level: Severity::Warning,

                source: RuleSource::User,
            },
        ]);
        engine.revalidate("g", "T");
        // 默认策略：warn → report，不阻断
        assert!(engine.validation_errors.is_empty());
        assert_eq!(engine.validation_warnings.len(), 1);
        assert!(engine.save_all());

        // 把 warning 改成 block → 同一条问题开始拦保存
        engine.project_mut().config.validate.warning = SeverityAction::Block;
        engine.revalidate("g", "T");
        assert_eq!(engine.validation_errors.len(), 1, "warning 被提升为阻断");
        assert!(engine.validation_warnings.is_empty());
        assert!(!engine.save_all(), "阻断级存在时保存必须失败");

        // 改成 ignore → 什么都不产生
        engine.project_mut().config.validate.warning = SeverityAction::Ignore;
        engine.revalidate("g", "T");
        assert!(engine.validation_errors.is_empty());
        assert!(engine.validation_warnings.is_empty());
        assert!(engine.save_all());
    }

    #[test]
    fn warn_level_rule_with_missing_target_does_not_block_save() {
        let (_dir, mut engine) = engine_with_saved_node("rules_warn", "g", "T");
        assert!(engine.set_node_rules("g", "T", vec![
            Rule {
                column: "id".into(),
                kind: RuleKind::RefExists { target_node: "Lang".into(), target_column: "key".into() },
                level: Severity::Warning,

                source: RuleSource::User,
            },
        ]));
        engine.revalidate("g", "T");
        assert!(engine.validation_errors.is_empty(), "warn 级规则的目标缺失只算警告");
        assert_eq!(engine.validation_warnings.len(), 1);
        assert!(engine.save_all());
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

    /// 手改 `.tbl`（加一列）后 `save`，`project.tblschema` 必须跟着刷新。
    ///
    /// `schema.sections` 是 groups 的投影，手改数据文件不会让任何 `original_*` 变化 ——
    /// 旧代码只看内存脏标，于是**永远**跳过这次写入，投影停在上一次结构变更时的样子。
    /// CLI 的工作流正是「改 .tbl → workspace save → 下一次以 schema 为准的操作」。
    #[test]
    fn saving_refreshes_a_schema_projection_left_stale_by_a_hand_edited_tbl() {
        let (dir, _engine) = engine_with_saved_node("stale_proj", "hero", "HeroBase");
        let tbl = config_dir(&dir).join("hero").join("HeroBase.tbl");
        // 手写一份多一列的 .tbl：v3 自描述，字段定义就在文件里
        std::fs::write(&tbl, "#!tbl v3\n#mode table\n---\n\n\
@field id\n  desc:ID\n  export:cs\n  type:int\n\n\
@field hp\n  desc:血量\n  export:cs\n  type:int\n\n\
[1]\n  hp:10\n").unwrap();

        // 重新加载：内存与盘一致，**不脏**
        let loaded = crate::project::load_specific_project(&dir, "p1").expect("reload");
        assert!(!loaded.schema_changed(), "刚加载的项目不该显示为改过");
        let mut engine = ProjectEngine::new(loaded, GlobalConfig::default());

        assert!(engine.save_all());

        let schema = std::fs::read_to_string(
            dir.join("projects/p1").join(crate::project::PROJECT_SCHEMA_FILE)).unwrap();
        assert!(schema.contains("hp | int | cs | 血量"),
            "save 没刷新投影:\n{}", schema);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 新建的空 Group 要能落盘活下来。
    ///
    /// 以前的清理逻辑是「三个 vec 都空 → 删目录 + 从内存移除」。CLI 每条命令一个进程，
    /// 于是 `schema add-group` 保存后空组当场消失，下一条 `schema add-table
    /// --group <新组>` 就查无此组（而且以前还会报"已添加 Table"）。
    #[test]
    fn a_new_empty_group_survives_save() {
        let (dir, mut engine) = engine_with_saved_node("empty_group", "hero", "A");
        let pid = engine.active_project_id().unwrap().to_string();
        engine.execute_action(&ProjectAction::NewGroup {
            project_id: pid, name: "fresh".into(),
            storage: crate::model::GroupStorage::Tbl,
        });
        assert!(engine.save_all(), "空组不该让保存失败");

        assert!(config_dir(&dir).join("fresh").is_dir(), "空组目录应已创建");
        let loaded = crate::project::load_specific_project(&dir, "p1").expect("reload");
        assert!(loaded.groups.iter().any(|g| g.name == "fresh"),
            "重新加载后空组应还在: {:?}", loaded.groups.iter().map(|g| &g.name).collect::<Vec<_>>());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 反过来：节点被删光的组要连目录一起消失，否则下次加载又冒出来一个空组。
    #[test]
    fn deleting_every_node_removes_the_group_directory() {
        let (dir, mut engine) = engine_with_saved_node("del_all", "hero", "A");
        engine.delete_group("hero");
        assert!(engine.save_all());
        assert!(!config_dir(&dir).join("hero").exists(), "删空的组目录应被清掉");
        let loaded = crate::project::load_specific_project(&dir, "p1").expect("reload");
        assert!(loaded.groups.is_empty(), "重新加载后不该再出现: {:?}",
            loaded.groups.iter().map(|g| &g.name).collect::<Vec<_>>());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── Xlsx 组（@02.27）────────────────────────────────────────────────

    /// 起一个已落盘的项目，再往里面加一个 xlsx 组 + 一张表（带数据）。
    fn engine_with_xlsx_group(label: &str) -> (PathBuf, ProjectEngine) {
        use crate::model::GroupStorage;
        let (dir, mut engine) = engine_with_saved_node(label, "hero", "A");
        let pid = engine.active_project_id().unwrap().to_string();
        engine.execute_action(&ProjectAction::NewGroup {
            project_id: pid.clone(), name: "xg".into(), storage: GroupStorage::Xlsx,
        });
        engine.execute_action(&ProjectAction::NewTable {
            project_id: pid, group: "xg".into(), name: "Hot".into(),
            fmt_ver: crate::tbl::DEFAULT_TBL_VERSION,
        });
        {
            let t = engine.find_table_mut("xg", "Hot").expect("Hot");
            t.schema.fields.push(FieldDef {
                name: "name".into(), desc: "名字".into(),
                tbl_type: "str".into(), export: Export::ClientServer,
            });
            t.records = vec![
                vec!["1".into(), "alpha".into()],
                vec!["2".into(), "beta".into()],
            ];
            t.dirty = true;
        }
        (dir, engine)
    }

    fn xlsx_book(dir: &Path) -> PathBuf {
        config_dir(dir).join("xg.xlsx")
    }

    #[test]
    fn xlsx_group_round_trips_data_through_a_workbook() {
        let (dir, mut engine) = engine_with_xlsx_group("xlsx_rt");
        assert!(engine.save_all(), "首次保存应成功");
        assert!(xlsx_book(&dir).is_file(), "应生成 xg.xlsx");

        let loaded = crate::project::load_specific_project(&dir, "p1").expect("reload");
        let g = loaded.groups.iter().find(|g| g.name == "xg").expect("xg");
        assert_eq!(g.storage, crate::model::GroupStorage::Xlsx);
        let t = g.tables.iter().find(|t| t.name == "Hot").expect("Hot");
        assert_eq!(t.schema.fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
                   vec!["id", "name"]);
        assert_eq!(t.records, vec![
            vec!["1".to_string(), "alpha".to_string()],
            vec!["2".to_string(), "beta".to_string()],
        ]);
        // desc 以 sheet 为准（用户拍板）：schema 里那句被 sheet 带回
        assert_eq!(t.schema.fields[1].desc, "名字");
        assert!(!loaded.is_dirty(), "刚加载的项目不该是脏的");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn renaming_xlsx_group_moves_root_workbook() {
        let (dir, mut engine) = engine_with_xlsx_group("xlsx_rename");
        assert!(engine.save_all());
        let pid = engine.active_project_id().unwrap().to_string();
        engine.execute_action(&ProjectAction::RenameGroup {
            project_id: pid,
            old_name: "xg".into(),
            new_name: "yg".into(),
        });
        assert!(engine.save_all(), "改名后应能保存");
        assert!(!config_dir(&dir).join("xg.xlsx").exists(), "旧工作簿不应还在");
        assert!(config_dir(&dir).join("yg.xlsx").is_file(), "新工作簿应在 config 根");
        let loaded = crate::project::load_specific_project(&dir, "p1").expect("reload");
        let g = loaded.groups.iter().find(|g| g.name == "yg").expect("yg");
        assert_eq!(g.storage, crate::model::GroupStorage::Xlsx);
        assert_eq!(g.dir, config_dir(&dir), "xlsx 组的 dir 是 config 根");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn emptying_xlsx_group_never_deletes_config_root() {
        let (dir, mut engine) = engine_with_xlsx_group("xlsx_empty_cleanup");
        assert!(engine.save_all());
        engine.delete_node("xg", "Hot");
        assert!(engine.save_all(), "删空组应能保存");
        assert!(!config_dir(&dir).join("xg.xlsx").exists(), "工作簿应被删除");
        assert!(config_dir(&dir).is_dir(), "config 根绝不能跟着被删");
        assert!(config_dir(&dir).join("hero").join("A.tbl").is_file(), "其它组不受影响");
        let loaded = crate::project::load_specific_project(&dir, "p1").expect("reload");
        assert!(!loaded.groups.iter().any(|g| g.name == "xg"), "空组应消失");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn old_xlsx_directory_layout_is_rejected() {
        let (dir, mut engine) = engine_with_xlsx_group("xlsx_old_layout");
        assert!(engine.save_all());
        let config = config_dir(&dir);
        let old_dir = config.join("xg");
        std::fs::create_dir_all(&old_dir).unwrap();
        std::fs::rename(config.join("xg.xlsx"), old_dir.join("xg.xlsx")).unwrap();
        let err = crate::project::load_specific_project(&dir, "p1")
            .expect_err("旧布局必须报错")
            .to_string();
        assert!(err.contains("旧布局"), "错误应指出旧布局: {err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn duplicate_xlsx_dir_and_file_is_rejected() {
        let (dir, mut engine) = engine_with_xlsx_group("xlsx_dup");
        assert!(engine.save_all());
        std::fs::create_dir_all(config_dir(&dir).join("xg")).unwrap();
        let err = crate::project::load_specific_project(&dir, "p1")
            .expect_err("同名目录和工作簿必须报错")
            .to_string();
        assert!(err.contains("同时存在"), "错误应指出同名冲突: {err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **干净的组一个字节都不许碰。** umya 每次写盘都会重打 zip + 打当前时间戳，
    /// 所以"顺手保存"会让 git 每次多一个改动（实测见 @02.27）。
    #[test]
    fn xlsx_group_noop_save_does_not_touch_the_file() {
        let (dir, mut engine) = engine_with_xlsx_group("xlsx_noop");
        assert!(engine.save_all());
        let book = xlsx_book(&dir);
        let before = std::fs::read(&book).expect("read book");
        let mtime = std::fs::metadata(&book).unwrap().modified().unwrap();

        // mtime 的分辨率够细，但只要写入发生在同一时间片里就可能相同；睡一下更稳。
        std::thread::sleep(std::time::Duration::from_millis(30));
        assert!(engine.save_all(), "干净保存也应返回成功");
        let after = std::fs::read(&book).expect("read book");
        let mtime2 = std::fs::metadata(&book).unwrap().modified().unwrap();
        assert_eq!(before, after, "干净保存把 workbook 重写了");
        assert_eq!(mtime, mtime2, "干净保存不该碰文件（mtime 变了说明写了一次）");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 只改一张表，其它 sheet 的 zip 条目必须**逐字节不变**（lazy_read 的保真契约）。
    #[test]
    fn saving_one_sheet_leaves_the_other_byte_identical() {
        let (dir, mut engine) = engine_with_xlsx_group("xlsx_isolate");
        assert!(engine.save_all());
        // 加第二张表，让 schema 里有两张（Hot 是 sheet1、Cold 是 sheet2）
        let pid = engine.active_project_id().unwrap().to_string();
        engine.execute_action(&ProjectAction::NewTable {
            project_id: pid, group: "xg".into(), name: "Cold".into(),
            fmt_ver: crate::tbl::DEFAULT_TBL_VERSION,
        });
        assert!(engine.save_all());
        let book = xlsx_book(&dir);

        // **必须用另一个库重写一遍**（模拟 Excel 自己保存过）。umya 重序列化**自己的**
        // 输出几乎是幂等的，拿 umya 造的文件测「有没有被重写」测不出来。
        {
            use rust_xlsxwriter::Workbook;
            let mut wb = Workbook::new();
            {
                let ws = wb.add_worksheet();
                ws.set_name("Hot").unwrap();
                for (c, v) in ["ID", "名字"].iter().enumerate() {
                    ws.write_string(0, c as u16, *v).unwrap();
                }
                for (c, v) in ["前后端", "前后端"].iter().enumerate() {
                    ws.write_string(1, c as u16, *v).unwrap();
                }
                for (c, v) in ["int", "str"].iter().enumerate() {
                    ws.write_string(2, c as u16, *v).unwrap();
                }
                for (c, v) in ["id", "name"].iter().enumerate() {
                    ws.write_string(3, c as u16, *v).unwrap();
                }
                ws.write_string(4, 0, "1").unwrap();
                ws.write_string(4, 1, "alpha").unwrap();
                ws.write_string(5, 0, "2").unwrap();
                ws.write_string(5, 1, "beta").unwrap();
            }
            {
                let ws = wb.add_worksheet();
                ws.set_name("Cold").unwrap();
                for (c, v) in ["ID"].iter().enumerate() {
                    ws.write_string(0, c as u16, *v).unwrap();
                }
                ws.write_string(1, 0, "前后端").unwrap();
                ws.write_string(2, 0, "int").unwrap();
                ws.write_string(3, 0, "id").unwrap();
            }
            std::fs::write(&book, wb.save_to_buffer().unwrap()).unwrap();
        }

        // 重新加载，此时 workbook 是「别人写的」
        let loaded = crate::project::load_specific_project(&dir, "p1").expect("reload");
        let mut engine = ProjectEngine::new(loaded, GlobalConfig::default());
        let untouched = crate::excel_sync::testutil::zip_entry(&book, "xl/worksheets/sheet2.xml");

        // 只改 Hot
        {
            let t = engine.find_table_mut("xg", "Hot").expect("Hot");
            t.records[0][1] = "changed".into();
            t.dirty = true;
        }
        assert!(engine.save_all());

        let after = crate::excel_sync::testutil::zip_entry(&book, "xl/worksheets/sheet2.xml");
        assert_eq!(untouched, after, "没改的 sheet 被重写了（lazy_read 失守）");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// schema 有 section、workbook 没有 sheet → 打不开（用户拍板：硬报错，不兜底成空表）。
    #[test]
    fn section_without_a_sheet_fails_the_load() {
        let (dir, mut engine) = engine_with_xlsx_group("xlsx_missing");
        assert!(engine.save_all());

        // 手工把 schema 里加一个没有 sheet 的 section
        let schema_path = dir.join("projects/p1").join(crate::project::PROJECT_SCHEMA_FILE);
        let mut txt = std::fs::read_to_string(&schema_path).unwrap();
        txt.push_str("\n[xg/Ghost] table\nid | int | cs | ID\n");
        std::fs::write(&schema_path, txt).unwrap();

        let err = crate::project::load_specific_project(&dir, "p1")
            .expect_err("缺 sheet 应当打不开");
        let msg = format!("{:#}", err);
        assert!(msg.contains("Ghost"), "错误里要点名: {}", msg);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 工具开着时外部改了 workbook → 拒绝保存，别静默覆盖策划的改动。
    #[test]
    fn external_change_blocks_the_save() {
        let (dir, mut engine) = engine_with_xlsx_group("xlsx_ext");
        assert!(engine.save_all());
        let book = xlsx_book(&dir);

        // 模拟外部编辑：改内容 + 换 mtime
        let mut bytes = std::fs::read(&book).unwrap();
        bytes.push(0);
        std::fs::write(&book, bytes).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));

        {
            let t = engine.find_table_mut("xg", "Hot").expect("Hot");
            t.records[0][1] = "tool-change".into();
            t.dirty = true;
        }
        assert!(!engine.save_all(), "外部改过时必须拒绝保存");
        assert!(engine.find_table("xg", "Hot").unwrap().dirty, "拒绝后节点应仍是脏的");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 外部改过、且组是干净的 → GUI 的轮询可以安全重载。
    #[test]
    fn an_external_change_is_detected_and_reloadable_when_clean() {
        let (dir, mut engine) = engine_with_xlsx_group("xlsx_reload");
        assert!(engine.save_all());
        let book = xlsx_book(&dir);
        {
            // 模拟 Excel 改了一个格子（B6 = 第二条记录的 name）
            let mut b = umya_spreadsheet::reader::xlsx::lazy_read(&book).unwrap();
            let ws = b.sheet_by_name_mut("Hot").unwrap();
            ws.cell_mut((2, 6)).set_value("external");
            umya_spreadsheet::writer::xlsx::write(&b, &book).unwrap();
        }

        let changes = engine.xlsx_external_changes();
        assert_eq!(changes.len(), 1, "应检出外部改动");
        assert!(!changes[0].dirty, "干净组");

        engine.reload_xlsx_group("p1", "xg").expect("重载应成功");
        assert_eq!(engine.find_table("xg", "Hot").unwrap().records[1][1], "external");
        assert!(engine.xlsx_external_changes().is_empty(), "重载后应不再有改动");
        assert!(!engine.find_project("p1").unwrap().is_dirty(), "重载不该把项目弄脏");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 有未保存改动时拒绝重载（会把人手上的东西抽走）。
    #[test]
    fn reload_refuses_a_dirty_group() {
        let (dir, mut engine) = engine_with_xlsx_group("xlsx_reload_dirty");
        assert!(engine.save_all());
        {
            let t = engine.find_table_mut("xg", "Hot").unwrap();
            t.records[0][1] = "mine".into();
            t.dirty = true;
        }
        assert!(engine.reload_xlsx_group("p1", "xg").is_err(), "脏组不该被重载");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// id 列是系统列：不能删，也不能在它左边插列。
    #[test]
    fn the_id_column_cannot_be_deleted_or_shifted() {
        let (dir, mut engine) = engine_with_saved_node("id_col", "hero", "A");

        engine.insert_column("hero", "A", 0);
        let t = engine.find_table("hero", "A").unwrap();
        assert_eq!(t.schema.fields.len(), 1, "id 左侧不该插进来");
        assert_eq!(t.schema.fields[0].name, "id");

        engine.delete_column("hero", "A", 0);
        let t = engine.find_table("hero", "A").unwrap();
        assert_eq!(t.schema.fields.len(), 1, "id 不该被删");
        assert_eq!(t.schema.fields[0].name, "id");

        // id 右侧可以插
        engine.insert_column("hero", "A", 1);
        assert_eq!(engine.find_table("hero", "A").unwrap().schema.fields.len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 整组粘贴时，子项也要按项目级唯一命名空间加 `_copy`，并改写组内引用。
    #[test]
    fn pasting_group_suffixes_colliding_children_and_rewrites_refs() {
        use crate::model::GroupStorage;
        let (dir, mut engine) = engine_with_saved_node("paste_child_dedupe", "hero", "A");
        let pid = engine.active_project_id().unwrap().to_string();
        engine.execute_action(&ProjectAction::NewTable {
            project_id: pid.clone(), group: "hero".into(), name: "B".into(),
            fmt_ver: crate::tbl::DEFAULT_TBL_VERSION,
        });
        {
            let t = engine.find_table_mut("hero", "A").expect("A");
            t.schema.fields.push(FieldDef {
                name: "self_ref".into(),
                desc: String::new(),
                tbl_type: "@A".into(),
                export: Export::ClientServer,
            });
            t.schema.fields.push(FieldDef {
                name: "other_ref".into(),
                desc: String::new(),
                tbl_type: "@B".into(),
                export: Export::ClientServer,
            });
            t.records[0].push("1".into());
            t.records[0].push(String::new());
            t.update_dirty();
        }
        engine.clipboard_copy_group(&pid, "hero");
        let new_group = engine.paste_group_as(&pid, Some(GroupStorage::Xlsx)).expect("粘贴应成功");
        let project = engine.find_project(&pid).unwrap();
        let pasted = project.groups.iter().find(|g| g.name == new_group).expect("新组");
        assert!(pasted.tables.iter().any(|t| t.name == "B_copy"), "第二个子项也要加 _copy");
        let t = pasted.tables.iter().find(|t| t.name == "A_copy").expect("子项应加 _copy");
        assert_eq!(t.schema.fields[1].tbl_type, "@A_copy", "自引用必须跟着改名");
        assert_eq!(t.schema.fields[2].tbl_type, "@B_copy", "对同组其它节点的引用也必须跟着改名");
        assert!(engine.validate().iter().all(|e| !e.contains("节点名")), "不应有重名错误: {:?}", engine.validate());
        assert!(engine.save_all(), "去重后应能保存: {:?}", engine.validate());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 粘贴命名对话框的数据侧：预览给出 `_copy` 预填，确认时用用户改的名字
    /// 一次性完成改名 + 组内引用改写；名字非法/重复则不落任何数据。
    #[test]
    fn preview_then_named_paste_rewrites_refs_and_rejects_duplicates() {
        use crate::model::GroupStorage;
        use std::collections::BTreeMap;

        let (dir, mut engine) = engine_with_saved_node("paste_named_preview", "hero", "A");
        let pid = engine.active_project_id().unwrap().to_string();
        engine.execute_action(&ProjectAction::NewTable {
            project_id: pid.clone(), group: "hero".into(), name: "B".into(),
            fmt_ver: crate::tbl::DEFAULT_TBL_VERSION,
        });
        {
            let t = engine.find_table_mut("hero", "A").expect("A");
            t.schema.fields.push(FieldDef {
                name: "self_ref".into(),
                desc: String::new(),
                tbl_type: "@A".into(),
                export: Export::ClientServer,
            });
            t.schema.fields.push(FieldDef {
                name: "other_ref".into(),
                desc: String::new(),
                tbl_type: "@B".into(),
                export: Export::ClientServer,
            });
            t.records[0].push("1".into());
            t.records[0].push(String::new());
            t.update_dirty();
        }
        engine.clipboard_copy_group(&pid, "hero");

        let preview = engine.preview_paste_group(&pid).expect("剪贴板里是 Group");
        assert!(preview.has_conflict, "组名与子项名都冲突");
        assert_eq!(preview.group_name, "hero_copy");
        let row = |name: &str| preview.nodes.iter().find(|n| n.old_name == name).expect(name);
        assert_eq!(row("A").new_name, "A_copy", "子项预填 _copy");
        assert_eq!(row("B").new_name, "B_copy", "子项预填 _copy");

        let mut names = BTreeMap::new();
        names.insert("A".to_string(), "HeroA".to_string());
        names.insert("B".to_string(), "HeroB".to_string());
        let group = engine
            .paste_group_as_with_names(&pid, Some(GroupStorage::Tbl), "hero_renamed", &names)
            .expect("用户确认的名字应能粘贴");
        assert_eq!(group, "hero_renamed");

        let project = engine.find_project(&pid).unwrap();
        let pasted = project.groups.iter().find(|g| g.name == group).expect("新组");
        let a = pasted.tables.iter().find(|t| t.name == "HeroA").expect("改名后的 A");
        let b = pasted.tables.iter().find(|t| t.name == "HeroB").expect("改名后的 B");
        assert_eq!(a.schema.fields[1].tbl_type, "@HeroA", "自引用跟着改名");
        assert_eq!(a.schema.fields[2].tbl_type, "@HeroB", "同组引用跟着改名");
        assert_eq!(b.name, "HeroB");
        assert!(engine.save_all(), "确认后的组应能保存: {:?}", engine.validate());

        // 再次确认：同名计划必须整体失败，且不产生半截 Group。
        let before = project_groups(&engine, &pid);
        let mut dup = BTreeMap::new();
        dup.insert("A".to_string(), "Same".to_string());
        dup.insert("B".to_string(), "SAME".to_string());
        let err = engine
            .paste_group_as_with_names(&pid, Some(GroupStorage::Tbl), "hero_bad", &dup)
            .expect_err("新组内两个子项同名必须拒绝");
        assert!(err.contains("节点名已存在"), "{err}");
        assert_eq!(project_groups(&engine, &pid), before, "失败不能留下任何组");

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn project_groups(engine: &ProjectEngine, project_id: &str) -> Vec<String> {
        engine
            .find_project(project_id)
            .map(|p| p.groups.iter().map(|g| g.name.clone()).collect())
            .unwrap_or_default()
    }

    #[test]
    fn pasting_node_uses_project_wide_name_uniqueness() {
        use crate::model::GroupStorage;
        let (dir, mut engine) = engine_with_saved_node("paste_node_dedupe", "hero", "A");
        let pid = engine.active_project_id().unwrap().to_string();
        engine.execute_action(&ProjectAction::NewGroup {
            project_id: pid.clone(), name: "other".into(), storage: GroupStorage::Tbl,
        });
        engine.clipboard_copy_node(&pid, "hero", "A", NodeKind::Table);
        let pasted = engine.paste_node_to(&pid, "other", None).expect("粘贴应成功");
        assert_eq!(pasted, "A_copy", "目标组没有 A，但项目内已有 A，仍要加 _copy");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn duplicate_node_names_block_export() {
        use crate::model::GroupStorage;
        let (dir, mut engine) = engine_with_saved_node("dup_node_export", "hero", "A");
        let pid = engine.active_project_id().unwrap().to_string();
        engine.execute_action(&ProjectAction::NewGroup {
            project_id: pid, name: "other".into(), storage: GroupStorage::Tbl,
        });
        engine.execute_action(&ProjectAction::NewTable {
            project_id: engine.active_project_id().unwrap().to_string(),
            group: "other".into(),
            name: "A".into(),
            fmt_ver: crate::tbl::DEFAULT_TBL_VERSION,
        });
        let err = engine.export_json().expect_err("重名必须阻断导出").to_string();
        assert!(err.contains("节点名") && err.contains("重复"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn duplicate_node_names_are_blocking_errors() {
        use crate::model::GroupStorage;
        let (dir, mut engine) = engine_with_saved_node("dup_node_name", "hero", "A");
        let pid = engine.active_project_id().unwrap().to_string();
        engine.execute_action(&ProjectAction::NewGroup {
            project_id: pid, name: "other".into(), storage: GroupStorage::Tbl,
        });
        engine.execute_action(&ProjectAction::NewTable {
            project_id: engine.active_project_id().unwrap().to_string(),
            group: "other".into(),
            name: "A".into(),
            fmt_ver: crate::tbl::DEFAULT_TBL_VERSION,
        });
        engine.revalidate_all();
        assert!(engine.validate().iter().any(|e| e.contains("节点名")), "应报节点名重复");
        assert!(!engine.save_all(), "节点名重复必须阻断保存");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 整组粘贴到另一种后端：节点内容不变，只换保存写入器（@02.26.8）。
    #[test]
    fn pasting_a_group_into_the_other_backend_converts_it() {
        use crate::model::GroupStorage;
        let (dir, mut engine) = engine_with_saved_node("paste_backend", "hero", "A");
        let pid = engine.active_project_id().unwrap().to_string();
        engine.clipboard_copy_group(&pid, "hero");
        let new_name = engine
            .paste_group_as(&pid, Some(GroupStorage::Xlsx))
            .expect("粘贴应成功");
        assert!(engine.save_all());

        // 新组：xlsx，工作簿里有 sheet A
        let book = config_dir(&dir).join(format!("{}.xlsx", new_name));
        assert!(book.is_file(), "应生成 {}", book.display());
        assert!(config_dir(&dir).join("hero").join("A.tbl").is_file(), "源组保持 tbl");

        let loaded = crate::project::load_specific_project(&dir, "p1").expect("reload");
        let pasted = loaded.groups.iter().find(|g| g.name == new_name).expect("新组");
        assert_eq!(pasted.storage, GroupStorage::Xlsx);
        assert_eq!(pasted.tables.len(), 1);
        assert_eq!(pasted.tables[0].name, "A_copy", "子项必须带 _copy 后缀");
        assert_eq!(pasted.tables[0].records, vec![vec!["1".to_string()]]);
        let src = loaded.groups.iter().find(|g| g.name == "hero").expect("源组");
        assert_eq!(src.storage, GroupStorage::Tbl);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 被拦下的保存**什么都不写**——包括 `project.tblschema`。
    ///
    /// schema 是 groups 的投影。先写 schema、再发现某个组写不进去，就会造出
    /// 「schema 里有这个节点、盘上却没有它的数据」——项目当场打不开。
    #[test]
    fn a_blocked_xlsx_save_writes_nothing_including_the_schema() {
        let (dir, mut engine) = engine_with_xlsx_group("xlsx_atomic");
        assert!(engine.save_all());
        let book_dir = config_dir(&dir);
        std::fs::write(book_dir.join("~$xg.xlsx"), b"lock").unwrap();

        let pid = engine.active_project_id().unwrap().to_string();
        engine.execute_action(&ProjectAction::NewTable {
            project_id: pid, group: "xg".into(), name: "Extra".into(),
            fmt_ver: crate::tbl::DEFAULT_TBL_VERSION,
        });
        let schema_path = dir.join("projects/p1").join(crate::project::PROJECT_SCHEMA_FILE);
        let before = std::fs::read_to_string(&schema_path).unwrap();

        assert!(!engine.save_all(), "被占用时必须失败");
        assert_eq!(std::fs::read_to_string(&schema_path).unwrap(), before,
            "失败时 schema 不该被写");

        // 项目仍然可加载（schema 与盘上仍然是上一次成功保存的状态）
        std::fs::remove_file(book_dir.join("~$xg.xlsx")).unwrap();
        crate::project::load_specific_project(&dir, "p1").expect("项目应仍可打开");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Excel 正开着（有 `~$` 锁文件）→ 拒绝保存。
    #[test]
    fn excel_lock_blocks_the_save() {
        let (dir, mut engine) = engine_with_xlsx_group("xlsx_lock");
        assert!(engine.save_all());
        let lock = config_dir(&dir).join("~$xg.xlsx");
        std::fs::write(&lock, b"lock").unwrap();

        {
            let t = engine.find_table_mut("xg", "Hot").expect("Hot");
            t.records[0][1] = "tool-change".into();
            t.dirty = true;
        }
        assert!(!engine.save_all(), "被 Excel 占用时必须拒绝保存");

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
            storage: crate::model::GroupStorage::Tbl, book_stamp: None,
            tables: vec![Table {
                name: "T".into(), original_name: "T".into(), path: gdir.join("T.tbl"),
                schema: TableSchema { fields: vec![id_field()] },
                records: vec![vec!["1".into()]],
                dirty: true, deleted: false,
                original_records: Vec::new(), original_fields: Vec::new(),
                saved: false, fmt_ver: crate::tbl::DEFAULT_TBL_VERSION, display_field: None,
            original_display_field: None,

                rules: Vec::new(),
                original_rules: Vec::new(),
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

            rules: Vec::new(),
            original_rules: Vec::new(),
        };
        let mut project = make_project(dir.clone(), project_root, "p1");
        project.groups.push(Group {
            name: "hero".into(), original_name: "hero".into(), dir: gdir.clone(),
            storage: crate::model::GroupStorage::Tbl, book_stamp: None,
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
            fmt_ver: crate::tbl::DEFAULT_TBL_VERSION,
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

