//! C++ 服务端导出（分离模式）。
//!
//! 形态：每张表 / 常量 / 枚举一个 `<Name>Tpl.h`（头文件即代码）。
//! 数据复用 json.rs 输出的 `gen/server/data/json/*.json`。
//! 用户在自己的 main.cpp 里：
//!   #define TBL_JSON_LIB_NLOHMANN
//!   #include "all_tpls.h"
//!   tbl::init_json("gen/server/data/json");
//!   auto* hero = tbl::HeroBaseTpl::Get(1);
//!
//! 设计要点：
//! - C++17 + nlohmann/json (header-only)
//! - 模板 / `inline` 变量 → 整个生成结果是头文件，避免 .cpp 链接问题
//! - 每个 `<Name>Tpl.h` 通过文件作用域静态对象（`TableRegister` 实例）把自己注册进 registry
//! - Ref 字段：保留 int(id)；@EnumName Ref 改写成对应 `EnumName_Enum`
//! - SeparatorsSection 在运行时从 JSON 的 `_sep` 段读出（而非编译时常量）

use std::collections::HashSet;
use std::fmt::Write;
use anyhow::Result;
use crate::model::*;
use crate::types::*;
use super::{to_camel_case, LineEnding};

const TPL_TBL_H: &str = include_str!("../../templates/cpp/tbl.h");
const TPL_LOADER_H: &str = include_str!("../../templates/cpp/loader.h");

fn is_server_export(export: &Export) -> bool {
    matches!(export, Export::ClientServer | Export::ServerOnly)
}

fn cpp_base_type(bt: BaseType) -> &'static str {
    match bt {
        BaseType::Int => "int32_t",
        BaseType::Long => "int64_t",
        BaseType::Float => "float",
        BaseType::Double => "double",
        BaseType::Str => "std::string",
        BaseType::Bool => "bool",
        BaseType::Txt => "std::string",
    }
}

fn cpp_base_default(bt: BaseType) -> &'static str {
    match bt {
        BaseType::Int | BaseType::Long => "0",
        BaseType::Float | BaseType::Double => "0",
        BaseType::Str => "{}",
        BaseType::Bool => "false",
        BaseType::Txt => "{}",
    }
}

fn cpp_base_parse_fn(bt: BaseType) -> &'static str {
    match bt {
        BaseType::Int => "parse_int32",
        BaseType::Long => "parse_int64",
        BaseType::Float => "parse_float",
        BaseType::Double => "parse_double",
        BaseType::Str => "parse_str",
        BaseType::Bool => "parse_bool",
        BaseType::Txt => "parse_str",
    }
}

fn cpp_field_type(tbl_type_str: &str, enum_names: &HashSet<String>) -> String {
    let Some(t) = TblType::parse(tbl_type_str) else { return "std::string".to_string() };
    let p = &t.params;
    match &t.paradigm {
        Paradigm::Base => cpp_base_type(p[0]).to_string(),
        Paradigm::Tuple2 => format!("Tuple2<{}, {}>", cpp_base_type(p[0]), cpp_base_type(p[1])),
        Paradigm::Tuple3 => format!("Tuple3<{}, {}, {}>", cpp_base_type(p[0]), cpp_base_type(p[1]), cpp_base_type(p[2])),
        Paradigm::Tuple4 => format!("Tuple4<{}, {}, {}, {}>", cpp_base_type(p[0]), cpp_base_type(p[1]), cpp_base_type(p[2]), cpp_base_type(p[3])),
        Paradigm::List => format!("std::vector<{}>", cpp_base_type(p[0])),
        Paradigm::Set => format!("std::unordered_set<{}>", cpp_base_type(p[0])),
        Paradigm::Map => format!("std::unordered_map<{}, {}>", cpp_base_type(p[0]), cpp_base_type(p[1])),
        Paradigm::ListTuple2 => format!("std::vector<Tuple2<{}, {}>>", cpp_base_type(p[0]), cpp_base_type(p[1])),
        Paradigm::ListTuple3 => format!("std::vector<Tuple3<{}, {}, {}>>", cpp_base_type(p[0]), cpp_base_type(p[1]), cpp_base_type(p[2])),
        Paradigm::ListTuple4 => format!("std::vector<Tuple4<{}, {}, {}, {}>>", cpp_base_type(p[0]), cpp_base_type(p[1]), cpp_base_type(p[2]), cpp_base_type(p[3])),
        Paradigm::MapTuple2 => format!("std::unordered_map<{}, Tuple2<{}, {}>>", cpp_base_type(p[0]), cpp_base_type(p[1]), cpp_base_type(p[2])),
        Paradigm::MapTuple3 => format!("std::unordered_map<{}, Tuple3<{}, {}, {}>>", cpp_base_type(p[0]), cpp_base_type(p[1]), cpp_base_type(p[2]), cpp_base_type(p[3])),
        Paradigm::MapTuple4 => format!("std::unordered_map<{}, Tuple4<{}, {}, {}, {}>>", cpp_base_type(p[0]), cpp_base_type(p[1]), cpp_base_type(p[2]), cpp_base_type(p[3]), cpp_base_type(p[4])),
        Paradigm::MapList => format!("std::unordered_map<{}, std::vector<{}>>", cpp_base_type(p[0]), cpp_base_type(p[1])),
        Paradigm::Ref => {
            if let Some(name) = &t.ref_name {
                if enum_names.contains(name) {
                    return format!("{}Enum", name);
                }
            }
            "int32_t".to_string()
        }
    }
}

/// raw 字符串变量名 → C++ 表达式（结果是该字段的目标类型）。`sep` 是 `SepConfig` 变量名。
fn cpp_parse_expr(raw_var: &str, t: &TblType, sep: &str, enum_names: &HashSet<String>) -> String {
    let p = &t.params;
    match &t.paradigm {
        Paradigm::Base => format!("{}({})", cpp_base_parse_fn(p[0]), raw_var),

        Paradigm::Tuple2 => format!(
            "parse_tuple2<{ta}, {tb}>({raw}, {sep}.tuple2, {fa}, {fb})",
            ta=cpp_base_type(p[0]), tb=cpp_base_type(p[1]),
            raw=raw_var, sep=sep,
            fa=cpp_base_parse_fn(p[0]), fb=cpp_base_parse_fn(p[1])
        ),
        Paradigm::Tuple3 => format!(
            "parse_tuple3<{ta}, {tb}, {tc}>({raw}, {sep}.tuple3, {fa}, {fb}, {fc})",
            ta=cpp_base_type(p[0]), tb=cpp_base_type(p[1]), tc=cpp_base_type(p[2]),
            raw=raw_var, sep=sep,
            fa=cpp_base_parse_fn(p[0]), fb=cpp_base_parse_fn(p[1]), fc=cpp_base_parse_fn(p[2])
        ),
        Paradigm::Tuple4 => format!(
            "parse_tuple4<{ta}, {tb}, {tc}, {td}>({raw}, {sep}.tuple4, {fa}, {fb}, {fc}, {fd})",
            ta=cpp_base_type(p[0]), tb=cpp_base_type(p[1]), tc=cpp_base_type(p[2]), td=cpp_base_type(p[3]),
            raw=raw_var, sep=sep,
            fa=cpp_base_parse_fn(p[0]), fb=cpp_base_parse_fn(p[1]), fc=cpp_base_parse_fn(p[2]), fd=cpp_base_parse_fn(p[3])
        ),

        Paradigm::List => format!(
            "parse_list<{t}>({raw}, {sep}.list, {f})",
            t=cpp_base_type(p[0]), raw=raw_var, sep=sep, f=cpp_base_parse_fn(p[0])
        ),
        Paradigm::Set => format!(
            "parse_set<{t}>({raw}, {sep}.set, {f})",
            t=cpp_base_type(p[0]), raw=raw_var, sep=sep, f=cpp_base_parse_fn(p[0])
        ),
        Paradigm::Map => format!(
            "parse_map<{tk}, {tv}>({raw}, {sep}.map_kv, {sep}.map_entry, {fk}, {fv})",
            tk=cpp_base_type(p[0]), tv=cpp_base_type(p[1]),
            raw=raw_var, sep=sep,
            fk=cpp_base_parse_fn(p[0]), fv=cpp_base_parse_fn(p[1])
        ),

        Paradigm::ListTuple2 => format!(
            "parse_list<Tuple2<{ta}, {tb}>>({raw}, {sep}.list_tuple2_list, std::function<Tuple2<{ta}, {tb}>(const std::string&)>([&](const std::string& s) {{ return parse_tuple2<{ta}, {tb}>(s, {sep}.list_tuple2_tuple, {fa}, {fb}); }}))",
            ta=cpp_base_type(p[0]), tb=cpp_base_type(p[1]),
            raw=raw_var, sep=sep,
            fa=cpp_base_parse_fn(p[0]), fb=cpp_base_parse_fn(p[1])
        ),
        Paradigm::ListTuple3 => format!(
            "parse_list<Tuple3<{ta}, {tb}, {tc}>>({raw}, {sep}.list_tuple3_list, std::function<Tuple3<{ta}, {tb}, {tc}>(const std::string&)>([&](const std::string& s) {{ return parse_tuple3<{ta}, {tb}, {tc}>(s, {sep}.list_tuple3_tuple, {fa}, {fb}, {fc}); }}))",
            ta=cpp_base_type(p[0]), tb=cpp_base_type(p[1]), tc=cpp_base_type(p[2]),
            raw=raw_var, sep=sep,
            fa=cpp_base_parse_fn(p[0]), fb=cpp_base_parse_fn(p[1]), fc=cpp_base_parse_fn(p[2])
        ),
        Paradigm::ListTuple4 => format!(
            "parse_list<Tuple4<{ta}, {tb}, {tc}, {td}>>({raw}, {sep}.list_tuple4_list, std::function<Tuple4<{ta}, {tb}, {tc}, {td}>(const std::string&)>([&](const std::string& s) {{ return parse_tuple4<{ta}, {tb}, {tc}, {td}>(s, {sep}.list_tuple4_tuple, {fa}, {fb}, {fc}, {fd}); }}))",
            ta=cpp_base_type(p[0]), tb=cpp_base_type(p[1]), tc=cpp_base_type(p[2]), td=cpp_base_type(p[3]),
            raw=raw_var, sep=sep,
            fa=cpp_base_parse_fn(p[0]), fb=cpp_base_parse_fn(p[1]), fc=cpp_base_parse_fn(p[2]), fd=cpp_base_parse_fn(p[3])
        ),

        Paradigm::MapTuple2 => format!(
            "parse_map<{tk}, Tuple2<{ta}, {tb}>>({raw}, {sep}.map_tuple2_kv, {sep}.map_tuple2_entry, {fk}, std::function<Tuple2<{ta}, {tb}>(const std::string&)>([&](const std::string& s) {{ return parse_tuple2<{ta}, {tb}>(s, {sep}.map_tuple2_tuple, {fa}, {fb}); }}))",
            tk=cpp_base_type(p[0]), ta=cpp_base_type(p[1]), tb=cpp_base_type(p[2]),
            raw=raw_var, sep=sep,
            fk=cpp_base_parse_fn(p[0]), fa=cpp_base_parse_fn(p[1]), fb=cpp_base_parse_fn(p[2])
        ),
        Paradigm::MapTuple3 => format!(
            "parse_map<{tk}, Tuple3<{ta}, {tb}, {tc}>>({raw}, {sep}.map_tuple3_kv, {sep}.map_tuple3_entry, {fk}, std::function<Tuple3<{ta}, {tb}, {tc}>(const std::string&)>([&](const std::string& s) {{ return parse_tuple3<{ta}, {tb}, {tc}>(s, {sep}.map_tuple3_tuple, {fa}, {fb}, {fc}); }}))",
            tk=cpp_base_type(p[0]), ta=cpp_base_type(p[1]), tb=cpp_base_type(p[2]), tc=cpp_base_type(p[3]),
            raw=raw_var, sep=sep,
            fk=cpp_base_parse_fn(p[0]), fa=cpp_base_parse_fn(p[1]), fb=cpp_base_parse_fn(p[2]), fc=cpp_base_parse_fn(p[3])
        ),
        Paradigm::MapTuple4 => format!(
            "parse_map<{tk}, Tuple4<{ta}, {tb}, {tc}, {td}>>({raw}, {sep}.map_tuple4_kv, {sep}.map_tuple4_entry, {fk}, std::function<Tuple4<{ta}, {tb}, {tc}, {td}>(const std::string&)>([&](const std::string& s) {{ return parse_tuple4<{ta}, {tb}, {tc}, {td}>(s, {sep}.map_tuple4_tuple, {fa}, {fb}, {fc}, {fd}); }}))",
            tk=cpp_base_type(p[0]), ta=cpp_base_type(p[1]), tb=cpp_base_type(p[2]), tc=cpp_base_type(p[3]), td=cpp_base_type(p[4]),
            raw=raw_var, sep=sep,
            fk=cpp_base_parse_fn(p[0]), fa=cpp_base_parse_fn(p[1]), fb=cpp_base_parse_fn(p[2]), fc=cpp_base_parse_fn(p[3]), fd=cpp_base_parse_fn(p[4])
        ),

        Paradigm::MapList => format!(
            "parse_map<{tk}, std::vector<{tv}>>({raw}, {sep}.map_list_kv, {sep}.map_list_entry, {fk}, std::function<std::vector<{tv}>(const std::string&)>([&](const std::string& s) {{ return parse_list<{tv}>(s, {sep}.map_list_item, {fv}); }}))",
            tk=cpp_base_type(p[0]), tv=cpp_base_type(p[1]),
            raw=raw_var, sep=sep,
            fk=cpp_base_parse_fn(p[0]), fv=cpp_base_parse_fn(p[1])
        ),

        Paradigm::Ref => {
            if let Some(name) = &t.ref_name {
                if enum_names.contains(name) {
                    return format!("static_cast<{}Enum>(parse_int32({}))", name, raw_var);
                }
            }
            format!("parse_int32({})", raw_var)
        }
    }
}

fn gen_table_header(table: &Table, ns: &str, enum_names: &HashSet<String>) -> String {
    let mut s = String::new();
    let cls = format!("{}Tpl", table.name);

    let fields: Vec<&FieldDef> = table.schema.fields.iter()
        .filter(|f| is_server_export(&f.export))
        .collect();

    writeln!(s, "// Auto-generated by tablet. Do not edit.").unwrap();
    writeln!(s, "#pragma once").unwrap();
    writeln!(s).unwrap();
    writeln!(s, "#include \"loader.h\"").unwrap();
    // 包含本项目所有 enum 头（简化引用——避免按字段精确包含）
    writeln!(s).unwrap();
    writeln!(s, "namespace {} {{", ns).unwrap();
    writeln!(s).unwrap();

    writeln!(s, "struct {} {{", cls).unwrap();
    for f in &fields {
        let typ = cpp_field_type(&f.tbl_type, enum_names);
        let name = to_camel_case(&f.name);
        // 给 base 类型加默认值，复合类型用 {} 兜底
        let default = match TblType::parse(&f.tbl_type) {
            Some(t) if t.paradigm == Paradigm::Base => cpp_base_default(t.params[0]).to_string(),
            Some(t) if t.paradigm == Paradigm::Ref => {
                if let Some(rn) = &t.ref_name {
                    if enum_names.contains(rn) { "{}".to_string() } else { "0".to_string() }
                } else { "0".to_string() }
            }
            _ => "{}".to_string(),
        };
        writeln!(s, "    {} {} = {};", typ, name, default).unwrap();
    }
    writeln!(s).unwrap();
    writeln!(s, "    static const {}* Get(int32_t id);", cls).unwrap();
    writeln!(s, "    static const std::unordered_map<int32_t, {}>& All();", cls).unwrap();
    writeln!(s, "}};").unwrap();
    writeln!(s).unwrap();

    // 注册 + 存储（inline 变量需 C++17）
    writeln!(s, "inline std::unordered_map<int32_t, {}>& {}_data() {{", cls, table.name).unwrap();
    writeln!(s, "    static std::unordered_map<int32_t, {}> m; return m;", cls).unwrap();
    writeln!(s, "}}").unwrap();
    writeln!(s).unwrap();

    writeln!(s, "inline const {}* {}::Get(int32_t id) {{", cls, cls).unwrap();
    writeln!(s, "    auto& m = {}_data();", table.name).unwrap();
    writeln!(s, "    auto it = m.find(id);").unwrap();
    writeln!(s, "    return it == m.end() ? nullptr : &it->second;").unwrap();
    writeln!(s, "}}").unwrap();
    writeln!(s, "inline const std::unordered_map<int32_t, {}>& {}::All() {{ return {}_data(); }}", cls, cls, table.name).unwrap();
    writeln!(s).unwrap();

    // 解析单行函数
    writeln!(s, "inline {} parse_{}(const std::unordered_map<std::string, std::string>& row, const SepConfig& sep) {{", cls, table.name).unwrap();
    writeln!(s, "    {} obj;", cls).unwrap();
    writeln!(s, "    auto get = [&](const char* k) -> std::string {{ auto it = row.find(k); return it == row.end() ? std::string() : it->second; }};").unwrap();
    for f in &fields {
        let camel = to_camel_case(&f.name);
        let key = camel.clone();
        match TblType::parse(&f.tbl_type) {
            Some(t) => {
                writeln!(s, "    {{ auto raw = get(\"{}\"); if (!raw.empty()) obj.{} = {}; }}",
                    key, camel, cpp_parse_expr("raw", &t, "sep", enum_names)).unwrap();
            }
            None => {
                writeln!(s, "    obj.{} = get(\"{}\");", camel, key).unwrap();
            }
        }
    }
    writeln!(s, "    return obj;").unwrap();
    writeln!(s, "}}").unwrap();
    writeln!(s).unwrap();

    // 静态注册：加载时把 rows 解析成 map<id, obj>
    writeln!(s, "namespace {{ struct {}Reg {{ {}Reg() {{", table.name, table.name).unwrap();
    writeln!(s, "    table_registry()[\"{}\"] = [](const std::vector<std::unordered_map<std::string, std::string>>& rows, const SepConfig& sep) {{",
        table.name).unwrap();
    writeln!(s, "        auto& m = {}_data(); m.clear();", table.name).unwrap();
    writeln!(s, "        for (const auto& r : rows) {{").unwrap();
    writeln!(s, "            auto obj = parse_{}(r, sep);", table.name).unwrap();
    writeln!(s, "            m.emplace(obj.id, std::move(obj));").unwrap();
    writeln!(s, "        }}").unwrap();
    writeln!(s, "    }};").unwrap();
    writeln!(s, "}} }}; inline {}Reg {}_reg_instance; }}", table.name, table.name).unwrap();
    writeln!(s).unwrap();

    writeln!(s, "}}  // namespace {}", ns).unwrap();
    s
}

fn gen_constant_header(constant: &Constant, ns: &str, enum_names: &HashSet<String>) -> String {
    let mut s = String::new();
    let cls = format!("{}Tpl", constant.name);

    let entries: Vec<&ConstEntry> = constant.entries.iter()
        .filter(|e| is_server_export(&e.export) && !e.name.is_empty())
        .collect();

    writeln!(s, "// Auto-generated by tablet. Do not edit.").unwrap();
    writeln!(s, "#pragma once").unwrap();
    writeln!(s).unwrap();
    writeln!(s, "#include \"loader.h\"").unwrap();
    writeln!(s).unwrap();
    writeln!(s, "namespace {} {{", ns).unwrap();
    writeln!(s).unwrap();

    writeln!(s, "struct {} {{", cls).unwrap();
    for e in &entries {
        let typ = cpp_field_type(&e.tbl_type, enum_names);
        let name = to_camel_case(&e.name);
        let default = match TblType::parse(&e.tbl_type) {
            Some(t) if t.paradigm == Paradigm::Base => cpp_base_default(t.params[0]).to_string(),
            Some(t) if t.paradigm == Paradigm::Ref => {
                if let Some(rn) = &t.ref_name {
                    if enum_names.contains(rn) { "{}".to_string() } else { "0".to_string() }
                } else { "0".to_string() }
            }
            _ => "{}".to_string(),
        };
        writeln!(s, "    {} {} = {};", typ, name, default).unwrap();
    }
    writeln!(s).unwrap();
    writeln!(s, "    static const {}& Get();", cls).unwrap();
    writeln!(s, "}};").unwrap();
    writeln!(s).unwrap();

    writeln!(s, "inline {}& {}_data() {{ static {} v; return v; }}", cls, constant.name, cls).unwrap();
    writeln!(s, "inline const {}& {}::Get() {{ return {}_data(); }}", cls, cls, constant.name).unwrap();
    writeln!(s).unwrap();

    writeln!(s, "inline {} parse_{}(const std::unordered_map<std::string, std::string>& row, const SepConfig& sep) {{", cls, constant.name).unwrap();
    writeln!(s, "    {} obj;", cls).unwrap();
    writeln!(s, "    auto get = [&](const char* k) -> std::string {{ auto it = row.find(k); return it == row.end() ? std::string() : it->second; }};").unwrap();
    for e in &entries {
        let camel = to_camel_case(&e.name);
        let key = camel.clone();
        match TblType::parse(&e.tbl_type) {
            Some(t) => {
                writeln!(s, "    {{ auto raw = get(\"{}\"); if (!raw.empty()) obj.{} = {}; }}",
                    key, camel, cpp_parse_expr("raw", &t, "sep", enum_names)).unwrap();
            }
            None => {
                writeln!(s, "    obj.{} = get(\"{}\");", camel, key).unwrap();
            }
        }
    }
    writeln!(s, "    return obj;").unwrap();
    writeln!(s, "}}").unwrap();
    writeln!(s).unwrap();

    writeln!(s, "namespace {{ struct {}Reg {{ {}Reg() {{", constant.name, constant.name).unwrap();
    writeln!(s, "    constant_registry()[\"{}\"] = [](const std::unordered_map<std::string, std::string>& row, const SepConfig& sep) {{",
        constant.name).unwrap();
    writeln!(s, "        {}_data() = parse_{}(row, sep);", constant.name, constant.name).unwrap();
    writeln!(s, "    }};").unwrap();
    writeln!(s, "}} }}; inline {}Reg {}_reg_instance; }}", constant.name, constant.name).unwrap();
    writeln!(s).unwrap();

    writeln!(s, "}}  // namespace {}", ns).unwrap();
    s
}

fn gen_enum_header(enum_def: &EnumDef, ns: &str) -> String {
    let mut s = String::new();
    let type_name = format!("{}Enum", enum_def.name);

    let valid: Vec<&EnumEntry> = enum_def.entries.iter()
        .filter(|e| !e.id.is_empty() && !e.name.is_empty())
        .collect();

    writeln!(s, "// Auto-generated by tablet. Do not edit.").unwrap();
    writeln!(s, "#pragma once").unwrap();
    writeln!(s).unwrap();
    writeln!(s, "#include \"tbl.h\"").unwrap();
    writeln!(s).unwrap();
    writeln!(s, "namespace {} {{", ns).unwrap();
    writeln!(s).unwrap();

    writeln!(s, "enum class {} : int32_t {{", type_name).unwrap();
    for e in &valid {
        let id = e.id.parse::<i64>().unwrap_or(0);
        writeln!(s, "    {} = {},  // {}", e.name, id, e.desc).unwrap();
    }
    writeln!(s, "}};").unwrap();
    writeln!(s).unwrap();

    writeln!(s, "inline const char* {}_desc({} v) {{", type_name, type_name).unwrap();
    writeln!(s, "    switch (v) {{").unwrap();
    for e in &valid {
        writeln!(s, "        case {}::{}: return \"{}\";", type_name, e.name, escape_cpp(&e.desc)).unwrap();
    }
    writeln!(s, "    }}").unwrap();
    writeln!(s, "    return \"\";").unwrap();
    writeln!(s, "}}").unwrap();
    writeln!(s).unwrap();

    writeln!(s, "}}  // namespace {}", ns).unwrap();
    s
}

fn escape_cpp(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// `all_tpls.h`：聚合 include —— 用户只需 `#include "all_tpls.h"` 即吃所有模板。
fn gen_all_header(ns: &str, table_names: &[String], constant_names: &[String], enum_names: &[String]) -> String {
    let mut s = String::new();
    writeln!(s, "// Auto-generated by tablet. Do not edit.").unwrap();
    writeln!(s, "#pragma once").unwrap();
    writeln!(s).unwrap();
    writeln!(s, "// 聚合头：包含本项目所有 *Tpl.h / *Enum.h。").unwrap();
    writeln!(s, "#include \"loader.h\"").unwrap();
    for name in enum_names {
        writeln!(s, "#include \"{}Enum.h\"", name).unwrap();
    }
    for name in table_names {
        writeln!(s, "#include \"{}Tpl.h\"", name).unwrap();
    }
    for name in constant_names {
        writeln!(s, "#include \"{}Tpl.h\"", name).unwrap();
    }
    writeln!(s).unwrap();
    writeln!(s, "namespace {} {{}}  // 仅占位让 IDE 知道 ns 存在", ns).unwrap();
    s
}

pub fn export_all_cpp(project: &Project) -> Result<super::ExportResult> {
    let export_cfg = &project.config.export;
    let server = export_cfg.server.as_ref();
    let cpp = server.and_then(|s| s.cpp.as_ref());

    let code_output = cpp
        .and_then(|c| c.code_output.as_deref())
        .unwrap_or("gen/server/cpp");
    let ns = cpp
        .and_then(|c| c.namespace.as_deref())
        .unwrap_or("game::config");

    let line_ending = LineEnding::from_config(
        export_cfg.line_ending.map(|l| l.as_str())
            .unwrap_or("lf")
    );
    let encoding = export_cfg.encoding.map(|e| e.as_str())
        .unwrap_or("utf-8").to_string();
    let opts = super::ExportOptions { line_ending, encoding };

    let output_dir = project.export_root().join(code_output);
    let mut collected: Vec<(std::path::PathBuf, Vec<u8>)> = Vec::new();

    let render = |tpl: &str| tpl.replace("{{NAMESPACE}}", ns);
    let mut collect = |dir: &std::path::Path, name: &str, content: &str| {
        collected.push((dir.join(name), opts.encode(content)));
    };

    collect(&output_dir, "tbl.h", &render(TPL_TBL_H));
    collect(&output_dir, "loader.h", &render(TPL_LOADER_H));

    let enum_names: HashSet<String> = project.groups.iter()
        .flat_map(|g| g.enums.iter())
        .filter(|e| !e.deleted)
        .map(|e| e.name.clone())
        .collect();

    let mut all_tables: Vec<String> = Vec::new();
    let mut all_constants: Vec<String> = Vec::new();
    let mut all_enums: Vec<String> = Vec::new();

    for group in &project.groups {
        for table in &group.tables {
            if table.deleted { continue; }
            let content = gen_table_header(table, ns, &enum_names);
            collect(&output_dir, &format!("{}Tpl.h", table.name), &content);
            all_tables.push(table.name.clone());
        }
        for constant in &group.constants {
            if constant.deleted { continue; }
            let content = gen_constant_header(constant, ns, &enum_names);
            collect(&output_dir, &format!("{}Tpl.h", constant.name), &content);
            all_constants.push(constant.name.clone());
        }
        for enum_def in &group.enums {
            if enum_def.deleted { continue; }
            let content = gen_enum_header(enum_def, ns);
            collect(&output_dir, &format!("{}Enum.h", enum_def.name), &content);
            all_enums.push(enum_def.name.clone());
        }
    }

    let all_h = gen_all_header(ns, &all_tables, &all_constants, &all_enums);
    collect(&output_dir, "all_tpls.h", &all_h);

    super::sync_export_dir(&output_dir, "h", collected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn no_enums() -> HashSet<String> { HashSet::new() }

    #[test]
    fn base_types() {
        assert_eq!(cpp_base_type(BaseType::Int), "int32_t");
        assert_eq!(cpp_base_type(BaseType::Long), "int64_t");
        assert_eq!(cpp_base_type(BaseType::Float), "float");
        assert_eq!(cpp_base_type(BaseType::Str), "std::string");
        assert_eq!(cpp_base_type(BaseType::Bool), "bool");
    }

    #[test]
    fn field_type_paradigms() {
        let e = no_enums();
        assert_eq!(cpp_field_type("int", &e), "int32_t");
        assert_eq!(cpp_field_type("str", &e), "std::string");
        assert_eq!(cpp_field_type("Tuple2<int,str>", &e), "Tuple2<int32_t, std::string>");
        assert_eq!(cpp_field_type("List<int>", &e), "std::vector<int32_t>");
        assert_eq!(cpp_field_type("Set<int>", &e), "std::unordered_set<int32_t>");
        assert_eq!(cpp_field_type("Map<int,str>", &e), "std::unordered_map<int32_t, std::string>");
        assert_eq!(cpp_field_type("Map<str,List<int>>", &e), "std::unordered_map<std::string, std::vector<int32_t>>");
        assert_eq!(cpp_field_type("@HeroBase", &e), "int32_t");
    }

    #[test]
    fn ref_to_enum_uses_enum_type() {
        let mut e = HashSet::new();
        e.insert("HeroType".to_string());
        assert_eq!(cpp_field_type("@HeroType", &e), "HeroTypeEnum");
    }

    #[test]
    fn parse_expr_for_base_and_list() {
        let t = TblType::parse("int").unwrap();
        let expr = cpp_parse_expr("raw", &t, "sep", &no_enums());
        assert_eq!(expr, "parse_int32(raw)");

        let t = TblType::parse("List<int>").unwrap();
        let expr = cpp_parse_expr("raw", &t, "sep", &no_enums());
        assert!(expr.contains("parse_list<int32_t>"));
        assert!(expr.contains("sep.list"));
    }

    #[test]
    fn parse_expr_for_map_list() {
        let t = TblType::parse("Map<str,List<int>>").unwrap();
        let expr = cpp_parse_expr("raw", &t, "sep", &no_enums());
        assert!(expr.contains("parse_map<std::string, std::vector<int32_t>>"));
        assert!(expr.contains("sep.map_list_kv"));
        assert!(expr.contains("sep.map_list_entry"));
        assert!(expr.contains("sep.map_list_item"));
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
            original_entries: Vec::new(), saved: true,
        };
        let out = gen_enum_header(&e, "game::config");
        assert!(out.contains("namespace game::config"));
        assert!(out.contains("enum class HeroTypeEnum : int32_t"));
        assert!(out.contains("Warrior = 1,"));
        assert!(out.contains("Mage = 2,"));
        assert!(out.contains("HeroTypeEnum_desc"));
        assert!(out.contains("return \"战士\""));
    }
}
