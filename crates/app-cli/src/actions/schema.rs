//! schema 子命令：show / add-* / rename-* / delete-*。
//!
//! 所有写操作执行后自动 save_all()。
//!
//! **每个写操作都先校验、再动手、最后看落盘结果。** 这三步缺一不可：
//! - 不校验：`add-table --name bad_name` 会造出生成不出合法类名的节点，
//!   `add-table` 撞名会把同名 `.tbl` 整个覆盖掉（实测过：一张有数据的表变回只有 id 一列）。
//! - 不看返回值：`save_all()` 在项目有验证错误时**什么都写**，只返回 false；
//!   照样 println"已添加"，脚本会以为成功了。
//! - 不查目标存在：`rename-node` / `delete-node` 对不存在的节点是静默 no-op。
//!
//! 这三件事对交互式使用只是"少一句提示"，对 AI / 脚本就是"错了却一片绿"。

use anyhow::{Result, bail};
use tablet_core::ops::{ProjectAction, ProjectEngine};

#[derive(serde::Serialize)]
pub struct SchemaShowEntry {
    pub group: String,
    pub nodes: Vec<SchemaNodeInfo>,
}

#[derive(serde::Serialize)]
pub struct SchemaNodeInfo {
    pub name: String,
    pub kind: &'static str,
    pub detail: String,
}

pub fn run_schema_show(engine: &ProjectEngine) -> Result<Vec<SchemaShowEntry>> {
    let project = engine.active()
        .ok_or_else(|| anyhow::anyhow!("没有活跃的 Project"))?;

    let mut entries = Vec::new();
    for g in &project.groups {
        let mut nodes = Vec::new();
        for t in &g.tables {
            nodes.push(SchemaNodeInfo {
                name: t.name.clone(),
                kind: "table",
                detail: format!("{} fields, {} rows", t.schema.fields.len(), t.records.len()),
            });
        }
        for c in &g.constants {
            nodes.push(SchemaNodeInfo {
                name: c.name.clone(),
                kind: "constant",
                detail: format!("{} entries", c.entries.len()),
            });
        }
        for e in &g.enums {
            nodes.push(SchemaNodeInfo {
                name: e.name.clone(),
                kind: "enum",
                detail: format!("{} entries", e.entries.len()),
            });
        }
        entries.push(SchemaShowEntry { group: g.name.clone(), nodes });
    }
    Ok(entries)
}

pub fn run_schema_add_group(
    engine: &mut ProjectEngine,
    name: &str,
    storage: &str,
) -> Result<()> {
    let pid = active_id(engine)?;
    if let Some(err) = engine.validate_group_name(name) {
        bail!("{}", err);
    }
    let storage = match storage {
        "tbl" => tablet_core::model::GroupStorage::Tbl,
        "xlsx" => tablet_core::model::GroupStorage::Xlsx,
        other => bail!("--storage 只接受 tbl / xlsx，收到 {:?}", other),
    };
    engine.execute_action(&ProjectAction::NewGroup {
        project_id: pid.clone(),
        name: name.to_string(),
        storage,
    });
    save_or_err(engine, "新建 Group")
}

pub fn run_schema_add_table(engine: &mut ProjectEngine, group: &str, name: &str) -> Result<()> {
    add_node(engine, group, name, NodeKind::Table)
}

pub fn run_schema_add_constant(engine: &mut ProjectEngine, group: &str, name: &str) -> Result<()> {
    add_node(engine, group, name, NodeKind::Constant)
}

/// 新建 Enum。条目在**同一条命令里**给全 —— 空枚举过不了 `validate_enum`
/// （"枚举至少需要一个条目"），`save_all` 就会整笔落空。以前这条命令没有 `--entry`，
/// 于是它对任何输入都是"声称成功、盘上什么都没有"。
pub fn run_schema_add_enum(
    engine: &mut ProjectEngine,
    group: &str,
    name: &str,
    entries: &[String],
) -> Result<()> {
    let parsed = parse_enum_entries(entries)?;
    let pid = active_id(engine)?;
    ensure_group(engine, group)?;
    if let Some(err) = engine.validate_node_name(name) {
        bail!("{}", err);
    }
    engine.execute_action(&ProjectAction::NewEnum {
        project_id: pid,
        group: group.to_string(),
        name: name.to_string(),
    });
    let node = engine
        .find_enum_mut(group, name)
        .ok_or_else(|| anyhow::anyhow!("新建 Enum 后找不到节点: {}/{}", group, name))?;
    node.entries = parsed;
    save_or_err(engine, "新建 Enum")
}

/// `ID:NAME[:DESC]` → `EnumEntry`。在动手之前全部解析完，错一个就整条命令失败。
fn parse_enum_entries(raw: &[String]) -> Result<Vec<tablet_core::model::EnumEntry>> {
    use tablet_core::validate::is_valid_enum_entry_name;

    let mut out: Vec<tablet_core::model::EnumEntry> = Vec::new();
    for item in raw {
        let mut it = item.splitn(3, ':');
        let id = it.next().unwrap_or("").trim();
        let name = it.next().unwrap_or("").trim();
        let desc = it.next().unwrap_or("").trim();
        if name.is_empty() {
            bail!("--entry \"{}\" 格式应为 ID:NAME[:DESC]", item);
        }
        let id_num: u32 = id
            .parse()
            .map_err(|_| anyhow::anyhow!("--entry \"{}\" 的 id 不是整数", item))?;
        if id_num == 0 {
            bail!("--entry \"{}\" 的 id 必须是正整数（0 保留给「未设置」）", item);
        }
        if !is_valid_enum_entry_name(name) {
            bail!("--entry \"{}\" 的条目名 {:?} 不合法（需大写字母开头，只含大写/数字/下划线）", item, name);
        }
        if out.iter().any(|e| e.id == id || e.name == name) {
            bail!("--entry \"{}\" 的 id 或名字与前面重复", item);
        }
        out.push(tablet_core::model::EnumEntry {
            id: id.to_string(),
            name: name.to_string(),
            desc: desc.to_string(),
        });
    }
    Ok(out)
}

pub fn run_schema_rename_group(engine: &mut ProjectEngine, old: &str, new: &str) -> Result<()> {
    let pid = active_id(engine)?;
    if !group_exists(engine, old) {
        bail!("Group 不存在: {}", old);
    }
    if let Some(err) = engine.validate_group_name_rename(new, old) {
        bail!("{}", err);
    }
    engine.execute_action(&ProjectAction::RenameGroup {
        project_id: pid.clone(),
        old_name: old.to_string(),
        new_name: new.to_string(),
    });
    // 改名现在只在 save 时搬文件（见 ops::apply_pending_renames）。save 因验证错误
    // 中止时盘上什么都没动，这里就不能对外声称"已重命名"。
    save_or_err(engine, "重命名")
}

pub fn run_schema_rename_node(
    engine: &mut ProjectEngine,
    group: &str,
    old: &str,
    new: &str,
) -> Result<()> {
    let pid = active_id(engine)?;
    ensure_group(engine, group)?;
    if !node_exists(engine, group, old) {
        bail!("节点不存在: {}/{}", group, old);
    }
    if let Some(err) = engine.validate_node_name_rename(new, old) {
        bail!("{}", err);
    }
    engine.execute_action(&ProjectAction::RenameNode {
        project_id: pid.clone(),
        group: group.to_string(),
        old_name: old.to_string(),
        new_name: new.to_string(),
    });
    save_or_err(engine, "重命名")
}

pub fn run_schema_delete_group(engine: &mut ProjectEngine, name: &str) -> Result<()> {
    active_id(engine)?;
    if !group_exists(engine, name) {
        bail!("Group 不存在: {}", name);
    }
    engine.delete_group(name);
    save_or_err(engine, "删除 Group")
}

pub fn run_schema_delete_node(engine: &mut ProjectEngine, group: &str, name: &str) -> Result<()> {
    active_id(engine)?;
    ensure_group(engine, group)?;
    if !node_exists(engine, group, name) {
        bail!("节点不存在: {}/{}", group, name);
    }
    engine.delete_node(group, name);
    save_or_err(engine, "删除节点")
}

// ─── 内部 ──────────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum NodeKind {
    Table,
    Constant,
}

impl std::fmt::Display for NodeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            NodeKind::Table => "Table",
            NodeKind::Constant => "Constant",
        })
    }
}

/// 新建节点三条命令共用的前置检查：活跃项目 / 组存在 / 名字合法。
fn add_node(engine: &mut ProjectEngine, group: &str, name: &str, kind: NodeKind) -> Result<()> {
    let pid = active_id(engine)?;
    ensure_group(engine, group)?;
    // `validate_node_name` 查两件事：符合 Java 类名规则（大写开头）+ 全项目唯一
    // （忽略大小写）。撞名必须在这里拦住 —— `NewTable` 是无条件 push，save 时
    // 同路径的旧 `.tbl` 会被直接覆盖。
    if let Some(err) = engine.validate_node_name(name) {
        bail!("{}", err);
    }
    let action = match kind {
        NodeKind::Table => ProjectAction::NewTable {
            project_id: pid,
            group: group.to_string(),
            name: name.to_string(),
        },
        NodeKind::Constant => ProjectAction::NewConstant {
            project_id: pid,
            group: group.to_string(),
            name: name.to_string(),
        },
    };
    engine.execute_action(&action);
    save_or_err(engine, &format!("新建 {}", kind))
}

fn group_exists(engine: &ProjectEngine, group: &str) -> bool {
    engine.active().is_some_and(|p| p.groups.iter().any(|g| g.name == group))
}

fn ensure_group(engine: &ProjectEngine, group: &str) -> Result<()> {
    if group_exists(engine, group) {
        Ok(())
    } else {
        bail!("Group 不存在: {}", group);
    }
}

fn node_exists(engine: &ProjectEngine, group: &str, name: &str) -> bool {
    engine.find_table(group, name).is_some()
        || engine.find_constant(group, name).is_some()
        || engine.find_enum(group, name).is_some()
}

/// `save_all()` 在项目有验证错误时**一个字节都不写**，只返回 false。对外声称"已添加 /
/// 已重命名 / 已删除"的命令必须看这个返回值，否则脚本拿到的是假的成功。
fn save_or_err(engine: &mut ProjectEngine, what: &str) -> Result<()> {
    if engine.save_all() {
        return Ok(());
    }
    let errs = engine.validate();
    if errs.is_empty() {
        // 具体原因（被 Excel 占用 / 外部改过 / 写入失败）由 core 记在 last_save_failure
        let reason = engine.last_save_failure.clone().unwrap_or_else(|| "保存失败".to_string());
        bail!("{}未落盘：{}", what, reason);
    }
    bail!(
        "{}未落盘：项目存在 {} 个验证错误，盘上没有任何改动\n{}",
        what, errs.len(), errs.join("\n")
    )
}

fn active_id(engine: &ProjectEngine) -> Result<String> {
    engine.active()
        .map(|p| p.schema.meta.id.clone())
        .ok_or_else(|| anyhow::anyhow!("没有活跃的 Project"))
}
