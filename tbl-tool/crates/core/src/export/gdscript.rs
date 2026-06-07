//! GDScript 导出（内嵌模式）。
//!
//! 形态参考 lua.rs，每张表/常量/枚举一个 `.gd` 文件，文件内单 `const X = {...}`。
//! 用 lua-style identifier 短键（`name = "x"` 等价于 `"name": "x"`），非 identifier 字符串键
//! 走标准 `"k": v`。Set 在 GDScript 无原生类型，按 Array 输出。
//! 引用字段 (Ref) 在脚本语言始终保留 int id，不做枚举类型替换。
//!
//! 用户使用：`var heroes = preload("res://gen/client/gdscript/HeroBase.gd").HeroBase`

use std::fmt::Write;
use anyhow::Result;
use crate::model::*;
use crate::types::*;
use super::LineEnding;

fn is_client_export(export: &Export) -> bool {
    matches!(export, Export::ClientServer | Export::ClientOnly)
}

fn gd_escape(s: &str) -> String {
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

fn is_gd_identifier(s: &str) -> bool {
    if s.is_empty() { return false; }
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_alphabetic() && first != '_' { return false; }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn base_to_gd(raw: &str, bt: BaseType) -> String {
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
            }).unwrap_or_else(|_| "0.0".to_string())
        }
        BaseType::Bool => {
            if raw == "true" || raw == "1" { "true".to_string() } else { "false".to_string() }
        }
        BaseType::Str => format!("\"{}\"", gd_escape(raw)),
    }
}

/// GDScript dict 键：identifier 风格的字符串走 lua-style (`key = value`)，
/// 其余走标准 `"key": value` / `1: value`。返回值不含尾随 `: ` 或 ` = `——
/// 由调用方按 lua-style 还是标准式决定。
enum GdKey {
    LuaStyle(String),  // `key`
    Standard(String),  // `"key"` or `1`
}

fn gd_map_key(raw: &str, bt: BaseType) -> GdKey {
    match bt {
        BaseType::Str => {
            if is_gd_identifier(raw) {
                GdKey::LuaStyle(raw.to_string())
            } else {
                GdKey::Standard(format!("\"{}\"", gd_escape(raw)))
            }
        }
        _ => GdKey::Standard(base_to_gd(raw, bt)),
    }
}

fn fmt_kv(k: GdKey, v: &str) -> String {
    match k {
        GdKey::LuaStyle(s) => format!("{} = {}", s, v),
        GdKey::Standard(s) => format!("{}: {}", s, v),
    }
}

fn value_to_gd(raw: &str, tbl_type_str: &str, sep: &SeparatorsSection) -> String {
    if raw.is_empty() { return "null".to_string(); }

    let tt = match TblType::parse(tbl_type_str) {
        Some(t) => t,
        None => return format!("\"{}\"", gd_escape(raw)),
    };

    let p = &tt.params;
    match &tt.paradigm {
        Paradigm::Base => base_to_gd(raw, p[0]),

        Paradigm::Tuple2 => {
            let parts: Vec<&str> = raw.splitn(2, &*sep.tuple2).collect();
            if parts.len() == 2 {
                format!("[{}, {}]", base_to_gd(parts[0], p[0]), base_to_gd(parts[1], p[1]))
            } else { format!("\"{}\"", gd_escape(raw)) }
        }
        Paradigm::Tuple3 => {
            let parts: Vec<&str> = raw.splitn(3, &*sep.tuple3).collect();
            if parts.len() == 3 {
                format!("[{}, {}, {}]", base_to_gd(parts[0], p[0]), base_to_gd(parts[1], p[1]), base_to_gd(parts[2], p[2]))
            } else { format!("\"{}\"", gd_escape(raw)) }
        }
        Paradigm::Tuple4 => {
            let parts: Vec<&str> = raw.splitn(4, &*sep.tuple4).collect();
            if parts.len() == 4 {
                format!("[{}, {}, {}, {}]", base_to_gd(parts[0], p[0]), base_to_gd(parts[1], p[1]), base_to_gd(parts[2], p[2]), base_to_gd(parts[3], p[3]))
            } else { format!("\"{}\"", gd_escape(raw)) }
        }

        Paradigm::List => {
            let items: Vec<String> = raw.split(&*sep.list).map(|v| base_to_gd(v, p[0])).collect();
            format!("[{}]", items.join(", "))
        }
        // GDScript 无 Set，回退 Array（值唯一性由数据保证）
        Paradigm::Set => {
            let items: Vec<String> = raw.split(&*sep.set).map(|v| base_to_gd(v, p[0])).collect();
            format!("[{}]", items.join(", "))
        }
        Paradigm::Map => {
            let entries: Vec<String> = raw.split(&*sep.map.entry).map(|entry| {
                let kv: Vec<&str> = entry.splitn(2, &*sep.map.kv).collect();
                if kv.len() == 2 {
                    fmt_kv(gd_map_key(kv[0], p[0]), &base_to_gd(kv[1], p[1]))
                } else { format!("\"{}\"", gd_escape(entry)) }
            }).collect();
            format!("{{{}}}", entries.join(", "))
        }

        Paradigm::ListTuple2 => {
            let items: Vec<String> = raw.split(&*sep.list_tuple2.list).map(|item| {
                let parts: Vec<&str> = item.splitn(2, &*sep.list_tuple2.tuple).collect();
                if parts.len() == 2 {
                    format!("[{}, {}]", base_to_gd(parts[0], p[0]), base_to_gd(parts[1], p[1]))
                } else { format!("\"{}\"", gd_escape(item)) }
            }).collect();
            format!("[{}]", items.join(", "))
        }
        Paradigm::ListTuple3 => {
            let items: Vec<String> = raw.split(&*sep.list_tuple3.list).map(|item| {
                let parts: Vec<&str> = item.splitn(3, &*sep.list_tuple3.tuple).collect();
                if parts.len() == 3 {
                    format!("[{}, {}, {}]", base_to_gd(parts[0], p[0]), base_to_gd(parts[1], p[1]), base_to_gd(parts[2], p[2]))
                } else { format!("\"{}\"", gd_escape(item)) }
            }).collect();
            format!("[{}]", items.join(", "))
        }
        Paradigm::ListTuple4 => {
            let items: Vec<String> = raw.split(&*sep.list_tuple4.list).map(|item| {
                let parts: Vec<&str> = item.splitn(4, &*sep.list_tuple4.tuple).collect();
                if parts.len() == 4 {
                    format!("[{}, {}, {}, {}]", base_to_gd(parts[0], p[0]), base_to_gd(parts[1], p[1]), base_to_gd(parts[2], p[2]), base_to_gd(parts[3], p[3]))
                } else { format!("\"{}\"", gd_escape(item)) }
            }).collect();
            format!("[{}]", items.join(", "))
        }

        Paradigm::MapTuple2 => {
            let entries: Vec<String> = raw.split(&*sep.map_tuple2.entry).map(|entry| {
                let kv: Vec<&str> = entry.splitn(2, &*sep.map_tuple2.kv).collect();
                if kv.len() == 2 {
                    let parts: Vec<&str> = kv[1].splitn(2, &*sep.map_tuple2.tuple).collect();
                    let v = if parts.len() == 2 {
                        format!("[{}, {}]", base_to_gd(parts[0], p[1]), base_to_gd(parts[1], p[2]))
                    } else { format!("\"{}\"", gd_escape(kv[1])) };
                    fmt_kv(gd_map_key(kv[0], p[0]), &v)
                } else { format!("\"{}\"", gd_escape(entry)) }
            }).collect();
            format!("{{{}}}", entries.join(", "))
        }
        Paradigm::MapTuple3 => {
            let entries: Vec<String> = raw.split(&*sep.map_tuple3.entry).map(|entry| {
                let kv: Vec<&str> = entry.splitn(2, &*sep.map_tuple3.kv).collect();
                if kv.len() == 2 {
                    let parts: Vec<&str> = kv[1].splitn(3, &*sep.map_tuple3.tuple).collect();
                    let v = if parts.len() == 3 {
                        format!("[{}, {}, {}]", base_to_gd(parts[0], p[1]), base_to_gd(parts[1], p[2]), base_to_gd(parts[2], p[3]))
                    } else { format!("\"{}\"", gd_escape(kv[1])) };
                    fmt_kv(gd_map_key(kv[0], p[0]), &v)
                } else { format!("\"{}\"", gd_escape(entry)) }
            }).collect();
            format!("{{{}}}", entries.join(", "))
        }
        Paradigm::MapTuple4 => {
            let entries: Vec<String> = raw.split(&*sep.map_tuple4.entry).map(|entry| {
                let kv: Vec<&str> = entry.splitn(2, &*sep.map_tuple4.kv).collect();
                if kv.len() == 2 {
                    let parts: Vec<&str> = kv[1].splitn(4, &*sep.map_tuple4.tuple).collect();
                    let v = if parts.len() == 4 {
                        format!("[{}, {}, {}, {}]", base_to_gd(parts[0], p[1]), base_to_gd(parts[1], p[2]), base_to_gd(parts[2], p[3]), base_to_gd(parts[3], p[4]))
                    } else { format!("\"{}\"", gd_escape(kv[1])) };
                    fmt_kv(gd_map_key(kv[0], p[0]), &v)
                } else { format!("\"{}\"", gd_escape(entry)) }
            }).collect();
            format!("{{{}}}", entries.join(", "))
        }
        Paradigm::MapList => {
            let entries: Vec<String> = raw.split(&*sep.map_list.entry).map(|entry| {
                let kv: Vec<&str> = entry.splitn(2, &*sep.map_list.kv).collect();
                if kv.len() == 2 {
                    let items: Vec<String> = kv[1].split(&*sep.map_list.item).map(|v| base_to_gd(v, p[1])).collect();
                    fmt_kv(gd_map_key(kv[0], p[0]), &format!("[{}]", items.join(", ")))
                } else { format!("\"{}\"", gd_escape(entry)) }
            }).collect();
            format!("{{{}}}", entries.join(", "))
        }

        // 引用类型：脚本语言始终输出 id（数字字面量），不做枚举类型替换
        Paradigm::Ref => base_to_gd(raw, BaseType::Int),
    }
}

pub fn export_table_gd(table: &Table, sep: &SeparatorsSection) -> String {
    let fields = &table.schema.fields;
    let export_cols: Vec<(usize, &FieldDef)> = fields.iter().enumerate()
        .filter(|(_, f)| is_client_export(&f.export))
        .collect();

    let index_col = fields.iter().position(|f| f.name == "id").unwrap_or(0);

    let mut s = String::new();
    writeln!(s, "const {} = {{", table.name).unwrap();

    for record in &table.records {
        let index_raw = record.get(index_col).map(|v| v.as_str()).unwrap_or("0");
        let index_val = index_raw.parse::<i64>().unwrap_or(0);

        let mut fields_str = String::new();
        for &(col, field) in &export_cols {
            let raw = record.get(col).map(|v| v.as_str()).unwrap_or("");
            if raw.is_empty() { continue; }
            let val = value_to_gd(raw, &field.tbl_type, sep);
            if !fields_str.is_empty() { fields_str.push_str(", "); }
            write!(fields_str, "{} = {}", field.name, val).unwrap();
        }

        writeln!(s, "    {}: {{{}}},", index_val, fields_str).unwrap();
    }

    writeln!(s, "}}").unwrap();
    s
}

pub fn export_constant_gd(constant: &Constant, sep: &SeparatorsSection) -> String {
    let mut s = String::new();
    writeln!(s, "const {} = {{", constant.name).unwrap();

    for entry in &constant.entries {
        if !is_client_export(&entry.export) { continue; }
        if entry.name.is_empty() { continue; }
        if entry.value.is_empty() { continue; }

        let val = value_to_gd(&entry.value, &entry.tbl_type, sep);
        writeln!(s, "    {} = {},", entry.name, val).unwrap();
    }

    writeln!(s, "}}").unwrap();
    s
}

pub fn export_enum_gd(enum_def: &EnumDef) -> String {
    let mut s = String::new();
    let valid: Vec<&EnumEntry> = enum_def.entries.iter()
        .filter(|e| !e.id.is_empty() && !e.name.is_empty())
        .collect();

    let max_name_len = valid.iter().map(|e| e.name.len()).max().unwrap_or(0);

    writeln!(s, "const {} = {{", enum_def.name).unwrap();

    for e in &valid {
        let pad = " ".repeat(max_name_len - e.name.len());
        let id = e.id.parse::<i64>().unwrap_or(0);
        writeln!(s, "    {}{} = {},  # {}", e.name, pad, id, e.desc).unwrap();
    }
    writeln!(s).unwrap();

    writeln!(s, "    desc = {{").unwrap();
    for e in &valid {
        let id = e.id.parse::<i64>().unwrap_or(0);
        writeln!(s, "        {}: \"{}\",", id, gd_escape(&e.desc)).unwrap();
    }
    writeln!(s, "    }},").unwrap();
    writeln!(s, "}}").unwrap();
    s
}

pub fn export_all_gdscript(project: &Project) -> Result<super::ExportResult> {
    let export_cfg = project.config.export.as_ref();
    let client = export_cfg.and_then(|e| e.client.as_ref());
    let gd = client.and_then(|c| c.gdscript.as_ref());

    let output = gd
        .and_then(|g| g.output.as_deref())
        .unwrap_or("gen/client/gdscript");

    let line_ending = LineEnding::from_config(
        gd.and_then(|g| g.line_ending.as_deref())
            .or_else(|| client.and_then(|c| c.line_ending.as_deref()))
            .or_else(|| export_cfg.and_then(|e| e.line_ending.as_deref()))
            .unwrap_or("lf")
    );
    let encoding = gd.and_then(|g| g.encoding.as_deref())
        .or_else(|| client.and_then(|c| c.encoding.as_deref()))
        .or_else(|| export_cfg.and_then(|e| e.encoding.as_deref()))
        .unwrap_or("utf-8").to_string();
    let opts = super::ExportOptions { line_ending, encoding };

    let sep = &project.config.separators;
    let output_dir = project.export_root().join(output);
    let mut collected = Vec::new();

    for group in &project.groups {
        for table in &group.tables {
            if table.deleted { continue; }
            let gd_src = export_table_gd(table, sep);
            let file_path = output_dir.join(format!("{}.gd", &table.name));
            collected.push((file_path, opts.encode(&gd_src)));
        }

        for constant in &group.constants {
            if constant.deleted { continue; }
            let gd_src = export_constant_gd(constant, sep);
            let file_path = output_dir.join(format!("{}.gd", &constant.name));
            collected.push((file_path, opts.encode(&gd_src)));
        }

        for enum_def in &group.enums {
            if enum_def.deleted { continue; }
            let gd_src = export_enum_gd(enum_def);
            let file_path = output_dir.join(format!("{}.gd", &enum_def.name));
            collected.push((file_path, opts.encode(&gd_src)));
        }
    }

    super::sync_export_dir(&output_dir, "gd", collected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SeparatorsSection;

    fn sep() -> SeparatorsSection { SeparatorsSection::default() }

    #[test]
    fn base_int_long() {
        assert_eq!(base_to_gd("42", BaseType::Int), "42");
        assert_eq!(base_to_gd("-1", BaseType::Long), "-1");
        assert_eq!(base_to_gd("abc", BaseType::Int), "0");
    }

    #[test]
    fn base_float_appends_dot_zero() {
        assert_eq!(base_to_gd("3", BaseType::Float), "3.0");
        assert_eq!(base_to_gd("3.5", BaseType::Float), "3.5");
    }

    #[test]
    fn base_bool() {
        assert_eq!(base_to_gd("true", BaseType::Bool), "true");
        assert_eq!(base_to_gd("1", BaseType::Bool), "true");
        assert_eq!(base_to_gd("0", BaseType::Bool), "false");
    }

    #[test]
    fn base_str_escape() {
        assert_eq!(base_to_gd("hi", BaseType::Str), r#""hi""#);
        assert_eq!(base_to_gd("a\"b", BaseType::Str), r#""a\"b""#);
        assert_eq!(base_to_gd("a\nb", BaseType::Str), r#""a\nb""#);
    }

    #[test]
    fn tuple2_list_set() {
        let s = sep();
        // Tuple2<int,str>: split by sep.tuple2 (default ",")
        assert_eq!(value_to_gd("1,foo", "Tuple2<int,str>", &s), "[1, \"foo\"]");
        // List<int>: sep.list (default ";")
        assert_eq!(value_to_gd("1;2;3", "List<int>", &s), "[1, 2, 3]");
        // Set<str> 退化成 Array
        assert_eq!(value_to_gd("a;b", "Set<str>", &s), "[\"a\", \"b\"]");
    }

    #[test]
    fn map_lua_style_for_identifier_str_key() {
        let s = sep();
        // Map<str,int>: default entry=";", kv=":"
        // identifier 键 → lua-style
        let out = value_to_gd("hp:100;mp:50", "Map<str,int>", &s);
        assert_eq!(out, "{hp = 100, mp = 50}");
        // 非 identifier 键 → 标准 "k": v
        let out = value_to_gd("hp/max:100", "Map<str,int>", &s);
        assert!(out.contains("\"hp/max\": 100"), "got {}", out);
        // int 键 → 标准 1: v
        let out = value_to_gd("1:a;2:b", "Map<int,str>", &s);
        assert_eq!(out, "{1: \"a\", 2: \"b\"}");
    }

    #[test]
    fn map_list_paradigm() {
        let s = sep();
        // MapList<str,int>: default entry=";", kv=":", item=","
        let out = value_to_gd("a:1,2,3;b:4,5", "Map<str,List<int>>", &s);
        assert_eq!(out, "{a = [1, 2, 3], b = [4, 5]}");
    }

    #[test]
    fn ref_paradigm_outputs_int() {
        let s = sep();
        // Ref 类型：原始值是 id 字符串，输出为整数
        assert_eq!(value_to_gd("42", "@HeroBase", &s), "42");
    }

    #[test]
    fn enum_gen() {
        let e = EnumDef {
            name: "HeroType".to_string(),
            path: std::path::PathBuf::new(),
            entries: vec![
                EnumEntry { id: "1".to_string(), name: "Warrior".to_string(), desc: "战士".to_string() },
                EnumEntry { id: "2".to_string(), name: "Mage".to_string(), desc: "法师".to_string() },
            ],
            dirty: false,
            deleted: false,
            original: String::new(),
        };
        let out = export_enum_gd(&e);
        assert!(out.contains("const HeroType = {"));
        assert!(out.contains("Warrior = 1,"));
        assert!(out.contains("Mage    = 2,"));
        assert!(out.contains("desc = {"));
        assert!(out.contains("1: \"战士\","));
    }
}
