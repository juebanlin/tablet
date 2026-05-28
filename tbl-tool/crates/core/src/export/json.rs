use anyhow::Result;
use serde_json::{Value, Map};
use crate::model::*;
use crate::types::*;
use super::{EmptyStrategy, LineEnding, to_camel_case, parse_base_value};

pub fn value_to_json(raw: &str, tbl_type: &TblType, sep: &SeparatorsSection) -> Value {
    match tbl_type.paradigm {
        Paradigm::Base => parse_base_value(raw, &tbl_type.params[0]),
        Paradigm::Tuple2 => parse_tuple(raw, &tbl_type.params, &sep.tuple2),
        Paradigm::Tuple3 => parse_tuple(raw, &tbl_type.params, &sep.tuple3),
        Paradigm::Tuple4 => parse_tuple(raw, &tbl_type.params, &sep.tuple4),
        Paradigm::List => parse_list(raw, &tbl_type.params[0], &sep.list),
        Paradigm::Set => parse_list(raw, &tbl_type.params[0], &sep.set),
        Paradigm::Map => parse_map(raw, &tbl_type.params[0], &tbl_type.params[1], &sep.map.kv, &sep.map.entry),
        Paradigm::ListTuple2 => parse_list_tuple(raw, &tbl_type.params, &sep.list_tuple2.tuple, &sep.list_tuple2.list),
        Paradigm::ListTuple3 => parse_list_tuple(raw, &tbl_type.params, &sep.list_tuple3.tuple, &sep.list_tuple3.list),
        Paradigm::ListTuple4 => parse_list_tuple(raw, &tbl_type.params, &sep.list_tuple4.tuple, &sep.list_tuple4.list),
        Paradigm::MapTuple2 => parse_map_tuple(raw, &tbl_type.params, &sep.map_tuple2.kv, &sep.map_tuple2.tuple, &sep.map_tuple2.entry),
        Paradigm::MapTuple3 => parse_map_tuple(raw, &tbl_type.params, &sep.map_tuple3.kv, &sep.map_tuple3.tuple, &sep.map_tuple3.entry),
        Paradigm::MapTuple4 => parse_map_tuple(raw, &tbl_type.params, &sep.map_tuple4.kv, &sep.map_tuple4.tuple, &sep.map_tuple4.entry),
        Paradigm::MapList => parse_map_list(raw, &tbl_type.params[0], &tbl_type.params[1], &sep.map_list.kv, &sep.map_list.item, &sep.map_list.entry),
    }
}

fn parse_tuple(raw: &str, params: &[BaseType], sep: &str) -> Value {
    let parts: Vec<&str> = raw.split(sep).collect();
    let arr: Vec<Value> = params.iter().enumerate().map(|(i, bt)| {
        let v = parts.get(i).map(|s| s.trim()).unwrap_or("");
        parse_base_value(v, bt)
    }).collect();
    Value::Array(arr)
}

fn parse_list(raw: &str, elem: &BaseType, sep: &str) -> Value {
    let arr: Vec<Value> = raw.split(sep)
        .map(|s| parse_base_value(s.trim(), elem))
        .collect();
    Value::Array(arr)
}

fn parse_map(raw: &str, _key_type: &BaseType, val_type: &BaseType, kv_sep: &str, entry_sep: &str) -> Value {
    let mut map = Map::new();
    for entry in raw.split(entry_sep) {
        let entry = entry.trim();
        if entry.is_empty() { continue; }
        if let Some((k, v)) = entry.split_once(kv_sep) {
            let key = k.trim().to_string();
            let val = parse_base_value(v.trim(), val_type);
            map.insert(key, val);
        }
    }
    Value::Object(map)
}

fn parse_list_tuple(raw: &str, params: &[BaseType], tuple_sep: &str, list_sep: &str) -> Value {
    let arr: Vec<Value> = raw.split(list_sep).filter(|s| !s.trim().is_empty()).map(|item| {
        parse_tuple(item.trim(), params, tuple_sep)
    }).collect();
    Value::Array(arr)
}

fn parse_map_tuple(raw: &str, params: &[BaseType], kv_sep: &str, tuple_sep: &str, entry_sep: &str) -> Value {
    let _key_type = &params[0];
    let val_params = &params[1..];
    let mut map = Map::new();
    for entry in raw.split(entry_sep) {
        let entry = entry.trim();
        if entry.is_empty() { continue; }
        if let Some((k, v)) = entry.split_once(kv_sep) {
            let key = k.trim().to_string();
            let val = parse_tuple(v.trim(), val_params, tuple_sep);
            map.insert(key, val);
        }
    }
    Value::Object(map)
}

fn parse_map_list(raw: &str, _key_type: &BaseType, elem_type: &BaseType, kv_sep: &str, item_sep: &str, entry_sep: &str) -> Value {
    let mut map = Map::new();
    for entry in raw.split(entry_sep) {
        let entry = entry.trim();
        if entry.is_empty() { continue; }
        if let Some((k, v)) = entry.split_once(kv_sep) {
            let key = k.trim().to_string();
            let arr: Vec<Value> = v.split(item_sep)
                .map(|s| parse_base_value(s.trim(), elem_type))
                .collect();
            map.insert(key, Value::Array(arr));
        }
    }
    Value::Object(map)
}


fn is_server_export(export: &Export) -> bool {
    matches!(export, Export::ClientServer | Export::ServerOnly)
}

pub fn export_table(table: &Table, sep: &SeparatorsSection, strategy: &EmptyStrategy) -> Value {
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
                }
            } else if let Some(t) = &tbl_type {
                obj.insert(key, value_to_json(raw, t, sep));
            } else {
                obj.insert(key, Value::from(raw));
            }
        }
        Value::Object(obj)
    }).collect();

    Value::Array(rows)
}

pub fn export_constant(constant: &Constant, sep: &SeparatorsSection, strategy: &EmptyStrategy) -> Value {
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
            }
        } else if let Some(t) = &tbl_type {
            obj.insert(key, value_to_json(&entry.value, t, sep));
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
    let strategy = EmptyStrategy::from_config(strategy_str);

    let line_ending = LineEnding::from_config(
        export_cfg.and_then(|e| e.line_ending.as_deref()).unwrap_or("lf")
    );

    let sep = &project.config.separators;
    let output_dir = project.workdir.join(data_output);
    let mut generated = Vec::new();

    for group in &project.groups {
        let group_dir = output_dir.join(&group.name);

        for table in &group.tables {
            if table.deleted { continue; }
            let json = export_table(table, sep, &strategy);
            let file_path = group_dir.join(format!("{}.json", &table.name));
            std::fs::create_dir_all(file_path.parent().unwrap())?;
            let content = serde_json::to_string_pretty(&json)?;
            let content = line_ending.normalize(&content);
            std::fs::write(&file_path, content.as_bytes())?;
            generated.push(file_path.display().to_string());
        }

        for constant in &group.constants {
            if constant.deleted { continue; }
            let json = export_constant(constant, sep, &strategy);
            let file_path = group_dir.join(format!("{}.json", &constant.name));
            std::fs::create_dir_all(file_path.parent().unwrap())?;
            let content = serde_json::to_string_pretty(&json)?;
            let content = line_ending.normalize(&content);
            std::fs::write(&file_path, content.as_bytes())?;
            generated.push(file_path.display().to_string());
        }
    }

    Ok(generated)
}
