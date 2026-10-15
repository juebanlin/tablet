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
        BaseType::Txt => "string",
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
        BaseType::Str | BaseType::Txt => format!("\"{}\"", ts_escape(raw)),
    }
}

/// TS 字面量对象的 key：合法 identifier 不加引号，否则加引号；int/long key 直接裸数字。
fn ts_obj_key(raw: &str, bt: BaseType) -> String {
    match bt {
        BaseType::Str | BaseType::Txt => {
            if is_ts_identifier(raw) {
                raw.to_string()
            } else {
                format!("\"{}\"", ts_escape(raw))
            }
        }
        _ => base_to_ts(raw, bt),
    }
}

/// 参数位类型注解。ref 元素 → number（脚本语言无强类型 ref helper）。
fn ts_param_type(param: &TypeParam) -> &'static str {
    match param {
        TypeParam::Base(bt) => base_to_ts_type(*bt),
        TypeParam::Ref(_) => "number",
    }
}

/// 参数位值 → TS 字面量。ref 元素退化为 int id。
fn ts_param(raw: &str, param: &TypeParam) -> String {
    match param {
        TypeParam::Base(bt) => base_to_ts(raw, *bt),
        TypeParam::Ref(_) => base_to_ts(raw, BaseType::Int),
    }
}

/// 参数位 key → TS 对象 key。ref key 退化为 int id。
fn ts_param_key(raw: &str, param: &TypeParam) -> String {
    match param {
        TypeParam::Base(bt) => ts_obj_key(raw, *bt),
        TypeParam::Ref(_) => ts_obj_key(raw, BaseType::Int),
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
        Paradigm::Base => ts_param_type(&p[0]).to_string(),
        Paradigm::Tuple2 => format!("[{}, {}]", ts_param_type(&p[0]), ts_param_type(&p[1])),
        Paradigm::Tuple3 => format!("[{}, {}, {}]", ts_param_type(&p[0]), ts_param_type(&p[1]), ts_param_type(&p[2])),
        Paradigm::Tuple4 => format!("[{}, {}, {}, {}]", ts_param_type(&p[0]), ts_param_type(&p[1]), ts_param_type(&p[2]), ts_param_type(&p[3])),
        Paradigm::List | Paradigm::Set => format!("{}[]", ts_param_type(&p[0])),
        Paradigm::Map => format!("Record<{}, {}>", ts_param_type(&p[0]), ts_param_type(&p[1])),
        Paradigm::ListTuple2 => format!("[{}, {}][]", ts_param_type(&p[0]), ts_param_type(&p[1])),
        Paradigm::ListTuple3 => format!("[{}, {}, {}][]", ts_param_type(&p[0]), ts_param_type(&p[1]), ts_param_type(&p[2])),
        Paradigm::ListTuple4 => format!("[{}, {}, {}, {}][]", ts_param_type(&p[0]), ts_param_type(&p[1]), ts_param_type(&p[2]), ts_param_type(&p[3])),
        Paradigm::MapTuple2 => format!("Record<{}, [{}, {}]>", ts_param_type(&p[0]), ts_param_type(&p[1]), ts_param_type(&p[2])),
        Paradigm::MapTuple3 => format!("Record<{}, [{}, {}, {}]>", ts_param_type(&p[0]), ts_param_type(&p[1]), ts_param_type(&p[2]), ts_param_type(&p[3])),
        Paradigm::MapTuple4 => format!("Record<{}, [{}, {}, {}, {}]>", ts_param_type(&p[0]), ts_param_type(&p[1]), ts_param_type(&p[2]), ts_param_type(&p[3]), ts_param_type(&p[4])),
        Paradigm::MapList => format!("Record<{}, {}[]>", ts_param_type(&p[0]), ts_param_type(&p[1])),
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
        Paradigm::Base => ts_param(raw, &p[0]),

        Paradigm::Tuple2 => {
            let parts: Vec<&str> = raw.splitn(2, &*sep.tuple2).collect();
            if parts.len() == 2 {
                format!("[{}, {}]", ts_param(parts[0], &p[0]), ts_param(parts[1], &p[1]))
            } else { format!("\"{}\"", ts_escape(raw)) }
        }
        Paradigm::Tuple3 => {
            let parts: Vec<&str> = raw.splitn(3, &*sep.tuple3).collect();
            if parts.len() == 3 {
                format!("[{}, {}, {}]", ts_param(parts[0], &p[0]), ts_param(parts[1], &p[1]), ts_param(parts[2], &p[2]))
            } else { format!("\"{}\"", ts_escape(raw)) }
        }
        Paradigm::Tuple4 => {
            let parts: Vec<&str> = raw.splitn(4, &*sep.tuple4).collect();
            if parts.len() == 4 {
                format!("[{}, {}, {}, {}]", ts_param(parts[0], &p[0]), ts_param(parts[1], &p[1]), ts_param(parts[2], &p[2]), ts_param(parts[3], &p[3]))
            } else { format!("\"{}\"", ts_escape(raw)) }
        }

        Paradigm::List => {
            let items: Vec<String> = raw.split(&*sep.list).map(|v| ts_param(v, &p[0])).collect();
            format!("[{}]", items.join(", "))
        }
        Paradigm::Set => {
            let items: Vec<String> = raw.split(&*sep.set).map(|v| ts_param(v, &p[0])).collect();
            format!("[{}]", items.join(", "))
        }
        Paradigm::Map => {
            let entries: Vec<String> = raw.split(&*sep.map.entry).map(|entry| {
                let kv: Vec<&str> = entry.splitn(2, &*sep.map.kv).collect();
                if kv.len() == 2 {
                    format!("{}: {}", ts_param_key(kv[0], &p[0]), ts_param(kv[1], &p[1]))
                } else { format!("\"{}\"", ts_escape(entry)) }
            }).collect();
            format!("{{ {} }}", entries.join(", "))
        }

        Paradigm::ListTuple2 => {
            let items: Vec<String> = raw.split(&*sep.list_tuple2.list).map(|item| {
                let parts: Vec<&str> = item.splitn(2, &*sep.list_tuple2.tuple).collect();
                if parts.len() == 2 {
                    format!("[{}, {}]", ts_param(parts[0], &p[0]), ts_param(parts[1], &p[1]))
                } else { format!("\"{}\"", ts_escape(item)) }
            }).collect();
            format!("[{}]", items.join(", "))
        }
        Paradigm::ListTuple3 => {
            let items: Vec<String> = raw.split(&*sep.list_tuple3.list).map(|item| {
                let parts: Vec<&str> = item.splitn(3, &*sep.list_tuple3.tuple).collect();
                if parts.len() == 3 {
                    format!("[{}, {}, {}]", ts_param(parts[0], &p[0]), ts_param(parts[1], &p[1]), ts_param(parts[2], &p[2]))
                } else { format!("\"{}\"", ts_escape(item)) }
            }).collect();
            format!("[{}]", items.join(", "))
        }
        Paradigm::ListTuple4 => {
            let items: Vec<String> = raw.split(&*sep.list_tuple4.list).map(|item| {
                let parts: Vec<&str> = item.splitn(4, &*sep.list_tuple4.tuple).collect();
                if parts.len() == 4 {
                    format!("[{}, {}, {}, {}]", ts_param(parts[0], &p[0]), ts_param(parts[1], &p[1]), ts_param(parts[2], &p[2]), ts_param(parts[3], &p[3]))
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
                        format!("[{}, {}]", ts_param(parts[0], &p[1]), ts_param(parts[1], &p[2]))
                    } else { format!("\"{}\"", ts_escape(kv[1])) };
                    format!("{}: {}", ts_param_key(kv[0], &p[0]), v)
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
                        format!("[{}, {}, {}]", ts_param(parts[0], &p[1]), ts_param(parts[1], &p[2]), ts_param(parts[2], &p[3]))
                    } else { format!("\"{}\"", ts_escape(kv[1])) };
                    format!("{}: {}", ts_param_key(kv[0], &p[0]), v)
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
                        format!("[{}, {}, {}, {}]", ts_param(parts[0], &p[1]), ts_param(parts[1], &p[2]), ts_param(parts[2], &p[3]), ts_param(parts[3], &p[4]))
                    } else { format!("\"{}\"", ts_escape(kv[1])) };
                    format!("{}: {}", ts_param_key(kv[0], &p[0]), v)
                } else { format!("\"{}\"", ts_escape(entry)) }
            }).collect();
            format!("{{ {} }}", entries.join(", "))
        }
        Paradigm::MapList => {
            let entries: Vec<String> = raw.split(&*sep.map_list.entry).map(|entry| {
                let kv: Vec<&str> = entry.splitn(2, &*sep.map_list.kv).collect();
                if kv.len() == 2 {
                    let items: Vec<String> = kv[1].split(&*sep.map_list.item).map(|v| ts_param(v, &p[1])).collect();
                    format!("{}: [{}]", ts_param_key(kv[0], &p[0]), items.join(", "))
                } else { format!("\"{}\"", ts_escape(entry)) }
            }).collect();
            format!("{{ {} }}", entries.join(", "))
        }

        Paradigm::Ref => base_to_ts(raw, BaseType::Int),
    }
}

pub fn export_table_ts(table: &Table, side: TypeScriptSide) -> String {
    let fields = &table.schema.fields;
    let export_cols: Vec<(usize, &FieldDef)> = fields.iter().enumerate()
        .filter(|(_, f)| export_visible(&f.export, side))
        .collect();

    let index_col = fields.iter().position(|f| f.name == "id").unwrap_or(0);

    let mut s = String::new();
    writeln!(s, "// 使用：import {{ {}, {}Tpl }} from './{}'",
        table.name, table.name, table.name).unwrap();
    // 内联产物没有加载器 API，遍历写法是最容易卡住的一条，直接写在文件头
    writeln!(s, "// 遍历：Object.values({})", table.name).unwrap();

    let iface_name = format!("{}Tpl", table.name);

    writeln!(s, "export interface {} {{", iface_name).unwrap();

    for &(_, field) in &export_cols {
        if !field.desc.is_empty() {
            writeln!(s, "    /** {} */", super::comment_block(&field.desc)).unwrap();
        }
        // JS/TS 生态的属性名惯例是 camelCase（也跟 JSON 数据文件的 key 对齐）
        writeln!(s, "    {}: {};", super::to_camel_case(&field.name), ts_field_type(&field.tbl_type)).unwrap();
    }
    writeln!(s, "}}").unwrap();
    writeln!(s).unwrap();

    writeln!(s, "export const {}: Record<number, {}> = {{", table.name, iface_name).unwrap();

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
            write!(fields_str, "{}: {}", super::to_camel_case(&field.name), val).unwrap();
        }

        writeln!(s, "    {}: {{ {} }},", index_val, fields_str).unwrap();
    }

    writeln!(s, "}};").unwrap();

    s
}

/// Table 字段空值默认值——保持 interface 类型完整性。
fn default_for_field(tbl_type_str: &str) -> String {
    let tt = match TblType::parse(tbl_type_str) {
        Some(t) => t,
        None => return "\"\"".to_string(),
    };
    match &tt.paradigm {
        Paradigm::Base => match &tt.params[0] {
            TypeParam::Base(bt) => match bt {
                BaseType::Int | BaseType::Long | BaseType::Float | BaseType::Double => "0".to_string(),
                BaseType::Str => "\"\"".to_string(),
                BaseType::Bool => "false".to_string(),
                BaseType::Txt => "\"\"".to_string(),
            },
            TypeParam::Ref(_) => "0".to_string(),
        },
        Paradigm::Ref => "0".to_string(),
        Paradigm::Tuple2 | Paradigm::Tuple3 | Paradigm::Tuple4
        | Paradigm::List | Paradigm::Set
        | Paradigm::ListTuple2 | Paradigm::ListTuple3 | Paradigm::ListTuple4 => "[]".to_string(),
        Paradigm::Map | Paradigm::MapTuple2 | Paradigm::MapTuple3 | Paradigm::MapTuple4 | Paradigm::MapList => "{}".to_string(),
    }
}

pub fn export_constant_ts(constant: &Constant, sep: &SeparatorsSection, side: TypeScriptSide) -> String {
    let mut s = String::new();
    writeln!(s, "// 使用：import {{ {} }} from './{}'", constant.name, constant.name).unwrap();
    writeln!(s, "export const {} = {{", constant.name).unwrap();

    for entry in &constant.entries {
        if !export_visible(&entry.export, side) { continue; }
        if entry.name.is_empty() { continue; }
        if entry.value.is_empty() { continue; }

        let val = value_to_ts(&entry.value, &entry.tbl_type, sep);
        // 常量导出的是纯值字面量（没有 interface 声明位），说明只能跟在值后面
        let comment = if entry.desc.is_empty() { String::new() } else { format!(" // {}", super::comment_line(&entry.desc)) };
        // 常量也是按属性访问（Xxx.maxLevel），跟字段名同一套大小写规则
        writeln!(s, "    {}: {},{}", super::to_camel_case(&entry.name), val, comment).unwrap();
    }

    writeln!(s, "}} as const;").unwrap();

    s
}

pub fn export_enum_ts(enum_def: &EnumDef) -> String {
    let mut s = String::new();
    writeln!(s, "// 使用：import {{ {}Enum, {}EnumDesc, {}EnumNames }} from './{}'",
        enum_def.name, enum_def.name, enum_def.name, enum_def.name).unwrap();
    let valid: Vec<&EnumEntry> = enum_def.entries.iter()
        .filter(|e| !e.id.is_empty() && !e.name.is_empty())
        .collect();

    let enum_name = format!("{}Enum", enum_def.name);

    writeln!(s, "export enum {} {{", enum_name).unwrap();
    for e in &valid {
        let id = e.id.parse::<i64>().unwrap_or(0);
        writeln!(s, "    /** {} */", super::comment_block(&e.desc)).unwrap();
        writeln!(s, "    {} = {},", e.name, id).unwrap();
    }
    writeln!(s, "}}").unwrap();

    writeln!(s).unwrap();

    writeln!(s, "export const {}Desc: Record<{}, string> = {{", enum_name, enum_name).unwrap();
    for e in &valid {
        writeln!(s, "    [{}.{}]: \"{}\",", enum_name, e.name, ts_escape(&e.desc)).unwrap();
    }
    writeln!(s, "}};").unwrap();

    writeln!(s).unwrap();

    // 值→名反查表。各语言自带的反查能力参差——TS 数值 enum 有原生反向映射但取出来是
    // string，Lua / GDScript 干脆没有——统一给一张 XxxEnumNames（与 Lua / GD 的 names
    // 表口径一致），业务代码跨后端不用换写法。
    writeln!(s, "export const {}Names: Record<{}, string> = {{", enum_name, enum_name).unwrap();
    for e in &valid {
        writeln!(s, "    [{}.{}]: \"{}\",", enum_name, e.name, e.name).unwrap();
    }
    writeln!(s, "}};").unwrap();
    s
}

pub fn export_all_typescript(project: &Project, side: TypeScriptSide) -> Result<super::ExportResult> {
    // TS 属性名走 camelCase，先挡掉转换后撞名（见 check_key_collision）
    super::check_project_key_collisions(project)?;
    let export_cfg = &project.config.export;

    // 按 side 取对应段；fall-back 串：side 段 → 父段（client/server）→ 顶层 export → 默认
    let output = match side {
        TypeScriptSide::Client => export_cfg.client.as_ref()
            .and_then(|c| c.typescript.as_ref())
            .and_then(|t| t.output.as_deref())
            .unwrap_or(side.default_output()),
        TypeScriptSide::Server => export_cfg.server.as_ref()
            .and_then(|s| s.typescript.as_ref())
            .and_then(|t| t.output.as_deref())
            .unwrap_or(side.default_output()),
    };

    let line_ending = LineEnding::from_config(
        export_cfg.line_ending.map(|l| l.as_str())
            .unwrap_or("lf")
    );
    let encoding = export_cfg.encoding.map(|e| e.as_str())
        .unwrap_or("utf-8").to_string();
    let opts = super::ExportOptions { line_ending, encoding };

    let sep = &project.schema.separators;
    let output_dir = project.export_root().join(output);
    let mut collected = Vec::new();

    for group in &project.groups {
        for table in &group.tables {
            if table.deleted { continue; }
            let src = export_table_ts(table, side);
            let file_path = output_dir.join(format!("{}.ts", &table.name));
            collected.push((file_path, src.clone()));
        }

        for constant in &group.constants {
            if constant.deleted { continue; }
            let src = export_constant_ts(constant, sep, side);
            let file_path = output_dir.join(format!("{}.ts", &constant.name));
            collected.push((file_path, src.clone()));
        }

        for enum_def in &group.enums {
            if enum_def.deleted { continue; }
            let src = export_enum_ts(enum_def);
            let file_path = output_dir.join(format!("{}.ts", &enum_def.name));
            collected.push((file_path, src.clone()));
        }
    }

    super::sync_export_dir(&output_dir, "ts", collected, &opts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sep() -> SeparatorsSection { SeparatorsSection::default() }

    #[test]
    fn interface_field_carries_desc_comment() {
        use super::super::test_fixtures::{field, table};
        let t = table("Item", vec![field("stack_max", "堆叠上限", "int")]);
        let out = export_table_ts(&t, TypeScriptSide::Client);
        assert!(out.contains("    /** 堆叠上限 */\n    stackMax: number;"), "got:\n{}", out);
    }

    #[test]
    fn table_key_is_camel_case_in_interface_and_data() {
        // 键名转换必须两边同时发生，interface 和 data 字面量对不上就会静默取到 undefined
        use super::super::test_fixtures::{field, table};
        let mut t = table("Item", vec![field("stack_max", "堆叠上限", "int")]);
        t.records.push(vec!["1".to_string()]);
        let out = export_table_ts(&t, TypeScriptSide::Client);
        assert!(out.contains("    stackMax: number;"), "interface 未转 camelCase:\n{}", out);
        assert!(out.contains("stackMax: 1"), "data 字面量未转 camelCase:\n{}", out);
        assert!(!out.contains("stack_max"), "不该残留源名 snake_case:\n{}", out);
    }

    #[test]
    fn constant_entry_carries_trailing_comment() {
        use super::super::test_fixtures::{constant, entry};
        let c = constant("GameConst", vec![entry("max_level", "最大等级", "int")]);
        let out = export_constant_ts(&c, &sep(), TypeScriptSide::Client);
        assert!(out.contains("    maxLevel: 1, // 最大等级"), "got:\n{}", out);
    }

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
    fn nested_ref_type_is_number_and_value_int() {
        // S24：复杂类型内 ref 元素退化为 number（int id）
        assert_eq!(ts_field_type("List<@Hero>"), "number[]");
        assert_eq!(ts_field_type("Set<@Hero>"), "number[]");
        assert_eq!(ts_field_type("Map<int,@Hero>"), "Record<number, number>");
        let s = sep();
        assert_eq!(value_to_ts("1;2", "List<@Hero>", &s), "[1, 2]");
        assert_eq!(value_to_ts("1:1001;2:1002", "Map<int,@Hero>", &s), "{ 1: 1001, 2: 1002 }");
        assert_eq!(value_to_ts("1001,3", "Tuple2<@Hero,int>", &s), "[1001, 3]");
    }

    #[test]
    fn enum_gen() {
        let e = EnumDef {
            name: "HeroType".to_string(),
            original_name: "HeroType".to_string(),
            path: std::path::PathBuf::new(),
            entries: vec![
                EnumEntry { id: "1".to_string(), name: "Warrior".to_string(), desc: "战士".to_string() },
                EnumEntry { id: "2".to_string(), name: "Mage".to_string(), desc: "法师".to_string() },
            ],
            dirty: false,
            deleted: false,
            original_entries: Vec::new(), saved: true,
            display_field: None,
            original_display_field: None,
        };
        let out = export_enum_ts(&e);
        assert!(out.contains("export enum HeroTypeEnum {"));
        assert!(out.contains("Warrior = 1,"));
        assert!(out.contains("Mage = 2,"));
        assert!(out.contains("export const HeroTypeEnumDesc: Record<HeroTypeEnum, string>"));
        assert!(out.contains("[HeroTypeEnum.Warrior]: \"战士\","));
    }

    #[test]
    fn enum_names_table_is_generated() {
        // 值→名反查表。TS 数值 enum 的原生反向映射取出来是 string，Lua / GD 则完全没有；
        // 统一给一张 Names，七个后端写法一致
        let e = EnumDef {
            name: "HeroType".to_string(),
            original_name: "HeroType".to_string(),
            path: std::path::PathBuf::new(),
            entries: vec![
                EnumEntry { id: "1".to_string(), name: "Warrior".to_string(), desc: "战士".to_string() },
            ],
            dirty: false, deleted: false, original_entries: Vec::new(), saved: true,
            display_field: None,
            original_display_field: None,
        };

        let out = export_enum_ts(&e);
        assert!(out.contains("export const HeroTypeEnumNames: Record<HeroTypeEnum, string>"), "got:\n{}", out);
        assert!(out.contains("[HeroTypeEnum.Warrior]: \"Warrior\","), "got:\n{}", out);
    }

    #[test]
    fn table_header_shows_iteration_idiom() {
        use super::super::test_fixtures::{field, table};
        let t = table("Item", vec![field("id", "物品 ID", "int")]);
        let out = export_table_ts(&t, TypeScriptSide::Client);
        assert!(out.contains("// 遍历：Object.values(Item)"), "got:\n{}", out);
    }

    #[test]
    fn header_usage_comment_uses_esm() {
        use super::super::test_fixtures::{constant, entry, field, table};

        let t = table("Item", vec![field("id", "物品 ID", "int")]);
        let out = export_table_ts(&t, TypeScriptSide::Client);
        assert!(out.contains("// 使用：import { Item, ItemTpl } from './Item'"), "got:\n{}", out);

        let c = constant("GameConst", vec![entry("max_level", "最大等级", "int")]);
        let out = export_constant_ts(&c, &sep(), TypeScriptSide::Client);
        assert!(out.contains("// 使用：import { GameConst } from './GameConst'"), "got:\n{}", out);

        let e = EnumDef {
            name: "HeroType".to_string(),
            original_name: "HeroType".to_string(),
            path: std::path::PathBuf::new(),
            entries: vec![EnumEntry { id: "1".to_string(), name: "Warrior".to_string(), desc: "战士".to_string() }],
            dirty: false, deleted: false, original_entries: Vec::new(), saved: true,
            display_field: None,
            original_display_field: None,
        };
        let out = export_enum_ts(&e);
        assert!(out.contains("// 使用：import { HeroTypeEnum, HeroTypeEnumDesc, HeroTypeEnumNames } from './HeroType'"), "got:\n{}", out);
    }

    #[test]
    fn no_commonjs_artifacts_anywhere() {
        // module_kind 废弃后产物只有 ESM 一种形态，任何文件都不该再出现
        // require / module.exports（@11.3.1 A3）
        use super::super::test_fixtures::{constant, entry, field, table};

        let t = table("Item", vec![field("id", "物品 ID", "int")]);
        let c = constant("GameConst", vec![entry("max_level", "最大等级", "int")]);
        let e = EnumDef {
            name: "HeroType".to_string(),
            original_name: "HeroType".to_string(),
            path: std::path::PathBuf::new(),
            entries: vec![EnumEntry { id: "1".to_string(), name: "Warrior".to_string(), desc: "战士".to_string() }],
            dirty: false, deleted: false, original_entries: Vec::new(), saved: true,
            display_field: None,
            original_display_field: None,
        };

        for src in [
            export_table_ts(&t, TypeScriptSide::Client),
            export_constant_ts(&c, &sep(), TypeScriptSide::Client),
            export_enum_ts(&e),
        ] {
            assert!(!src.contains("module.exports"), "残留 CJS 产物:\n{}", src);
            assert!(!src.contains("require("), "残留 require 调用:\n{}", src);
        }
    }
}
