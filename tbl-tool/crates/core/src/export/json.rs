use anyhow::Result;
use serde_json::{Value, Map, json};
use crate::model::*;
use crate::types::*;
use super::{EmptyStrategy, LineEnding, to_camel_case, parse_base_value};

fn value_to_json(raw: &str, tbl_type: &TblType) -> Value {
    match tbl_type.paradigm {
        Paradigm::Base => parse_base_value(raw, &tbl_type.params[0]),
        _ => Value::from(raw),
    }
}

fn is_server_export(export: &Export) -> bool {
    matches!(export, Export::ClientServer | Export::ServerOnly)
}

fn build_sep_meta(sep: &SeparatorsSection) -> Value {
    json!({
        "list": sep.list,
        "set": sep.set,
        "tuple2": sep.tuple2,
        "tuple3": sep.tuple3,
        "tuple4": sep.tuple4,
        "map_kv": sep.map.kv,
        "map_entry": sep.map.entry,
        "list_tuple2_tuple": sep.list_tuple2.tuple,
        "list_tuple2_list": sep.list_tuple2.list,
        "list_tuple3_tuple": sep.list_tuple3.tuple,
        "list_tuple3_list": sep.list_tuple3.list,
        "list_tuple4_tuple": sep.list_tuple4.tuple,
        "list_tuple4_list": sep.list_tuple4.list,
        "map_tuple2_kv": sep.map_tuple2.kv,
        "map_tuple2_tuple": sep.map_tuple2.tuple,
        "map_tuple2_entry": sep.map_tuple2.entry,
        "map_tuple3_kv": sep.map_tuple3.kv,
        "map_tuple3_tuple": sep.map_tuple3.tuple,
        "map_tuple3_entry": sep.map_tuple3.entry,
        "map_tuple4_kv": sep.map_tuple4.kv,
        "map_tuple4_tuple": sep.map_tuple4.tuple,
        "map_tuple4_entry": sep.map_tuple4.entry,
        "map_list_kv": sep.map_list.kv,
        "map_list_item": sep.map_list.item,
        "map_list_entry": sep.map_list.entry
    })
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


pub fn export_all_json(project: &Project) -> Result<Vec<String>> {
    let export_cfg = project.config.export.as_ref();

    let data_output = export_cfg
        .and_then(|e| e.server.as_ref())
        .and_then(|s| s.data_output.as_deref())
        .unwrap_or("gen/server/data");

    let strategy_str = export_cfg
        .and_then(|e| e.json.as_ref())
        .and_then(|j| j.empty_as.as_deref())
        .unwrap_or("null");
    let strategy = EmptyStrategy::from_json_config(strategy_str);

    let line_ending = LineEnding::from_config(
        export_cfg.and_then(|e| e.json.as_ref()).and_then(|j| j.line_ending.as_deref())
            .or_else(|| export_cfg.and_then(|e| e.line_ending.as_deref()))
            .unwrap_or("lf")
    );
    let encoding = export_cfg.and_then(|e| e.json.as_ref()).and_then(|j| j.encoding.as_deref())
        .or_else(|| export_cfg.and_then(|e| e.encoding.as_deref()))
        .unwrap_or("utf-8").to_string();
    let opts = super::ExportOptions { line_ending, encoding };

    let sep_meta = build_sep_meta(&project.config.separators);
    let output_dir = project.workdir.join(data_output).join("json");
    let mut generated = Vec::new();

    for group in &project.groups {
        let group_dir = output_dir.join(&group.name);

        for table in &group.tables {
            if table.deleted { continue; }
            let data = export_table(table, &strategy);
            let mut wrapper = Map::new();
            wrapper.insert("_sep".to_string(), sep_meta.clone());
            wrapper.insert("data".to_string(), data);
            let file_path = group_dir.join(format!("{}.json", &table.name));
            let content = serde_json::to_string_pretty(&Value::Object(wrapper))?;
            opts.write_file(&file_path, &content)?;
            generated.push(file_path.display().to_string());
        }

        for constant in &group.constants {
            if constant.deleted { continue; }
            let data = export_constant(constant, &strategy);
            let mut wrapper = Map::new();
            wrapper.insert("_sep".to_string(), sep_meta.clone());
            wrapper.insert("data".to_string(), data);
            let file_path = group_dir.join(format!("{}.json", &constant.name));
            let content = serde_json::to_string_pretty(&Value::Object(wrapper))?;
            opts.write_file(&file_path, &content)?;
            generated.push(file_path.display().to_string());
        }
    }

    Ok(generated)
}
