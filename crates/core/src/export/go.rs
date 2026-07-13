use std::fmt::Write;
use std::collections::HashSet;
use anyhow::Result;
use crate::model::*;
use crate::types::*;
use super::{to_camel_case, to_pascal_case, LineEnding};

const TPL_LOADER: &str = include_str!("../../templates/go/loader.go");
const TPL_PARSER: &str = include_str!("../../templates/go/parser.go");
const TPL_SEP: &str = include_str!("../../templates/go/sep.go");
const TPL_TUPLES: &str = include_str!("../../templates/go/tuples.go");

fn is_server_export(export: &Export) -> bool {
    matches!(export, Export::ClientServer | Export::ServerOnly)
}

// 把 .tbl 文件名 / 字段名转换为 Go 文件名（snake_case）
fn to_snake_case(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 { out.push('_'); }
            for lc in c.to_lowercase() { out.push(lc); }
        } else {
            out.push(c);
        }
    }
    out
}

// 字段在 Go 中的导出名（PascalCase）
fn go_field_name(name: &str) -> String {
    to_pascal_case(name)
}

// 数据文件中的字段 key（与 Java 一致：camelCase）
fn data_key(name: &str) -> String {
    to_camel_case(name)
}

fn go_type_for(tbl_type_str: &str, enum_names: &HashSet<String>) -> String {
    match TblType::parse(tbl_type_str) {
        Some(t) => {
            if t.paradigm == Paradigm::Ref {
                if let Some(name) = &t.ref_name {
                    if enum_names.contains(name) {
                        return format!("{}Enum", name);
                    }
                }
                return "int32".to_string();
            }
            t_go_decl(&t)
        }
        None => "string".to_string(),
    }
}

// 自定义 go 类型表达：Tuple 用泛型结构体，避免裸数组无法承载混合类型
fn t_go_decl(t: &TblType) -> String {
    let p = &t.params;
    match &t.paradigm {
        Paradigm::Base => p[0].go_type().to_string(),
        Paradigm::Tuple2 => format!("Tuple2[{},{}]", p[0].go_type(), p[1].go_type()),
        Paradigm::Tuple3 => format!("Tuple3[{},{},{}]", p[0].go_type(), p[1].go_type(), p[2].go_type()),
        Paradigm::Tuple4 => format!("Tuple4[{},{},{},{}]", p[0].go_type(), p[1].go_type(), p[2].go_type(), p[3].go_type()),
        Paradigm::List => format!("[]{}", p[0].go_type()),
        Paradigm::Set => format!("map[{}]struct{{}}", p[0].go_type()),
        Paradigm::Map => format!("map[{}]{}", p[0].go_type(), p[1].go_type()),
        Paradigm::ListTuple2 => format!("[]Tuple2[{},{}]", p[0].go_type(), p[1].go_type()),
        Paradigm::ListTuple3 => format!("[]Tuple3[{},{},{}]", p[0].go_type(), p[1].go_type(), p[2].go_type()),
        Paradigm::ListTuple4 => format!("[]Tuple4[{},{},{},{}]", p[0].go_type(), p[1].go_type(), p[2].go_type(), p[3].go_type()),
        Paradigm::MapTuple2 => format!("map[{}]Tuple2[{},{}]", p[0].go_type(), p[1].go_type(), p[2].go_type()),
        Paradigm::MapTuple3 => format!("map[{}]Tuple3[{},{},{}]", p[0].go_type(), p[1].go_type(), p[2].go_type(), p[3].go_type()),
        Paradigm::MapTuple4 => format!("map[{}]Tuple4[{},{},{},{}]", p[0].go_type(), p[1].go_type(), p[2].go_type(), p[3].go_type(), p[4].go_type()),
        Paradigm::MapList => format!("map[{}][]{}", p[0].go_type(), p[1].go_type()),
        // 引用类型默认按 table ref 处理，存储 id；enum ref 由调用方根据被引用项 mode 改写
        Paradigm::Ref => "int32".to_string(),
    }
}

// 基础类型的 string→Go 解析函数名
fn base_parse_fn(bt: BaseType) -> &'static str {
    match bt {
        BaseType::Int => "parseInt32",
        BaseType::Long => "parseInt64",
        BaseType::Float => "parseFloat32",
        BaseType::Double => "parseFloat64",
        BaseType::Str => "parseStr",
        BaseType::Bool => "parseBool",
    }
}

// 生成「raw 字符串 → Go 类型」的表达式
// raw_var 是字符串变量名；sep_var 是 SepConfig 变量名
fn parse_expr(raw_var: &str, t: &TblType, sep_var: &str) -> String {
    let p = &t.params;
    match &t.paradigm {
        Paradigm::Base => format!("{}({})", base_parse_fn(p[0]), raw_var),

        Paradigm::Tuple2 => format!(
            "ParseTuple2({}, {}.Tuple2, {}, {})",
            raw_var, sep_var, base_parse_fn(p[0]), base_parse_fn(p[1])
        ),
        Paradigm::Tuple3 => format!(
            "ParseTuple3({}, {}.Tuple3, {}, {}, {})",
            raw_var, sep_var, base_parse_fn(p[0]), base_parse_fn(p[1]), base_parse_fn(p[2])
        ),
        Paradigm::Tuple4 => format!(
            "ParseTuple4({}, {}.Tuple4, {}, {}, {}, {})",
            raw_var, sep_var, base_parse_fn(p[0]), base_parse_fn(p[1]), base_parse_fn(p[2]), base_parse_fn(p[3])
        ),

        Paradigm::List => match p[0] {
            BaseType::Int => format!("parseListInt32({}, {}.List)", raw_var, sep_var),
            BaseType::Long => format!("parseListInt64({}, {}.List)", raw_var, sep_var),
            BaseType::Float => format!("parseListFloat32({}, {}.List)", raw_var, sep_var),
            BaseType::Double => format!("parseListFloat64({}, {}.List)", raw_var, sep_var),
            BaseType::Str => format!("parseListString({}, {}.List)", raw_var, sep_var),
            BaseType::Bool => format!("parseListBool({}, {}.List)", raw_var, sep_var),
        },
        Paradigm::Set => match p[0] {
            BaseType::Int => format!("parseSetInt32({}, {}.Set)", raw_var, sep_var),
            BaseType::Long => format!("parseSetInt64({}, {}.Set)", raw_var, sep_var),
            BaseType::Float => format!("parseSetFloat32({}, {}.Set)", raw_var, sep_var),
            BaseType::Double => format!("parseSetFloat64({}, {}.Set)", raw_var, sep_var),
            BaseType::Str => format!("parseSetString({}, {}.Set)", raw_var, sep_var),
            // bool 集合在实际项目里几乎不会出现，但兜底
            BaseType::Bool => format!("parseSetString({}, {}.Set)", raw_var, sep_var),
        },

        Paradigm::Map => format!(
            "ParseMap({}, {}.MapKv, {}.MapEntry, {}, {})",
            raw_var, sep_var, sep_var, base_parse_fn(p[0]), base_parse_fn(p[1])
        ),

        Paradigm::ListTuple2 => format!(
            "ParseListItems({raw}, {sep}.ListTuple2List, func(s string) Tuple2[{ta},{tb}] {{ return ParseTuple2(s, {sep}.ListTuple2Tuple, {fa}, {fb}) }})",
            raw=raw_var, sep=sep_var,
            ta=p[0].go_type(), tb=p[1].go_type(),
            fa=base_parse_fn(p[0]), fb=base_parse_fn(p[1])
        ),
        Paradigm::ListTuple3 => format!(
            "ParseListItems({raw}, {sep}.ListTuple3List, func(s string) Tuple3[{ta},{tb},{tc}] {{ return ParseTuple3(s, {sep}.ListTuple3Tuple, {fa}, {fb}, {fc}) }})",
            raw=raw_var, sep=sep_var,
            ta=p[0].go_type(), tb=p[1].go_type(), tc=p[2].go_type(),
            fa=base_parse_fn(p[0]), fb=base_parse_fn(p[1]), fc=base_parse_fn(p[2])
        ),
        Paradigm::ListTuple4 => format!(
            "ParseListItems({raw}, {sep}.ListTuple4List, func(s string) Tuple4[{ta},{tb},{tc},{td}] {{ return ParseTuple4(s, {sep}.ListTuple4Tuple, {fa}, {fb}, {fc}, {fd}) }})",
            raw=raw_var, sep=sep_var,
            ta=p[0].go_type(), tb=p[1].go_type(), tc=p[2].go_type(), td=p[3].go_type(),
            fa=base_parse_fn(p[0]), fb=base_parse_fn(p[1]), fc=base_parse_fn(p[2]), fd=base_parse_fn(p[3])
        ),

        Paradigm::MapTuple2 => format!(
            "ParseMap({raw}, {sep}.MapTuple2Kv, {sep}.MapTuple2Entry, {fk}, func(s string) Tuple2[{tb},{tc}] {{ return ParseTuple2(s, {sep}.MapTuple2Tuple, {fb}, {fc}) }})",
            raw=raw_var, sep=sep_var,
            tb=p[1].go_type(), tc=p[2].go_type(),
            fk=base_parse_fn(p[0]), fb=base_parse_fn(p[1]), fc=base_parse_fn(p[2])
        ),
        Paradigm::MapTuple3 => format!(
            "ParseMap({raw}, {sep}.MapTuple3Kv, {sep}.MapTuple3Entry, {fk}, func(s string) Tuple3[{tb},{tc},{td}] {{ return ParseTuple3(s, {sep}.MapTuple3Tuple, {fb}, {fc}, {fd}) }})",
            raw=raw_var, sep=sep_var,
            tb=p[1].go_type(), tc=p[2].go_type(), td=p[3].go_type(),
            fk=base_parse_fn(p[0]), fb=base_parse_fn(p[1]), fc=base_parse_fn(p[2]), fd=base_parse_fn(p[3])
        ),
        Paradigm::MapTuple4 => format!(
            "ParseMap({raw}, {sep}.MapTuple4Kv, {sep}.MapTuple4Entry, {fk}, func(s string) Tuple4[{tb},{tc},{td},{te}] {{ return ParseTuple4(s, {sep}.MapTuple4Tuple, {fb}, {fc}, {fd}, {fe}) }})",
            raw=raw_var, sep=sep_var,
            tb=p[1].go_type(), tc=p[2].go_type(), td=p[3].go_type(), te=p[4].go_type(),
            fk=base_parse_fn(p[0]), fb=base_parse_fn(p[1]), fc=base_parse_fn(p[2]), fd=base_parse_fn(p[3]), fe=base_parse_fn(p[4])
        ),

        Paradigm::MapList => match p[1] {
            BaseType::Int => format!(
                "ParseMap({raw}, {sep}.MapListKv, {sep}.MapListEntry, {fk}, func(s string) []int32 {{ return parseListInt32(s, {sep}.MapListItem) }})",
                raw=raw_var, sep=sep_var, fk=base_parse_fn(p[0])
            ),
            BaseType::Long => format!(
                "ParseMap({raw}, {sep}.MapListKv, {sep}.MapListEntry, {fk}, func(s string) []int64 {{ return parseListInt64(s, {sep}.MapListItem) }})",
                raw=raw_var, sep=sep_var, fk=base_parse_fn(p[0])
            ),
            BaseType::Float => format!(
                "ParseMap({raw}, {sep}.MapListKv, {sep}.MapListEntry, {fk}, func(s string) []float32 {{ return parseListFloat32(s, {sep}.MapListItem) }})",
                raw=raw_var, sep=sep_var, fk=base_parse_fn(p[0])
            ),
            BaseType::Double => format!(
                "ParseMap({raw}, {sep}.MapListKv, {sep}.MapListEntry, {fk}, func(s string) []float64 {{ return parseListFloat64(s, {sep}.MapListItem) }})",
                raw=raw_var, sep=sep_var, fk=base_parse_fn(p[0])
            ),
            BaseType::Str => format!(
                "ParseMap({raw}, {sep}.MapListKv, {sep}.MapListEntry, {fk}, func(s string) []string {{ return parseListString(s, {sep}.MapListItem) }})",
                raw=raw_var, sep=sep_var, fk=base_parse_fn(p[0])
            ),
            BaseType::Bool => format!(
                "ParseMap({raw}, {sep}.MapListKv, {sep}.MapListEntry, {fk}, func(s string) []bool {{ return parseListBool(s, {sep}.MapListItem) }})",
                raw=raw_var, sep=sep_var, fk=base_parse_fn(p[0])
            ),
        },

        // 引用类型默认按 table ref 处理 → int32(id)；enum ref 由调用方在生成时改写
        Paradigm::Ref => format!("parseInt32({})", raw_var),
    }
}

fn parse_expr_for(raw_var: &str, t: &TblType, sep_var: &str, enum_names: &HashSet<String>) -> String {
    if t.paradigm == Paradigm::Ref {
        if let Some(name) = &t.ref_name {
            if enum_names.contains(name) {
                return format!("{}Enum(parseInt32({}))", name, raw_var);
            }
        }
        return format!("parseInt32({})", raw_var);
    }
    parse_expr(raw_var, t, sep_var)
}

// PLACEHOLDER_GO_GEN_TPL

pub fn export_all_go(project: &Project) -> Result<super::ExportResult> {
    let export_cfg = project.config.export.as_ref();
    let server = export_cfg.and_then(|e| e.server.as_ref());
    let go = server.and_then(|s| s.go.as_ref());

    let code_output = go
        .and_then(|g| g.code_output.as_deref())
        .unwrap_or("gen/server/go");
    let pkg = go
        .and_then(|g| g.package.as_deref())
        .unwrap_or("config");

    let line_ending = LineEnding::from_config(
        go.and_then(|g| g.line_ending.map(|l| l.as_str()))
            .or_else(|| server.and_then(|s| s.line_ending.map(|l| l.as_str())))
            .or_else(|| export_cfg.and_then(|e| e.line_ending.map(|l| l.as_str())))
            .unwrap_or("lf")
    );
    let encoding = go.and_then(|g| g.encoding.map(|e| e.as_str()))
        .or_else(|| server.and_then(|s| s.encoding.map(|e| e.as_str())))
        .or_else(|| export_cfg.and_then(|e| e.encoding.map(|e| e.as_str())))
        .unwrap_or("utf-8").to_string();
    let opts = super::ExportOptions { line_ending, encoding };

    let output_dir = project.export_root().join(code_output).join(pkg);
    let mut collected: Vec<(std::path::PathBuf, Vec<u8>)> = Vec::new();

    let render = |tpl: &str| tpl.replace("{{PACKAGE}}", pkg);
    let mut collect = |dir: &std::path::Path, name: &str, content: &str| {
        let path = dir.join(name);
        collected.push((path, opts.encode(content)));
    };

    collect(&output_dir, "loader.go", &render(TPL_LOADER));
    collect(&output_dir, "parser.go", &render(TPL_PARSER));
    collect(&output_dir, "parse_str.go", &render(GEN_PARSE_STR));
    collect(&output_dir, "sep.go", &render(TPL_SEP));
    collect(&output_dir, "tuples.go", &render(TPL_TUPLES));

    let enum_names: HashSet<String> = project.groups.iter()
        .flat_map(|g| g.enums.iter())
        .filter(|e| !e.deleted)
        .map(|e| e.name.clone())
        .collect();

    for group in &project.groups {
        for table in &group.tables {
            if table.deleted { continue; }
            let content = gen_table_tpl(table, pkg, &group.name, &enum_names);
            let filename = format!("{}_tpl.go", to_snake_case(&table.name));
            collect(&output_dir, &filename, &content);
        }

        for constant in &group.constants {
            if constant.deleted { continue; }
            let content = gen_constant_tpl(constant, pkg, &group.name, &enum_names);
            let filename = format!("{}_tpl.go", to_snake_case(&constant.name));
            collect(&output_dir, &filename, &content);
        }

        for enum_def in &group.enums {
            if enum_def.deleted { continue; }
            let content = gen_enum_file(enum_def, pkg);
            let filename = format!("{}_enum.go", to_snake_case(&enum_def.name));
            collect(&output_dir, &filename, &content);
        }
    }

    super::sync_export_dir(&output_dir, "go", collected)
}

const GEN_PARSE_STR: &str = r#"package {{PACKAGE}}

// parseStr 仅是 raw 字符串本身（保留接口对称性以便泛型函数复用）
func parseStr(raw string) string { return raw }
"#;

fn gen_table_tpl(table: &Table, pkg: &str, _group: &str, enum_names: &HashSet<String>) -> String {
    let mut s = String::new();
    let cls = format!("{}Tpl", table.name);
    let key = table.name.clone();

    let fields: Vec<&FieldDef> = table.schema.fields.iter()
        .filter(|f| is_server_export(&f.export))
        .collect();

    writeln!(s, "package {}", pkg).unwrap();
    writeln!(s).unwrap();

    // struct
    writeln!(s, "type {} struct {{", cls).unwrap();
    for f in &fields {
        writeln!(s, "\t{} {} // {}", go_field_name(&f.name), go_type_for(&f.tbl_type, enum_names), f.tbl_type).unwrap();
    }
    writeln!(s, "}}").unwrap();
    writeln!(s).unwrap();

    // store
    writeln!(s, "var {}_data = map[int32]*{}{{}}", table.name, cls).unwrap();
    writeln!(s).unwrap();

    // parse function
    writeln!(s, "func parse{}(row map[string]string, sep SepConfig) *{} {{", table.name, cls).unwrap();
    writeln!(s, "\tobj := &{}{{}}", cls).unwrap();
    for f in &fields {
        let key_name = data_key(&f.name);
        let go_field = go_field_name(&f.name);
        match TblType::parse(&f.tbl_type) {
            Some(t) => {
                writeln!(s, "\tif raw, ok := row[\"{}\"]; ok && raw != \"\" {{", key_name).unwrap();
                writeln!(s, "\t\tobj.{} = {}", go_field, parse_expr_for("raw", &t, "sep", enum_names)).unwrap();
                writeln!(s, "\t}}").unwrap();
            }
            None => {
                writeln!(s, "\tobj.{} = row[\"{}\"]", go_field, key_name).unwrap();
            }
        }
    }
    writeln!(s, "\treturn obj").unwrap();
    writeln!(s, "}}").unwrap();
    writeln!(s).unwrap();

    // init register
    writeln!(s, "func init() {{").unwrap();
    writeln!(s, "\tregisterTable(\"{}\", func(rows []map[string]string, sep SepConfig) {{", key).unwrap();
    writeln!(s, "\t\tm := map[int32]*{}{{}}", cls).unwrap();
    writeln!(s, "\t\tfor _, row := range rows {{").unwrap();
    writeln!(s, "\t\t\tobj := parse{}(row, sep)", table.name).unwrap();
    writeln!(s, "\t\t\tm[obj.Id] = obj").unwrap();
    writeln!(s, "\t\t}}").unwrap();
    writeln!(s, "\t\t{}_data = m", table.name).unwrap();
    writeln!(s, "\t}})").unwrap();
    writeln!(s, "}}").unwrap();
    writeln!(s).unwrap();

    // accessor functions
    writeln!(s, "func Get{}(id int32) *{} {{ return {}_data[id] }}", table.name, cls, table.name).unwrap();
    writeln!(s).unwrap();
    writeln!(s, "func GetAll{}() map[int32]*{} {{ return {}_data }}", table.name, cls, table.name).unwrap();

    s
}

fn gen_constant_tpl(constant: &Constant, pkg: &str, _group: &str, enum_names: &HashSet<String>) -> String {
    let mut s = String::new();
    let cls = format!("{}Tpl", constant.name);
    let key = constant.name.clone();

    let entries: Vec<&ConstEntry> = constant.entries.iter()
        .filter(|e| is_server_export(&e.export) && !e.name.is_empty())
        .collect();

    writeln!(s, "package {}", pkg).unwrap();
    writeln!(s).unwrap();

    // struct
    writeln!(s, "type {} struct {{", cls).unwrap();
    for e in &entries {
        writeln!(s, "\t{} {} // {}", go_field_name(&e.name), go_type_for(&e.tbl_type, enum_names), e.tbl_type).unwrap();
    }
    writeln!(s, "}}").unwrap();
    writeln!(s).unwrap();

    // store
    writeln!(s, "var {}_data = &{}{{}}", constant.name, cls).unwrap();
    writeln!(s).unwrap();

    // parse function
    writeln!(s, "func parse{}(row map[string]string, sep SepConfig) *{} {{", constant.name, cls).unwrap();
    writeln!(s, "\tobj := &{}{{}}", cls).unwrap();
    for e in &entries {
        let key_name = data_key(&e.name);
        let go_field = go_field_name(&e.name);
        match TblType::parse(&e.tbl_type) {
            Some(t) => {
                writeln!(s, "\tif raw, ok := row[\"{}\"]; ok && raw != \"\" {{", key_name).unwrap();
                writeln!(s, "\t\tobj.{} = {}", go_field, parse_expr_for("raw", &t, "sep", enum_names)).unwrap();
                writeln!(s, "\t}}").unwrap();
            }
            None => {
                writeln!(s, "\tobj.{} = row[\"{}\"]", go_field, key_name).unwrap();
            }
        }
    }
    writeln!(s, "\treturn obj").unwrap();
    writeln!(s, "}}").unwrap();
    writeln!(s).unwrap();

    // init register
    writeln!(s, "func init() {{").unwrap();
    writeln!(s, "\tregisterConstant(\"{}\", func(row map[string]string, sep SepConfig) {{", key).unwrap();
    writeln!(s, "\t\t{}_data = parse{}(row, sep)", constant.name, constant.name).unwrap();
    writeln!(s, "\t}})").unwrap();
    writeln!(s, "}}").unwrap();
    writeln!(s).unwrap();

    // accessor function
    writeln!(s, "func Get{}() *{} {{ return {}_data }}", constant.name, cls, constant.name).unwrap();

    s
}

fn gen_enum_file(enum_def: &EnumDef, pkg: &str) -> String {
    let mut s = String::new();
    let type_name = format!("{}Enum", enum_def.name);

    let valid: Vec<&EnumEntry> = enum_def.entries.iter()
        .filter(|e| !e.id.is_empty() && !e.name.is_empty())
        .collect();

    writeln!(s, "package {}", pkg).unwrap();
    writeln!(s).unwrap();
    writeln!(s, "type {} int32", type_name).unwrap();
    writeln!(s).unwrap();

    let max_name_len = valid.iter()
        .map(|e| e.name.len())
        .max()
        .unwrap_or(0);

    writeln!(s, "const (").unwrap();
    for e in &valid {
        let pad = " ".repeat(max_name_len - e.name.len());
        writeln!(s, "\t{}_{}{} {} = {} // {}",
            type_name, e.name, pad, type_name, e.id, e.desc).unwrap();
    }
    writeln!(s, ")").unwrap();
    writeln!(s).unwrap();

    let desc_var = format!("{}{}Desc", lowercase_first(&enum_def.name), "Enum");
    let by_name_var = format!("{}{}ByName", lowercase_first(&enum_def.name), "Enum");

    writeln!(s, "var {} = map[{}]string{{", desc_var, type_name).unwrap();
    for e in &valid {
        let pad = " ".repeat(max_name_len - e.name.len());
        writeln!(s, "\t{}_{}:{} \"{}\",", type_name, e.name, pad, escape_go_str(&e.desc)).unwrap();
    }
    writeln!(s, "}}").unwrap();
    writeln!(s).unwrap();

    writeln!(s, "var {} = map[string]{}{{", by_name_var, type_name).unwrap();
    for e in &valid {
        let pad = " ".repeat(max_name_len - e.name.len());
        writeln!(s, "\t\"{}\":{} {}_{},", e.name, pad, type_name, e.name).unwrap();
    }
    writeln!(s, "}}").unwrap();
    writeln!(s).unwrap();

    writeln!(s, "func (h {}) Desc() string {{ return {}[h] }}", type_name, desc_var).unwrap();
    writeln!(s).unwrap();

    writeln!(s, "func (h {}) String() string {{", type_name).unwrap();
    writeln!(s, "\tfor name, val := range {} {{", by_name_var).unwrap();
    writeln!(s, "\t\tif val == h {{").unwrap();
    writeln!(s, "\t\t\treturn name").unwrap();
    writeln!(s, "\t\t}}").unwrap();
    writeln!(s, "\t}}").unwrap();
    writeln!(s, "\treturn \"\"").unwrap();
    writeln!(s, "}}").unwrap();
    writeln!(s).unwrap();

    writeln!(s, "func Parse{}(name string) ({}, bool) {{", type_name, type_name).unwrap();
    writeln!(s, "\tv, ok := {}[name]", by_name_var).unwrap();
    writeln!(s, "\treturn v, ok").unwrap();
    writeln!(s, "}}").unwrap();

    s
}

fn lowercase_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_ascii_lowercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

fn escape_go_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}
