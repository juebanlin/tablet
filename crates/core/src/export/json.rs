use anyhow::Result;
use serde_json::{Value, Map, json};
use crate::model::*;
use crate::types::*;
use super::{EmptyStrategy, LineEnding, to_camel_case, parse_base_value};
use super::sep_meta::{collect_used_sep_keys_constant, collect_used_sep_keys_table};

fn value_to_json(raw: &str, tbl_type: &TblType) -> Value {
    match tbl_type.paradigm {
        Paradigm::Base => parse_base_value(raw, &tbl_type.params[0]),
        // 引用类型：数据层永远是 int(id)
        Paradigm::Ref => parse_base_value(raw, &BaseType::Int),
        _ => Value::from(raw),
    }
}

fn is_server_export(export: &Export) -> bool {
    matches!(export, Export::ClientServer | Export::ServerOnly)
}

/// 按 `used_keys` 裁剪输出 _sep 对象。空集返回 None —— 调用方据此决定是否插入 _sep wrapper。
fn build_sep_meta(sep: &SeparatorsSection, used_keys: &std::collections::BTreeSet<SepKey>) -> Option<Value> {
    if used_keys.is_empty() {
        return None;
    }
    let mut obj = Map::new();
    for k in SepKey::ALL {
        if used_keys.contains(&k) {
            obj.insert(k.as_export_key().to_string(), json!(k.get(sep)));
        }
    }
    Some(Value::Object(obj))
}

pub fn export_table(table: &Table, strategy: &EmptyStrategy) -> Value {
    let fields = &table.schema.fields;
    let export_cols: Vec<(usize, &FieldDef)> = fields.iter().enumerate()
        .filter(|(_, f)| is_server_export(&f.export))
        .collect();

    let rows: Vec<Value> = table.records.iter().map(|record| {
        let mut obj = Map::new();
        for &(col, field) in &export_cols {
            let raw = record.get(col).map(|s| s.as_str()).unwrap_or("");
            let key = to_camel_case(&field.name);
            let tbl_type = TblType::parse(&field.tbl_type);

            if raw.is_empty() {
                match strategy {
                    EmptyStrategy::Omit => continue,
                    EmptyStrategy::Null => { obj.insert(key, Value::Null); }
                    EmptyStrategy::Empty => { obj.insert(key, Value::from("")); }
                }
            } else if let Some(t) = &tbl_type {
                obj.insert(key, value_to_json(raw, t));
            } else {
                obj.insert(key, Value::from(raw));
            }
        }
        Value::Object(obj)
    }).collect();

    Value::Array(rows)
}

pub fn export_constant(constant: &Constant, strategy: &EmptyStrategy) -> Value {
    let mut obj = Map::new();
    for entry in &constant.entries {
        if !is_server_export(&entry.export) { continue; }
        if entry.name.is_empty() { continue; }

        let key = to_camel_case(&entry.name);
        let tbl_type = TblType::parse(&entry.tbl_type);

        if entry.value.is_empty() {
            match strategy {
                EmptyStrategy::Omit => continue,
                EmptyStrategy::Null => { obj.insert(key, Value::Null); }
                EmptyStrategy::Empty => { obj.insert(key, Value::from("")); }
            }
        } else if let Some(t) = &tbl_type {
            obj.insert(key, value_to_json(&entry.value, t));
        } else {
            obj.insert(key, Value::from(entry.value.as_str()));
        }
    }
    Value::Object(obj)
}


pub fn export_all_json(project: &Project) -> Result<super::ExportResult> {
    let export_cfg = project.config.export.as_ref();

    let data_output = export_cfg
        .and_then(|e| e.server.as_ref())
        .and_then(|s| s.data_output.as_deref())
        .unwrap_or("gen/server/data");

    let strategy_str = export_cfg
        .and_then(|e| e.json.as_ref())
        .and_then(|j| j.empty_as.map(|e| e.as_str()))
        .unwrap_or("null");
    let strategy = EmptyStrategy::from_json_config(strategy_str);

    let line_ending = LineEnding::from_config(
        export_cfg.and_then(|e| e.json.as_ref()).and_then(|j| j.line_ending.map(|l| l.as_str()))
            .or_else(|| export_cfg.and_then(|e| e.line_ending.map(|l| l.as_str())))
            .unwrap_or("lf")
    );
    let encoding = export_cfg.and_then(|e| e.json.as_ref()).and_then(|j| j.encoding.map(|e| e.as_str()))
        .or_else(|| export_cfg.and_then(|e| e.encoding.map(|e| e.as_str())))
        .unwrap_or("utf-8").to_string();
    let opts = super::ExportOptions { line_ending, encoding };

    let sep = &project.config.separators;
    let output_dir = project.export_root().join(data_output).join("json");
    let mut collected = Vec::new();

    for group in &project.groups {
        for table in &group.tables {
            if table.deleted { continue; }
            let data = export_table(table, &strategy);
            let used = collect_used_sep_keys_table(table);
            let mut wrapper = Map::new();
            if let Some(meta) = build_sep_meta(sep, &used) {
                wrapper.insert("_sep".to_string(), meta);
            }
            wrapper.insert("data".to_string(), data);
            let file_path = output_dir.join(format!("{}.json", &table.name));
            let content = serde_json::to_string_pretty(&Value::Object(wrapper))?;
            collected.push((file_path, opts.encode(&content)));
        }

        for constant in &group.constants {
            if constant.deleted { continue; }
            let data = export_constant(constant, &strategy);
            let used = collect_used_sep_keys_constant(constant);
            let mut wrapper = Map::new();
            if let Some(meta) = build_sep_meta(sep, &used) {
                wrapper.insert("_sep".to_string(), meta);
            }
            wrapper.insert("data".to_string(), data);
            let file_path = output_dir.join(format!("{}.json", &constant.name));
            let content = serde_json::to_string_pretty(&Value::Object(wrapper))?;
            collected.push((file_path, opts.encode(&content)));
        }
    }

    super::sync_export_dir(&output_dir, "json", collected)
}

pub fn export_json_filtered(project: &Project, filter: &super::DataFilter) -> Result<super::ExportResult> {
    let export_cfg = project.config.export.as_ref();

    let data_output = export_cfg
        .and_then(|e| e.server.as_ref())
        .and_then(|s| s.data_output.as_deref())
        .unwrap_or("gen/server/data");

    let strategy_str = export_cfg
        .and_then(|e| e.json.as_ref())
        .and_then(|j| j.empty_as.map(|e| e.as_str()))
        .unwrap_or("null");
    let strategy = EmptyStrategy::from_json_config(strategy_str);

    let line_ending = LineEnding::from_config(
        export_cfg.and_then(|e| e.json.as_ref()).and_then(|j| j.line_ending.map(|l| l.as_str()))
            .or_else(|| export_cfg.and_then(|e| e.line_ending.map(|l| l.as_str())))
            .unwrap_or("lf")
    );
    let encoding = export_cfg.and_then(|e| e.json.as_ref()).and_then(|j| j.encoding.map(|e| e.as_str()))
        .or_else(|| export_cfg.and_then(|e| e.encoding.map(|e| e.as_str())))
        .unwrap_or("utf-8").to_string();
    let opts = super::ExportOptions { line_ending, encoding };

    let sep = &project.config.separators;
    let output_dir = project.export_root().join(data_output).join("json");
    let mut collected = Vec::new();

    for group in &project.groups {
        if !filter.matches_group(&group.name) { continue; }
        for table in &group.tables {
            if table.deleted { continue; }
            if !filter.matches_node(&table.name) { continue; }
            let data = export_table(table, &strategy);
            let used = collect_used_sep_keys_table(table);
            let mut wrapper = Map::new();
            if let Some(meta) = build_sep_meta(sep, &used) {
                wrapper.insert("_sep".to_string(), meta);
            }
            wrapper.insert("data".to_string(), data);
            let file_path = output_dir.join(format!("{}.json", &table.name));
            let content = serde_json::to_string_pretty(&Value::Object(wrapper))?;
            collected.push((file_path, opts.encode(&content)));
        }

        for constant in &group.constants {
            if constant.deleted { continue; }
            if !filter.matches_node(&constant.name) { continue; }
            let data = export_constant(constant, &strategy);
            let used = collect_used_sep_keys_constant(constant);
            let mut wrapper = Map::new();
            if let Some(meta) = build_sep_meta(sep, &used) {
                wrapper.insert("_sep".to_string(), meta);
            }
            wrapper.insert("data".to_string(), data);
            let file_path = output_dir.join(format!("{}.json", &constant.name));
            let content = serde_json::to_string_pretty(&Value::Object(wrapper))?;
            collected.push((file_path, opts.encode(&content)));
        }
    }

    super::sync_export_dir(&output_dir, "json", collected)
}
