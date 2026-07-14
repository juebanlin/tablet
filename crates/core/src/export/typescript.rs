//! TypeScript 导出（内嵌模式）。
//!
//! 形态：每张表/常量/枚举一个 `.ts` 文件。
//! - Table → `export interface XxxTpl { ... }` + `export const Xxx: Record<number, XxxTpl> = { ... }`
//! - Constant → `export const Xxx = { ... } as const`
//! - Enum → `export enum XxxEnum { ... }` + `export const XxxEnumDesc: Record<XxxEnum, string> = { ... }`
//!
//! 双 side：
//! - [`TypeScriptSide::Client`] → `[export.client.typescript]`，字段 `Export::ClientServer | ClientOnly`
//! - [`TypeScriptSide::Server`] → `[export.server.typescript]`（Node.js 服务端），字段 `Export::ClientServer | ServerOnly`
//!
//! 类型映射：
//! - int/long/float/double → number（long 不做溢出检测，文档说明限制）
//! - str → string
//! - bool → boolean
//! - List<T>/Set<T> → T[]
//! - Map<K,V> → Record<K, V>
//! - Ref（@T 或 @Enum）→ 字段类型为 number（保留 id），脚本语言无强类型 ref helper

use std::fmt::Write;
use anyhow::Result;
use crate::model::*;
use crate::types::*;
use super::LineEnding;

/// TypeScript 双 side 拆分点：决定字段可见性与默认输出路径。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeScriptSide {
    Client,
    Server,
}

impl TypeScriptSide {
    fn default_output(self) -> &'static str {
        match self {
            TypeScriptSide::Client => "gen/client/typescript",
            TypeScriptSide::Server => "gen/server/typescript",
        }
    }
}

fn export_visible(export: &Export, side: TypeScriptSide) -> bool {
    match side {
        TypeScriptSide::Client => matches!(export, Export::ClientServer | Export::ClientOnly),
        TypeScriptSide::Server => matches!(export, Export::ClientServer | Export::ServerOnly),
    }
}

fn ts_escape(s: &str) -> String {
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

fn is_ts_identifier(s: &str) -> bool {
    if s.is_empty() { return false; }
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_alphabetic() && first != '_' && first != '$' { return false; }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

fn base_to_ts_type(bt: BaseType) -> &'static str {
    match bt {
        BaseType::Int | BaseType::Long | BaseType::Float | BaseType::Double => "number",
        BaseType::Str => "string",
        BaseType::Bool => "boolean",
    }
}

fn base_to_ts(raw: &str, bt: BaseType) -> String {
    match bt {
        BaseType::Int | BaseType::Long => {
            raw.parse::<i64>().map(|v| v.to_string()).unwrap_or_else(|_| "0".to_string())
        }
        BaseType::Float | BaseType::Double => {
            raw.parse::<f64>().map(|v| v.to_string()).unwrap_or_else(|_| "0".to_string())
        }
        BaseType::Bool => {
            if raw == "true" || raw == "1" { "true".to_string() } else { "false".to_string() }
        }
        BaseType::Str => format!("\"{}\"", ts_escape(raw)),
    }
}

/// TS 字面量对象的 key：合法 identifier 不加引号，否则加引号；int/long key 直接裸数字。
fn ts_obj_key(raw: &str, bt: BaseType) -> String {
    match bt {
        BaseType::Str => {
            if is_ts_identifier(raw) {
                raw.to_string()
            } else {
                format!("\"{}\"", ts_escape(raw))
            }
        }
        _ => base_to_ts(raw, bt),
    }
}

/// 类型注解（TS interface 字段类型）。Ref 字段在脚本语言全部回退 number。
fn ts_field_type(tbl_type_str: &str) -> String {
    let tt = match TblType::parse(tbl_type_str) {
        Some(t) => t,
        None => return "string".to_string(),
    };

    let p = &tt.params;
    match &tt.paradigm {
        Paradigm::Base => base_to_ts_type(p[0]).to_string(),
        Paradigm::Tuple2 => format!("[{}, {}]", base_to_ts_type(p[0]), base_to_ts_type(p[1])),
        Paradigm::Tuple3 => format!("[{}, {}, {}]", base_to_ts_type(p[0]), base_to_ts_type(p[1]), base_to_ts_type(p[2])),
        Paradigm::Tuple4 => format!("[{}, {}, {}, {}]", base_to_ts_type(p[0]), base_to_ts_type(p[1]), base_to_ts_type(p[2]), base_to_ts_type(p[3])),
        Paradigm::List | Paradigm::Set => format!("{}[]", base_to_ts_type(p[0])),
        Paradigm::Map => format!("Record<{}, {}>", base_to_ts_type(p[0]), base_to_ts_type(p[1])),
        Paradigm::ListTuple2 => format!("[{}, {}][]", base_to_ts_type(p[0]), base_to_ts_type(p[1])),
        Paradigm::ListTuple3 => format!("[{}, {}, {}][]", base_to_ts_type(p[0]), base_to_ts_type(p[1]), base_to_ts_type(p[2])),
        Paradigm::ListTuple4 => format!("[{}, {}, {}, {}][]", base_to_ts_type(p[0]), base_to_ts_type(p[1]), base_to_ts_type(p[2]), base_to_ts_type(p[3])),
        Paradigm::MapTuple2 => format!("Record<{}, [{}, {}]>", base_to_ts_type(p[0]), base_to_ts_type(p[1]), base_to_ts_type(p[2])),
        Paradigm::MapTuple3 => format!("Record<{}, [{}, {}, {}]>", base_to_ts_type(p[0]), base_to_ts_type(p[1]), base_to_ts_type(p[2]), base_to_ts_type(p[3])),
        Paradigm::MapTuple4 => format!("Record<{}, [{}, {}, {}, {}]>", base_to_ts_type(p[0]), base_to_ts_type(p[1]), base_to_ts_type(p[2]), base_to_ts_type(p[3]), base_to_ts_type(p[4])),
        Paradigm::MapList => format!("Record<{}, {}[]>", base_to_ts_type(p[0]), base_to_ts_type(p[1])),
        Paradigm::Ref => "number".to_string(),
    }
}

fn value_to_ts(raw: &str, tbl_type_str: &str, sep: &SeparatorsSection) -> String {
    if raw.is_empty() { return "null".to_string(); }

    let tt = match TblType::parse(tbl_type_str) {
        Some(t) => t,
        None => return format!("\"{}\"", ts_escape(raw)),
    };

    let p = &tt.params;
    match &tt.paradigm {
        Paradigm::Base => base_to_ts(raw, p[0]),

        Paradigm::Tuple2 => {
            let parts: Vec<&str> = raw.splitn(2, &*sep.tuple2).collect();
            if parts.len() == 2 {
                format!("[{}, {}]", base_to_ts(parts[0], p[0]), base_to_ts(parts[1], p[1]))
            } else { format!("\"{}\"", ts_escape(raw)) }
        }
        Paradigm::Tuple3 => {
            let parts: Vec<&str> = raw.splitn(3, &*sep.tuple3).collect();
            if parts.len() == 3 {
                format!("[{}, {}, {}]", base_to_ts(parts[0], p[0]), base_to_ts(parts[1], p[1]), base_to_ts(parts[2], p[2]))
            } else { format!("\"{}\"", ts_escape(raw)) }
        }
        Paradigm::Tuple4 => {
            let parts: Vec<&str> = raw.splitn(4, &*sep.tuple4).collect();
            if parts.len() == 4 {
                format!("[{}, {}, {}, {}]", base_to_ts(parts[0], p[0]), base_to_ts(parts[1], p[1]), base_to_ts(parts[2], p[2]), base_to_ts(parts[3], p[3]))
            } else { format!("\"{}\"", ts_escape(raw)) }
        }

        Paradigm::List => {
            let items: Vec<String> = raw.split(&*sep.list).map(|v| base_to_ts(v, p[0])).collect();
            format!("[{}]", items.join(", "))
        }
        Paradigm::Set => {
            let items: Vec<String> = raw.split(&*sep.set).map(|v| base_to_ts(v, p[0])).collect();
            format!("[{}]", items.join(", "))
        }
        Paradigm::Map => {
            let entries: Vec<String> = raw.split(&*sep.map.entry).map(|entry| {
                let kv: Vec<&str> = entry.splitn(2, &*sep.map.kv).collect();
                if kv.len() == 2 {
                    format!("{}: {}", ts_obj_key(kv[0], p[0]), base_to_ts(kv[1], p[1]))
                } else { format!("\"{}\"", ts_escape(entry)) }
            }).collect();
            format!("{{ {} }}", entries.join(", "))
        }

        Paradigm::ListTuple2 => {
            let items: Vec<String> = raw.split(&*sep.list_tuple2.list).map(|item| {
                let parts: Vec<&str> = item.splitn(2, &*sep.list_tuple2.tuple).collect();
                if parts.len() == 2 {
                    format!("[{}, {}]", base_to_ts(parts[0], p[0]), base_to_ts(parts[1], p[1]))
                } else { format!("\"{}\"", ts_escape(item)) }
            }).collect();
            format!("[{}]", items.join(", "))
        }
        Paradigm::ListTuple3 => {
            let items: Vec<String> = raw.split(&*sep.list_tuple3.list).map(|item| {
                let parts: Vec<&str> = item.splitn(3, &*sep.list_tuple3.tuple).collect();
                if parts.len() == 3 {
                    format!("[{}, {}, {}]", base_to_ts(parts[0], p[0]), base_to_ts(parts[1], p[1]), base_to_ts(parts[2], p[2]))
                } else { format!("\"{}\"", ts_escape(item)) }
            }).collect();
            format!("[{}]", items.join(", "))
        }
        Paradigm::ListTuple4 => {
            let items: Vec<String> = raw.split(&*sep.list_tuple4.list).map(|item| {
                let parts: Vec<&str> = item.splitn(4, &*sep.list_tuple4.tuple).collect();
                if parts.len() == 4 {
                    format!("[{}, {}, {}, {}]", base_to_ts(parts[0], p[0]), base_to_ts(parts[1], p[1]), base_to_ts(parts[2], p[2]), base_to_ts(parts[3], p[3]))
                } else { format!("\"{}\"", ts_escape(item)) }
            }).collect();
            format!("[{}]", items.join(", "))
        }

        Paradigm::MapTuple2 => {
            let entries: Vec<String> = raw.split(&*sep.map_tuple2.entry).map(|entry| {
                let kv: Vec<&str> = entry.splitn(2, &*sep.map_tuple2.kv).collect();
                if kv.len() == 2 {
                    let parts: Vec<&str> = kv[1].splitn(2, &*sep.map_tuple2.tuple).collect();
                    let v = if parts.len() == 2 {
                        format!("[{}, {}]", base_to_ts(parts[0], p[1]), base_to_ts(parts[1], p[2]))
                    } else { format!("\"{}\"", ts_escape(kv[1])) };
                    format!("{}: {}", ts_obj_key(kv[0], p[0]), v)
                } else { format!("\"{}\"", ts_escape(entry)) }
            }).collect();
            format!("{{ {} }}", entries.join(", "))
        }
        Paradigm::MapTuple3 => {
            let entries: Vec<String> = raw.split(&*sep.map_tuple3.entry).map(|entry| {
                let kv: Vec<&str> = entry.splitn(2, &*sep.map_tuple3.kv).collect();
                if kv.len() == 2 {
                    let parts: Vec<&str> = kv[1].splitn(3, &*sep.map_tuple3.tuple).collect();
                    let v = if parts.len() == 3 {
                        format!("[{}, {}, {}]", base_to_ts(parts[0], p[1]), base_to_ts(parts[1], p[2]), base_to_ts(parts[2], p[3]))
                    } else { format!("\"{}\"", ts_escape(kv[1])) };
                    format!("{}: {}", ts_obj_key(kv[0], p[0]), v)
                } else { format!("\"{}\"", ts_escape(entry)) }
            }).collect();
            format!("{{ {} }}", entries.join(", "))
        }
        Paradigm::MapTuple4 => {
            let entries: Vec<String> = raw.split(&*sep.map_tuple4.entry).map(|entry| {
                let kv: Vec<&str> = entry.splitn(2, &*sep.map_tuple4.kv).collect();
                if kv.len() == 2 {
                    let parts: Vec<&str> = kv[1].splitn(4, &*sep.map_tuple4.tuple).collect();
                    let v = if parts.len() == 4 {
                        format!("[{}, {}, {}, {}]", base_to_ts(parts[0], p[1]), base_to_ts(parts[1], p[2]), base_to_ts(parts[2], p[3]), base_to_ts(parts[3], p[4]))
                    } else { format!("\"{}\"", ts_escape(kv[1])) };
                    format!("{}: {}", ts_obj_key(kv[0], p[0]), v)
                } else { format!("\"{}\"", ts_escape(entry)) }
            }).collect();
            format!("{{ {} }}", entries.join(", "))
        }
        Paradigm::MapList => {
            let entries: Vec<String> = raw.split(&*sep.map_list.entry).map(|entry| {
                let kv: Vec<&str> = entry.splitn(2, &*sep.map_list.kv).collect();
                if kv.len() == 2 {
                    let items: Vec<String> = kv[1].split(&*sep.map_list.item).map(|v| base_to_ts(v, p[1])).collect();
                    format!("{}: [{}]", ts_obj_key(kv[0], p[0]), items.join(", "))
                } else { format!("\"{}\"", ts_escape(entry)) }
            }).collect();
            format!("{{ {} }}", entries.join(", "))
        }

        Paradigm::Ref => base_to_ts(raw, BaseType::Int),
    }
}

pub fn export_table_ts(table: &Table, side: TypeScriptSide, module_kind: crate::enums::ModuleKind) -> String {
    let fields = &table.schema.fields;
    let export_cols: Vec<(usize, &FieldDef)> = fields.iter().enumerate()
        .filter(|(_, f)| export_visible(&f.export, side))
        .collect();

    let index_col = fields.iter().position(|f| f.name == "id").unwrap_or(0);

    let mut s = String::new();

    let iface_name = format!("{}Tpl", table.name);

    match module_kind {
        crate::enums::ModuleKind::Esm => {
            writeln!(s, "export interface {} {{", iface_name).unwrap();
        }
        crate::enums::ModuleKind::CommonJs => {
            writeln!(s, "interface {} {{", iface_name).unwrap();
        }
    }

    for &(_, field) in &export_cols {
        writeln!(s, "    {}: {};", field.name, ts_field_type(&field.tbl_type)).unwrap();
    }
    writeln!(s, "}}").unwrap();
    writeln!(s).unwrap();

    match module_kind {
        crate::enums::ModuleKind::Esm => {
            writeln!(s, "export const {}: Record<number, {}> = {{", table.name, iface_name).unwrap();
        }
        crate::enums::ModuleKind::CommonJs => {
            writeln!(s, "const {}: Record<number, {}> = {{", table.name, iface_name).unwrap();
        }
    }

    let sep = &SeparatorsSection::default();
    for record in &table.records {
        let index_raw = record.get(index_col).map(|v| v.as_str()).unwrap_or("0");
        let index_val = index_raw.parse::<i64>().unwrap_or(0);

        let mut fields_str = String::new();
        for &(col, field) in &export_cols {
            let raw = record.get(col).map(|v| v.as_str()).unwrap_or("");
            let val = if raw.is_empty() {
                // 空值用类型默认值（保持 TS 类型完整性）
                default_for_field(&field.tbl_type)
            } else {
                value_to_ts(raw, &field.tbl_type, sep)
            };
            if !fields_str.is_empty() { fields_str.push_str(", "); }
            write!(fields_str, "{}: {}", field.name, val).unwrap();
        }

        writeln!(s, "    {}: {{ {} }},", index_val, fields_str).unwrap();
    }

    writeln!(s, "}};").unwrap();

    if module_kind == crate::enums::ModuleKind::CommonJs {
        writeln!(s, "module.exports.{} = {};", table.name, table.name).unwrap();
    }

    s
}

/// Table 字段空值默认值——保持 interface 类型完整性。
fn default_for_field(tbl_type_str: &str) -> String {
    let tt = match TblType::parse(tbl_type_str) {
        Some(t) => t,
        None => return "\"\"".to_string(),
    };
    match &tt.paradigm {
        Paradigm::Base => match tt.params[0] {
            BaseType::Int | BaseType::Long | BaseType::Float | BaseType::Double => "0".to_string(),
            BaseType::Str => "\"\"".to_string(),
            BaseType::Bool => "false".to_string(),
        },
        Paradigm::Ref => "0".to_string(),
        Paradigm::Tuple2 | Paradigm::Tuple3 | Paradigm::Tuple4
        | Paradigm::List | Paradigm::Set
        | Paradigm::ListTuple2 | Paradigm::ListTuple3 | Paradigm::ListTuple4 => "[]".to_string(),
        Paradigm::Map | Paradigm::MapTuple2 | Paradigm::MapTuple3 | Paradigm::MapTuple4 | Paradigm::MapList => "{}".to_string(),
    }
}

pub fn export_constant_ts(constant: &Constant, sep: &SeparatorsSection, side: TypeScriptSide, module_kind: crate::enums::ModuleKind) -> String {
    let mut s = String::new();

    match module_kind {
        crate::enums::ModuleKind::Esm => {
            writeln!(s, "export const {} = {{", constant.name).unwrap();
        }
        crate::enums::ModuleKind::CommonJs => {
            writeln!(s, "const {} = {{", constant.name).unwrap();
        }
    }

    for entry in &constant.entries {
        if !export_visible(&entry.export, side) { continue; }
        if entry.name.is_empty() { continue; }
        if entry.value.is_empty() { continue; }

        let val = value_to_ts(&entry.value, &entry.tbl_type, sep);
        writeln!(s, "    {}: {},", entry.name, val).unwrap();
    }

    match module_kind {
        crate::enums::ModuleKind::Esm => {
            writeln!(s, "}} as const;").unwrap();
        }
        crate::enums::ModuleKind::CommonJs => {
            writeln!(s, "}};").unwrap();
            writeln!(s, "module.exports.{} = {};", constant.name, constant.name).unwrap();
        }
    }

    s
}

pub fn export_enum_ts(enum_def: &EnumDef, module_kind: crate::enums::ModuleKind) -> String {
    let mut s = String::new();
    let valid: Vec<&EnumEntry> = enum_def.entries.iter()
        .filter(|e| !e.id.is_empty() && !e.name.is_empty())
        .collect();

    let enum_name = format!("{}Enum", enum_def.name);

    match module_kind {
        crate::enums::ModuleKind::Esm => {
            writeln!(s, "export enum {} {{", enum_name).unwrap();
        }
        crate::enums::ModuleKind::CommonJs => {
            writeln!(s, "const {} = {{", enum_name).unwrap();
        }
    }

    for e in &valid {
        let id = e.id.parse::<i64>().unwrap_or(0);
        match module_kind {
            crate::enums::ModuleKind::Esm => {
                writeln!(s, "    /** {} */", e.desc).unwrap();
                writeln!(s, "    {} = {},", e.name, id).unwrap();
            }
            crate::enums::ModuleKind::CommonJs => {
                writeln!(s, "    {}: {}, // {}", e.name, id, e.desc).unwrap();
            }
        }
    }
    writeln!(s, "}}").unwrap();

    if module_kind == crate::enums::ModuleKind::CommonJs {
        writeln!(s, "module.exports.{} = {};", enum_name, enum_name).unwrap();
    }

    writeln!(s).unwrap();

    match module_kind {
        crate::enums::ModuleKind::Esm => {
            writeln!(s, "export const {}Desc: Record<{}, string> = {{", enum_name, enum_name).unwrap();
            for e in &valid {
                writeln!(s, "    [{}.{}]: \"{}\",", enum_name, e.name, ts_escape(&e.desc)).unwrap();
            }
            writeln!(s, "}};").unwrap();
        }
        crate::enums::ModuleKind::CommonJs => {
            writeln!(s, "const {}Desc = {{", enum_name).unwrap();
            for e in &valid {
                writeln!(s, "    [{}[\"{}\"]]: \"{}\",", enum_name, e.name, ts_escape(&e.desc)).unwrap();
            }
            writeln!(s, "}};").unwrap();
            writeln!(s, "module.exports.{}Desc = {}Desc;", enum_name, enum_name).unwrap();
        }
    }
    s
}

pub fn export_all_typescript(project: &Project, side: TypeScriptSide) -> Result<super::ExportResult> {
    let export_cfg = project.config.export.as_ref();

    // 按 side 取对应段；fall-back 串：side 段 → 父段（client/server）→ 顶层 export → 默认
    let (output, module_kind) = match side {
        TypeScriptSide::Client => {
            let ts = export_cfg.and_then(|e| e.client.as_ref()).and_then(|c| c.typescript.as_ref());
            let output = ts.and_then(|t| t.output.as_deref()).unwrap_or(side.default_output());
            let module_kind = ts.and_then(|t| t.module_kind).unwrap_or(crate::enums::ModuleKind::default());
            (output, module_kind)
        }
        TypeScriptSide::Server => {
            let ts = export_cfg.and_then(|e| e.server.as_ref()).and_then(|s| s.typescript.as_ref());
            let output = ts.and_then(|t| t.output.as_deref()).unwrap_or(side.default_output());
            let module_kind = ts.and_then(|t| t.module_kind).unwrap_or(crate::enums::ModuleKind::default());
            (output, module_kind)
        }
    };

    let line_ending = LineEnding::from_config(
        export_cfg.and_then(|e| e.line_ending.map(|l| l.as_str()))
            .unwrap_or("lf")
    );
    let encoding = export_cfg.and_then(|e| e.encoding.map(|e| e.as_str()))
        .unwrap_or("utf-8").to_string();
    let opts = super::ExportOptions { line_ending, encoding };

    let sep = &project.config.separators;
    let output_dir = project.export_root().join(output);
    let mut collected = Vec::new();

    for group in &project.groups {
        for table in &group.tables {
            if table.deleted { continue; }
            let src = export_table_ts(table, side, module_kind);
            let file_path = output_dir.join(format!("{}.ts", &table.name));
            collected.push((file_path, opts.encode(&src)));
        }

        for constant in &group.constants {
            if constant.deleted { continue; }
            let src = export_constant_ts(constant, sep, side, module_kind);
            let file_path = output_dir.join(format!("{}.ts", &constant.name));
            collected.push((file_path, opts.encode(&src)));
        }

        for enum_def in &group.enums {
            if enum_def.deleted { continue; }
            let src = export_enum_ts(enum_def, module_kind);
            let file_path = output_dir.join(format!("{}.ts", &enum_def.name));
            collected.push((file_path, opts.encode(&src)));
        }
    }

    super::sync_export_dir(&output_dir, "ts", collected)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sep() -> SeparatorsSection { SeparatorsSection::default() }

    #[test]
    fn base_types() {
        assert_eq!(base_to_ts_type(BaseType::Int), "number");
        assert_eq!(base_to_ts_type(BaseType::Long), "number");
        assert_eq!(base_to_ts_type(BaseType::Float), "number");
        assert_eq!(base_to_ts_type(BaseType::Str), "string");
        assert_eq!(base_to_ts_type(BaseType::Bool), "boolean");
    }

    #[test]
    fn field_type_paradigms() {
        assert_eq!(ts_field_type("int"), "number");
        assert_eq!(ts_field_type("str"), "string");
        assert_eq!(ts_field_type("bool"), "boolean");
        assert_eq!(ts_field_type("Tuple2<int,str>"), "[number, string]");
        assert_eq!(ts_field_type("Tuple4<int,int,int,int>"), "[number, number, number, number]");
        assert_eq!(ts_field_type("List<int>"), "number[]");
        assert_eq!(ts_field_type("Set<str>"), "string[]");
        assert_eq!(ts_field_type("Map<int,str>"), "Record<number, string>");
        assert_eq!(ts_field_type("List<Tuple2<int,str>>"), "[number, string][]");
        assert_eq!(ts_field_type("Map<str,Tuple2<int,int>>"), "Record<string, [number, number]>");
        assert_eq!(ts_field_type("Map<str,List<int>>"), "Record<string, number[]>");
        assert_eq!(ts_field_type("@HeroBase"), "number");
    }

    #[test]
    fn value_str_escape() {
        assert_eq!(base_to_ts("a\"b", BaseType::Str), r#""a\"b""#);
        assert_eq!(base_to_ts("a\nb", BaseType::Str), r#""a\nb""#);
    }

    #[test]
    fn value_tuple_list() {
        let s = sep();
        assert_eq!(value_to_ts("1,foo", "Tuple2<int,str>", &s), "[1, \"foo\"]");
        assert_eq!(value_to_ts("1;2;3", "List<int>", &s), "[1, 2, 3]");
        assert_eq!(value_to_ts("a;b", "Set<str>", &s), "[\"a\", \"b\"]");
    }

    #[test]
    fn value_map() {
        let s = sep();
        // Map<str,int>: entry=";", kv=":"
        let out = value_to_ts("hp:100;mp:50", "Map<str,int>", &s);
        assert_eq!(out, "{ hp: 100, mp: 50 }");
        // 非 identifier key 加引号
        let out = value_to_ts("hp/max:100", "Map<str,int>", &s);
        assert!(out.contains("\"hp/max\": 100"), "got {}", out);
        // int 键裸数字
        let out = value_to_ts("1:a;2:b", "Map<int,str>", &s);
        assert_eq!(out, "{ 1: \"a\", 2: \"b\" }");
    }

    #[test]
    fn value_map_list() {
        let s = sep();
        let out = value_to_ts("a:1,2,3;b:4,5", "Map<str,List<int>>", &s);
        assert_eq!(out, "{ a: [1, 2, 3], b: [4, 5] }");
    }

    #[test]
    fn ref_value_outputs_int() {
        let s = sep();
        assert_eq!(value_to_ts("42", "@HeroBase", &s), "42");
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
        let out = export_enum_ts(&e, crate::enums::ModuleKind::Esm);
        assert!(out.contains("export enum HeroTypeEnum {"));
        assert!(out.contains("Warrior = 1,"));
        assert!(out.contains("Mage = 2,"));
        assert!(out.contains("export const HeroTypeEnumDesc: Record<HeroTypeEnum, string>"));
        assert!(out.contains("[HeroTypeEnum.Warrior]: \"战士\","));
    }
}
