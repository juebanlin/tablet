use std::fmt::Write;
use anyhow::Result;
use crate::model::*;
use crate::types::*;
use super::LineEnding;

fn is_client_export(export: &Export) -> bool {
    matches!(export, Export::ClientServer | Export::ClientOnly)
}

fn lua_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            _ => out.push(c),
        }
    }
    out
}

fn is_lua_identifier(s: &str) -> bool {
    if s.is_empty() { return false; }
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_alphabetic() && first != '_' { return false; }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn base_to_lua(raw: &str, bt: BaseType) -> String {
    match bt {
        BaseType::Int | BaseType::Long => {
            raw.parse::<i64>().map(|v| v.to_string()).unwrap_or_else(|_| "0".to_string())
        }
        BaseType::Float | BaseType::Double => {
            raw.parse::<f64>().map(|v| {
                if v.fract() == 0.0 && !raw.contains('.') {
                    format!("{}.0", v as i64)
                } else {
                    raw.to_string()
                }
            }).unwrap_or_else(|_| "0".to_string())
        }
        BaseType::Bool => {
            if raw == "true" || raw == "1" { "true".to_string() } else { "false".to_string() }
        }
        BaseType::Str => format!("\"{}\"", lua_escape(raw)),
    }
}

fn lua_map_key(raw: &str, bt: BaseType) -> String {
    match bt {
        BaseType::Str => {
            if is_lua_identifier(raw) {
                raw.to_string()
            } else {
                format!("[\"{}\"]", lua_escape(raw))
            }
        }
        _ => format!("[{}]", base_to_lua(raw, bt)),
    }
}

fn value_to_lua(raw: &str, tbl_type_str: &str, sep: &SeparatorsSection) -> String {
    if raw.is_empty() { return "nil".to_string(); }

    let tt = match TblType::parse(tbl_type_str) {
        Some(t) => t,
        None => return format!("\"{}\"", lua_escape(raw)),
    };

    let p = &tt.params;
    match &tt.paradigm {
        Paradigm::Base => base_to_lua(raw, p[0]),

        Paradigm::Tuple2 => {
            let parts: Vec<&str> = raw.splitn(2, &*sep.tuple2).collect();
            if parts.len() == 2 {
                format!("{{{}, {}}}", base_to_lua(parts[0], p[0]), base_to_lua(parts[1], p[1]))
            } else { format!("\"{}\"", lua_escape(raw)) }
        }
        Paradigm::Tuple3 => {
            let parts: Vec<&str> = raw.splitn(3, &*sep.tuple3).collect();
            if parts.len() == 3 {
                format!("{{{}, {}, {}}}", base_to_lua(parts[0], p[0]), base_to_lua(parts[1], p[1]), base_to_lua(parts[2], p[2]))
            } else { format!("\"{}\"", lua_escape(raw)) }
        }
        Paradigm::Tuple4 => {
            let parts: Vec<&str> = raw.splitn(4, &*sep.tuple4).collect();
            if parts.len() == 4 {
                format!("{{{}, {}, {}, {}}}", base_to_lua(parts[0], p[0]), base_to_lua(parts[1], p[1]), base_to_lua(parts[2], p[2]), base_to_lua(parts[3], p[3]))
            } else { format!("\"{}\"", lua_escape(raw)) }
        }

        Paradigm::List => {
            let items: Vec<String> = raw.split(&*sep.list).map(|v| base_to_lua(v, p[0])).collect();
            format!("{{{}}}", items.join(", "))
        }
        Paradigm::Set => {
            let items: Vec<String> = raw.split(&*sep.set).map(|v| format!("[{}]=true", base_to_lua(v, p[0]))).collect();
            format!("{{{}}}", items.join(", "))
        }
        Paradigm::Map => {
            let entries: Vec<String> = raw.split(&*sep.map.entry).map(|entry| {
                let kv: Vec<&str> = entry.splitn(2, &*sep.map.kv).collect();
                if kv.len() == 2 {
                    let k = lua_map_key(kv[0], p[0]);
                    let v = base_to_lua(kv[1], p[1]);
                    if k.starts_with('[') { format!("{}={}", k, v) } else { format!("{}={}", k, v) }
                } else { format!("\"{}\"", lua_escape(entry)) }
            }).collect();
            format!("{{{}}}", entries.join(", "))
        }

        Paradigm::ListTuple2 => {
            let items: Vec<String> = raw.split(&*sep.list_tuple2.list).map(|item| {
                let parts: Vec<&str> = item.splitn(2, &*sep.list_tuple2.tuple).collect();
                if parts.len() == 2 {
                    format!("{{{}, {}}}", base_to_lua(parts[0], p[0]), base_to_lua(parts[1], p[1]))
                } else { format!("\"{}\"", lua_escape(item)) }
            }).collect();
            format!("{{{}}}", items.join(", "))
        }
        Paradigm::ListTuple3 => {
            let items: Vec<String> = raw.split(&*sep.list_tuple3.list).map(|item| {
                let parts: Vec<&str> = item.splitn(3, &*sep.list_tuple3.tuple).collect();
                if parts.len() == 3 {
                    format!("{{{}, {}, {}}}", base_to_lua(parts[0], p[0]), base_to_lua(parts[1], p[1]), base_to_lua(parts[2], p[2]))
                } else { format!("\"{}\"", lua_escape(item)) }
            }).collect();
            format!("{{{}}}", items.join(", "))
        }
        Paradigm::ListTuple4 => {
            let items: Vec<String> = raw.split(&*sep.list_tuple4.list).map(|item| {
                let parts: Vec<&str> = item.splitn(4, &*sep.list_tuple4.tuple).collect();
                if parts.len() == 4 {
                    format!("{{{}, {}, {}, {}}}", base_to_lua(parts[0], p[0]), base_to_lua(parts[1], p[1]), base_to_lua(parts[2], p[2]), base_to_lua(parts[3], p[3]))
                } else { format!("\"{}\"", lua_escape(item)) }
            }).collect();
            format!("{{{}}}", items.join(", "))
        }

        Paradigm::MapTuple2 => {
            let entries: Vec<String> = raw.split(&*sep.map_tuple2.entry).map(|entry| {
                let kv: Vec<&str> = entry.splitn(2, &*sep.map_tuple2.kv).collect();
                if kv.len() == 2 {
                    let k = lua_map_key(kv[0], p[0]);
                    let parts: Vec<&str> = kv[1].splitn(2, &*sep.map_tuple2.tuple).collect();
                    let v = if parts.len() == 2 {
                        format!("{{{}, {}}}", base_to_lua(parts[0], p[1]), base_to_lua(parts[1], p[2]))
                    } else { format!("\"{}\"", lua_escape(kv[1])) };
                    format!("{}={}", k, v)
                } else { format!("\"{}\"", lua_escape(entry)) }
            }).collect();
            format!("{{{}}}", entries.join(", "))
        }
        Paradigm::MapTuple3 => {
            let entries: Vec<String> = raw.split(&*sep.map_tuple3.entry).map(|entry| {
                let kv: Vec<&str> = entry.splitn(2, &*sep.map_tuple3.kv).collect();
                if kv.len() == 2 {
                    let k = lua_map_key(kv[0], p[0]);
                    let parts: Vec<&str> = kv[1].splitn(3, &*sep.map_tuple3.tuple).collect();
                    let v = if parts.len() == 3 {
                        format!("{{{}, {}, {}}}", base_to_lua(parts[0], p[1]), base_to_lua(parts[1], p[2]), base_to_lua(parts[2], p[3]))
                    } else { format!("\"{}\"", lua_escape(kv[1])) };
                    format!("{}={}", k, v)
                } else { format!("\"{}\"", lua_escape(entry)) }
            }).collect();
            format!("{{{}}}", entries.join(", "))
        }
        Paradigm::MapTuple4 => {
            let entries: Vec<String> = raw.split(&*sep.map_tuple4.entry).map(|entry| {
                let kv: Vec<&str> = entry.splitn(2, &*sep.map_tuple4.kv).collect();
                if kv.len() == 2 {
                    let k = lua_map_key(kv[0], p[0]);
                    let parts: Vec<&str> = kv[1].splitn(4, &*sep.map_tuple4.tuple).collect();
                    let v = if parts.len() == 4 {
                        format!("{{{}, {}, {}, {}}}", base_to_lua(parts[0], p[1]), base_to_lua(parts[1], p[2]), base_to_lua(parts[2], p[3]), base_to_lua(parts[3], p[4]))
                    } else { format!("\"{}\"", lua_escape(kv[1])) };
                    format!("{}={}", k, v)
                } else { format!("\"{}\"", lua_escape(entry)) }
            }).collect();
            format!("{{{}}}", entries.join(", "))
        }
        Paradigm::MapList => {
            let entries: Vec<String> = raw.split(&*sep.map_list.entry).map(|entry| {
                let kv: Vec<&str> = entry.splitn(2, &*sep.map_list.kv).collect();
                if kv.len() == 2 {
                    let k = lua_map_key(kv[0], p[0]);
                    let items: Vec<String> = kv[1].split(&*sep.map_list.item).map(|v| base_to_lua(v, p[1])).collect();
                    format!("{}={{{}}}", k, items.join(", "))
                } else { format!("\"{}\"", lua_escape(entry)) }
            }).collect();
            format!("{{{}}}", entries.join(", "))
        }

        // 引用类型：lua 始终输出 id（数字字面量）
        Paradigm::Ref => base_to_lua(raw, BaseType::Int),
    }
}

pub fn export_table_lua(table: &Table, sep: &SeparatorsSection) -> String {
    let fields = &table.schema.fields;
    let export_cols: Vec<(usize, &FieldDef)> = fields.iter().enumerate()
        .filter(|(_, f)| is_client_export(&f.export))
        .collect();

    let index_col = fields.iter().position(|f| f.name == "id").unwrap_or(0);

    let mut s = String::new();
    writeln!(s, "local {} = {{", table.name).unwrap();

    for record in &table.records {
        let index_raw = record.get(index_col).map(|v| v.as_str()).unwrap_or("0");
        let index_val = index_raw.parse::<i64>().unwrap_or(0);

        let mut fields_str = String::new();
        for &(col, field) in &export_cols {
            let raw = record.get(col).map(|v| v.as_str()).unwrap_or("");
            if raw.is_empty() { continue; }
            let val = value_to_lua(raw, &field.tbl_type, sep);
            if !fields_str.is_empty() { fields_str.push_str(", "); }
            write!(fields_str, "{}={}", field.name, val).unwrap();
        }

        writeln!(s, "    [{}] = {{{}}},", index_val, fields_str).unwrap();
    }

    writeln!(s, "}}").unwrap();
    writeln!(s, "return {}", table.name).unwrap();
    s
}

pub fn export_constant_lua(constant: &Constant, sep: &SeparatorsSection) -> String {
    let mut s = String::new();
    writeln!(s, "local {} = {{", constant.name).unwrap();

    for entry in &constant.entries {
        if !is_client_export(&entry.export) { continue; }
        if entry.name.is_empty() { continue; }
        if entry.value.is_empty() { continue; }

        let val = value_to_lua(&entry.value, &entry.tbl_type, sep);
        writeln!(s, "    {} = {},", entry.name, val).unwrap();
    }

    writeln!(s, "}}").unwrap();
    writeln!(s, "return {}", constant.name).unwrap();
    s
}

pub fn export_all_lua(project: &Project) -> Result<super::ExportResult> {
    let export_cfg = project.config.export.as_ref();
    let client = export_cfg.and_then(|e| e.client.as_ref());
    let lua = client.and_then(|c| c.lua.as_ref());

    let output = lua
        .and_then(|l| l.output.as_deref())
        .unwrap_or("gen/client");

    let line_ending = LineEnding::from_config(
        lua.and_then(|l| l.line_ending.as_deref())
            .or_else(|| client.and_then(|c| c.line_ending.as_deref()))
            .or_else(|| export_cfg.and_then(|e| e.line_ending.as_deref()))
            .unwrap_or("lf")
    );
    let encoding = lua.and_then(|l| l.encoding.as_deref())
        .or_else(|| client.and_then(|c| c.encoding.as_deref()))
        .or_else(|| export_cfg.and_then(|e| e.encoding.as_deref()))
        .unwrap_or("utf-8").to_string();
    let opts = super::ExportOptions { line_ending, encoding };

    let sep = &project.config.separators;
    let output_dir = project.workdir.join(output);
    let mut collected = Vec::new();

    for group in &project.groups {
        for table in &group.tables {
            if table.deleted { continue; }
            let lua = export_table_lua(table, sep);
            let file_path = output_dir.join(format!("{}.lua", &table.name));
            collected.push((file_path, opts.encode(&lua)));
        }

        for constant in &group.constants {
            if constant.deleted { continue; }
            let lua = export_constant_lua(constant, sep);
            let file_path = output_dir.join(format!("{}.lua", &constant.name));
            collected.push((file_path, opts.encode(&lua)));
        }
    }

    super::sync_export_dir(&output_dir, "lua", collected)
}
