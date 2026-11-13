// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 juebanlin <juebanlin@gmail.com>

//! 项目级依赖分析。
//!
//! 只读 `Project` 的当前内存状态，不落盘、不改任何节点：
//! - 节点：未删除的 Table / Enum / Constant；
//! - 边：字段 / 常量类型里的 `@Xxx`（含 `List<@X>` 等嵌套位置）；
//! - 层级：叶子节点为第 1 层，其它节点取「1 + 依赖目标最大层级」；
//! - 角色：基表 / 业务表 / 无入边根表 / 桥接 / 附属候选 / 未接入 / 断引用目标。
//!
//! 规则里的 `ref_exists` 暂不当作依赖边，避免和字段引用重复计数；
//! 后续需要时可以在 `DepEdge.kind` 上扩展。

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::model::Project;
use crate::types::TblType;

#[derive(Debug, Clone, Serialize)]
pub struct DepGraph {
    pub project_id: String,
    pub project_name: String,
    pub nodes: Vec<DepNode>,
    pub edges: Vec<DepEdge>,
    pub summary: DepSummary,
}

#[derive(Debug, Clone, Serialize)]
pub struct DepNode {
    pub id: String,
    pub group: String,
    pub kind: String,
    pub rows: usize,
    pub fields: usize,
    pub layer: Option<u32>,
    /// `base` / `business` / `root` / `bridge` / `satellite` / `orphan` / `constant` / `missing` / `cycle`
    pub role: String,
    /// 附属候选的所属表。
    pub owner: Option<String>,
    pub rules: Vec<String>,
    pub field_names: Vec<String>,
    pub incoming: usize,
    pub outgoing: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DepEdge {
    pub from: String,
    pub to: String,
    pub label: String,
    /// `field` / `bridge` / `attach` / `broken`
    pub kind: String,
    pub broken: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DepSummary {
    pub nodes: usize,
    pub edges: usize,
    pub unconnected: usize,
    pub broken: usize,
    pub cycles: usize,
    pub layers: usize,
    pub roots: usize,
}

#[derive(Debug, Clone)]
struct RawEdge {
    from: String,
    to: String,
    label: String,
}

pub fn analyze(project: &Project) -> DepGraph {
    let mut nodes: BTreeMap<String, DepNode> = BTreeMap::new();
    let mut raw_edges: Vec<RawEdge> = Vec::new();

    for group in &project.groups {
        for table in &group.tables {
            if table.deleted {
                continue;
            }
            let field_names: Vec<String> = table
                .schema
                .fields
                .iter()
                .filter(|f| !f.is_aux())
                .map(|f| f.name.clone())
                .collect();
            let rules: Vec<String> = table.rules.iter().map(|r| r.expr_string()).collect();
            nodes.insert(
                table.name.clone(),
                DepNode {
                    id: table.name.clone(),
                    group: group.name.clone(),
                    kind: "table".into(),
                    rows: table.records.len(),
                    fields: field_names.len(),
                    layer: None,
                    role: String::new(),
                    owner: None,
                    rules,
                    field_names: field_names.clone(),
                    incoming: 0,
                    outgoing: 0,
                },
            );
            for field in table.schema.fields.iter().filter(|f| !f.is_aux()) {
                for target in ref_targets(&field.tbl_type) {
                    raw_edges.push(RawEdge {
                        from: table.name.clone(),
                        to: target,
                        label: field.name.clone(),
                    });
                }
            }
        }

        for constant in &group.constants {
            if constant.deleted {
                continue;
            }
            let entry_names: Vec<String> = constant.entries.iter().map(|e| e.name.clone()).collect();
            nodes.insert(
                constant.name.clone(),
                DepNode {
                    id: constant.name.clone(),
                    group: group.name.clone(),
                    kind: "constant".into(),
                    rows: constant.entries.len(),
                    fields: entry_names.len(),
                    layer: None,
                    role: String::new(),
                    owner: None,
                    rules: Vec::new(),
                    field_names: entry_names,
                    incoming: 0,
                    outgoing: 0,
                },
            );
            for entry in &constant.entries {
                for target in ref_targets(&entry.tbl_type) {
                    raw_edges.push(RawEdge {
                        from: constant.name.clone(),
                        to: target,
                        label: entry.name.clone(),
                    });
                }
            }
        }

        for enum_def in &group.enums {
            if enum_def.deleted {
                continue;
            }
            let entry_names: Vec<String> = enum_def.entries.iter().map(|e| e.name.clone()).collect();
            nodes.insert(
                enum_def.name.clone(),
                DepNode {
                    id: enum_def.name.clone(),
                    group: group.name.clone(),
                    kind: "enum".into(),
                    rows: enum_def.entries.len(),
                    fields: entry_names.len(),
                    layer: None,
                    role: String::new(),
                    owner: None,
                    rules: Vec::new(),
                    field_names: entry_names,
                    incoming: 0,
                    outgoing: 0,
                },
            );
        }
    }

    // 引用目标不存在时补一个幽灵节点，让断引用在图上仍有落点。
    let missing: BTreeSet<String> = raw_edges
        .iter()
        .filter(|e| !nodes.contains_key(&e.to))
        .map(|e| e.to.clone())
        .collect();
    for name in &missing {
        nodes.insert(
            name.clone(),
            DepNode {
                id: name.clone(),
                group: String::new(),
                kind: "missing".into(),
                rows: 0,
                fields: 0,
                layer: None,
                role: String::new(),
                owner: None,
                rules: Vec::new(),
                field_names: Vec::new(),
                incoming: 0,
                outgoing: 0,
            },
        );
    }

    // 同一对 (from,to) 的多个字段合并成一条边，详情里列出所有字段。
    let mut grouped: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    for edge in &raw_edges {
        grouped
            .entry((edge.from.clone(), edge.to.clone()))
            .or_default()
            .insert(edge.label.clone());
    }
    let mut edges: Vec<DepEdge> = grouped
        .into_iter()
        .map(|((from, to), labels)| {
            let broken = missing.contains(&to);
            DepEdge {
                from,
                to,
                label: labels.into_iter().collect::<Vec<_>>().join(", "),
                kind: "field".into(),
                broken,
            }
        })
        .collect();

    let mut out_map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut in_map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for edge in &edges {
        out_map.entry(edge.from.clone()).or_default().push(edge.to.clone());
        in_map.entry(edge.to.clone()).or_default().push(edge.from.clone());
    }
    for values in out_map.values_mut() {
        values.sort();
        values.dedup();
    }
    for values in in_map.values_mut() {
        values.sort();
        values.dedup();
    }

    // 最长依赖链层级；环上的节点会保持 0，最后单独标记。
    let names: Vec<String> = nodes.keys().cloned().collect();
    let mut layers: BTreeMap<String, u32> = names
        .iter()
        .map(|name| {
            let leaf = out_map.get(name).map_or(true, |v| v.is_empty());
            (name.clone(), if leaf { 1 } else { 0 })
        })
        .collect();
    for _ in 0..=names.len() {
        let mut changed = false;
        for name in &names {
            if layers[name] != 0 {
                continue;
            }
            let Some(targets) = out_map.get(name) else {
                layers.insert(name.clone(), 1);
                changed = true;
                continue;
            };
            if targets.iter().all(|target| layers.get(target).copied().unwrap_or(0) > 0) {
                let max = targets
                    .iter()
                    .map(|target| layers.get(target).copied().unwrap_or(1))
                    .max()
                    .unwrap_or(0);
                layers.insert(name.clone(), max + 1);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let cycle_nodes: BTreeSet<String> = names
        .iter()
        .filter(|name| layers.get(*name).copied().unwrap_or(0) == 0)
        .cloned()
        .collect();
    for name in &cycle_nodes {
        layers.insert(name.clone(), 1);
    }

    for node in nodes.values_mut() {
        node.incoming = in_map.get(&node.id).map_or(0, |v| v.len());
        node.outgoing = out_map.get(&node.id).map_or(0, |v| v.len());
        node.layer = Some(layers.get(&node.id).copied().unwrap_or(1));
    }

    // 先定层级，再定桥接 / 附属 / 根节点角色。
    let target_layers: BTreeMap<String, (u32, u32)> = out_map
        .iter()
        .map(|(from, targets)| {
            let min = targets
                .iter()
                .map(|t| layers.get(t).copied().unwrap_or(1))
                .min()
                .unwrap_or(1);
            let max = targets
                .iter()
                .map(|t| layers.get(t).copied().unwrap_or(1))
                .max()
                .unwrap_or(1);
            (from.clone(), (min, max))
        })
        .collect();

    let node_groups: BTreeMap<String, String> = nodes
        .iter()
        .map(|(name, node)| (name.clone(), node.group.clone()))
        .collect();
    for node in nodes.values_mut() {
        if node.kind == "missing" {
            node.role = "missing".into();
            continue;
        }
        if cycle_nodes.contains(&node.id) {
            node.role = "cycle".into();
            continue;
        }
        if node.kind == "constant" {
            node.role = "constant".into();
            continue;
        }
        if node.incoming == 0 && node.outgoing == 0 {
            node.role = "orphan".into();
            continue;
        }
        let span = target_layers.get(&node.id).copied().unwrap_or((1, 1));
        let bridge = node.kind == "table" && node.outgoing >= 2 && span.0 != span.1;
        if bridge {
            node.role = "bridge".into();
            continue;
        }
        if node.kind == "table" && node.incoming == 1 && node.outgoing == 0 {
            let owner = in_map.get(&node.id).and_then(|v| v.first()).cloned();
            // 同组 + 单一消费者 + 无出边，才作为附属候选。
            // 跨组单消费者更可能是共享基表（例如 item/Equip 被 hero/HeroBase 引用），
            // 不自动挂到引用方旁边；后续可用显式标记覆盖。
            let same_group = owner
                .as_ref()
                .and_then(|name| node_groups.get(name))
                .is_some_and(|group| group == &node.group);
            if same_group {
                node.role = "satellite".into();
                node.owner = owner;
                continue;
            }
        }
        if node.incoming == 0 && node.outgoing > 0 {
            node.role = "root".into();
            continue;
        }
        node.role = if node.layer == Some(1) { "base".into() } else { "business".into() };
    }

    // 边按来源/目标角色补上桥接、附属、断引用标记。
    for edge in &mut edges {
        if edge.broken {
            edge.kind = "broken".into();
        } else if nodes.get(&edge.from).is_some_and(|n| n.role == "bridge") {
            edge.kind = "bridge".into();
        } else if nodes.get(&edge.to).is_some_and(|n| {
            n.role == "satellite" && n.owner.as_deref() == Some(edge.from.as_str())
        }) {
            edge.kind = "attach".into();
        }
    }

    let mut node_list: Vec<DepNode> = nodes.into_values().collect();
    node_list.sort_by(|a, b| {
        a.group
            .cmp(&b.group)
            .then(a.layer.cmp(&b.layer))
            .then(a.id.cmp(&b.id))
    });
    edges.sort_by(|a, b| a.from.cmp(&b.from).then(a.to.cmp(&b.to)));

    let unconnected = node_list.iter().filter(|n| n.role == "orphan").count();
    let broken = edges.iter().filter(|e| e.broken).count();
    let roots = node_list.iter().filter(|n| n.role == "root").count();
    let max_layer = node_list.iter().filter_map(|n| n.layer).max().unwrap_or(0) as usize;

    let project_name = if project.schema.meta.name.is_empty() {
        project.schema.meta.id.clone()
    } else {
        project.schema.meta.name.clone()
    };
    DepGraph {
        project_id: project.schema.meta.id.clone(),
        project_name,
        summary: DepSummary {
            nodes: node_list.len(),
            edges: edges.len(),
            unconnected,
            broken,
            cycles: cycle_nodes.len(),
            layers: max_layer,
            roots,
        },
        nodes: node_list,
        edges,
    }
}

fn ref_targets(tbl_type: &str) -> Vec<String> {
    TblType::parse(tbl_type)
        .map(|ty| ty.ref_targets().into_iter().map(str::to_string).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        Constant, ConstEntry, EnumDef, EnumEntry, Export, FieldDef, Group, GroupStorage, Project,
        ProjectConfig, ProjectState, Table, TableSchema,
    };
    use crate::schema_model::TblSchema;
    use crate::tbl::DEFAULT_TBL_VERSION;
    use std::path::PathBuf;

    fn field(name: &str, ty: &str) -> FieldDef {
        FieldDef {
            name: name.into(),
            desc: String::new(),
            tbl_type: ty.into(),
            export: Export::ClientServer,
            aux: None,
            source_col: None,
        }
    }

    fn table(name: &str, fields: Vec<FieldDef>) -> Table {
        Table {
            name: name.into(),
            original_name: name.into(),
            path: PathBuf::from(format!("{name}.tbl")),
            schema: TableSchema { fields: fields.clone() },
            records: vec![vec![String::new(); fields.len()]],
            dirty: false,
            deleted: false,
            original_records: Vec::new(),
            original_fields: Vec::new(),
            saved: true,
            fmt_ver: DEFAULT_TBL_VERSION,
            display_field: None,
            original_display_field: None,
            rules: Vec::new(),
            original_rules: Vec::new(),
        }
    }

    fn project() -> Project {
        let mut schema = TblSchema::default();
        schema.meta.id = "dep".into();
        schema.meta.name = "依赖演示".into();
        Project {
            workdir: PathBuf::from("."),
            project_root: PathBuf::from("."),
            config: ProjectConfig::default(),
            schema,
            original_meta: Default::default(),
            original_separators: Default::default(),
            original_export: Default::default(),
            original_validate: Default::default(),
            groups: vec![Group {
                name: "hero".into(),
                original_name: "hero".into(),
                dir: PathBuf::from("hero"),
                tables: vec![
                    table("Hero", vec![field("id", "int"), field("base_id", "@HeroBase")]),
                    table(
                        "HeroBase",
                        vec![field("id", "int"), field("kind", "@HeroType"), field("equip", "@Equip")],
                    ),
                    table("Equip", vec![field("id", "int")]),
                    table("Skill", vec![field("id", "int"), field("equip", "@Equip")]),
                ],
                constants: vec![Constant {
                    name: "GameConst".into(),
                    original_name: "GameConst".into(),
                    path: PathBuf::new(),
                    entries: vec![ConstEntry {
                        name: "max_level".into(),
                        tbl_type: "int".into(),
                        value: "100".into(),
                        export: Export::ClientServer,
                        desc: String::new(),
                    }],
                    original_entries: Vec::new(),
                    dirty: false,
                    deleted: false,
                    saved: true,
                }],
                enums: vec![EnumDef {
                    name: "HeroType".into(),
                    original_name: "HeroType".into(),
                    path: PathBuf::new(),
                    entries: vec![EnumEntry { id: "1".into(), name: "WARRIOR".into(), desc: String::new() }],
                    original_entries: Vec::new(),
                    dirty: false,
                    deleted: false,
                    saved: true,
                    display_field: None,
                    original_display_field: None,
                    rules: Vec::new(),
                    original_rules: Vec::new(),
                }],
                is_new: false,
                storage: GroupStorage::Tbl,
                book_stamp: None,
            }],
            state: ProjectState::Pending,
            load_warnings: Vec::new(),
        }
    }

    #[test]
    fn computes_layers_roles_and_edges() {
        let g = analyze(&project());
        let role = |name: &str| g.nodes.iter().find(|n| n.id == name).map(|n| n.role.as_str());
        assert_eq!(role("Hero"), Some("root"));
        assert_eq!(role("HeroBase"), Some("business"));
        assert_eq!(role("HeroType"), Some("base"));
        assert_eq!(role("Equip"), Some("base"));
        assert_eq!(g.summary.layers, 3);
        assert!(g.edges.iter().any(|e| e.from == "HeroBase" && e.to == "HeroType" && e.label == "kind"));
    }

    #[test]
    fn marks_orphans_and_missing_targets() {
        let mut p = project();
        p.groups[0].tables.push(table("OldHero", vec![field("id", "int")]));
        p.groups[0].tables.push(table("QuestReward", vec![field("id", "int"), field("item_id", "@ItemBase")]));
        let g = analyze(&p);
        let role = |name: &str| g.nodes.iter().find(|n| n.id == name).map(|n| n.role.as_str());
        assert_eq!(role("OldHero"), Some("orphan"));
        assert_eq!(role("ItemBase"), Some("missing"));
        assert!(g.edges.iter().any(|e| e.broken && e.to == "ItemBase"));
        assert_eq!(g.summary.broken, 1);
    }

    #[test]
    fn marks_bridge_and_satellite() {
        let mut p = project();
        p.groups[0].tables.push(table(
            "HeroEquip",
            vec![field("hero_id", "@HeroBase"), field("equip_id", "@Equip")],
        ));
        p.groups[0].tables.push(table("HeroSkin", vec![field("id", "int")]));
        p.groups[0].tables[0]
            .schema
            .fields
            .push(field("skin", "@HeroSkin"));
        let g = analyze(&p);
        let node = |name: &str| g.nodes.iter().find(|n| n.id == name).unwrap();
        assert_eq!(node("HeroEquip").role, "bridge");
        assert_eq!(node("HeroSkin").role, "satellite");
        assert_eq!(node("HeroSkin").owner.as_deref(), Some("Hero"));
    }
}
