//! C# 导出器（分离模式）。
//!
//! 三个 runtime（dotnet / unity / godot）共享 schema 类（`<Name>Tpl.cs` / `<Name>Enum.cs`），
//! 只在 `Loader.cs` 上分三套——加载入口 / JSON 库不同。
//!
//! 配置：
//!   [export.server.csharp_dotnet]   namespace, code_output → 走 dotnet runtime
//!   [export.client.csharp_unity]    同上 → unity runtime
//!   [export.client.csharp_godot]    同上 → godot runtime
//!
//! 通过 [`CSharpRuntime`] 选择使用哪个 Loader 模板与 Privacy。
//! - dotnet : 服务端 → `Export::ClientServer | Export::ServerOnly` 字段可见
//! - unity / godot : 客户端 → `Export::ClientServer | Export::ClientOnly` 字段可见
//!
//! 数据复用 json.rs 输出（`{_sep, data}` wrapper）。

use std::collections::HashSet;
use std::fmt::Write;
use anyhow::Result;
use crate::model::*;
use crate::types::*;
use super::{to_camel_case, to_pascal_case, LineEnding};

const TPL_TBL: &str = include_str!("../../templates/csharp/Tbl.cs");
const TPL_LOADER_DOTNET: &str = include_str!("../../templates/csharp/Loader.dotnet.cs");
const TPL_LOADER_UNITY: &str = include_str!("../../templates/csharp/Loader.unity.cs");
const TPL_LOADER_GODOT: &str = include_str!("../../templates/csharp/Loader.godot.cs");

/// C# 三 runtime 的拆分点：决定 Loader.cs 模板与字段可见性。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CSharpRuntime {
    Dotnet,
    Unity,
    Godot,
}

impl CSharpRuntime {
    fn is_server(self) -> bool { matches!(self, CSharpRuntime::Dotnet) }
    fn loader_tpl(self) -> &'static str {
        match self {
            CSharpRuntime::Dotnet => TPL_LOADER_DOTNET,
            CSharpRuntime::Unity => TPL_LOADER_UNITY,
            CSharpRuntime::Godot => TPL_LOADER_GODOT,
        }
    }
    fn default_namespace(self) -> &'static str {
        match self {
            CSharpRuntime::Dotnet => "Game.Config.Server",
            CSharpRuntime::Unity => "Game.Config.Client",
            CSharpRuntime::Godot => "Game.Config.Client",
        }
    }
    fn default_code_output(self) -> &'static str {
        match self {
            CSharpRuntime::Dotnet => "gen/server/csharp",
            CSharpRuntime::Unity => "gen/client/csharp_unity",
            CSharpRuntime::Godot => "gen/client/csharp_godot",
        }
    }
}

fn export_visible(export: &Export, runtime: CSharpRuntime) -> bool {
    if runtime.is_server() {
        matches!(export, Export::ClientServer | Export::ServerOnly)
    } else {
        matches!(export, Export::ClientServer | Export::ClientOnly)
    }
}

fn cs_base_type(bt: BaseType) -> &'static str {
    match bt {
        BaseType::Int => "int",
        BaseType::Long => "long",
        BaseType::Float => "float",
        BaseType::Double => "double",
        BaseType::Str => "string",
        BaseType::Bool => "bool",
    }
}

fn cs_base_default(bt: BaseType) -> &'static str {
    match bt {
        BaseType::Int | BaseType::Long => "0",
        BaseType::Float => "0f",
        BaseType::Double => "0d",
        BaseType::Str => "\"\"",
        BaseType::Bool => "false",
    }
}

fn cs_base_parse_fn(bt: BaseType) -> &'static str {
    match bt {
        BaseType::Int => "TblParse.ParseInt32",
        BaseType::Long => "TblParse.ParseInt64",
        BaseType::Float => "TblParse.ParseFloat",
        BaseType::Double => "TblParse.ParseDouble",
        BaseType::Str => "TblParse.ParseStr",
        BaseType::Bool => "TblParse.ParseBool",
    }
}

fn cs_field_type(tbl_type_str: &str, enum_names: &HashSet<String>) -> String {
    let Some(t) = TblType::parse(tbl_type_str) else { return "string".to_string() };
    let p = &t.params;
    match &t.paradigm {
        Paradigm::Base => cs_base_type(p[0]).to_string(),
        Paradigm::Tuple2 => format!("Tuple2<{}, {}>", cs_base_type(p[0]), cs_base_type(p[1])),
        Paradigm::Tuple3 => format!("Tuple3<{}, {}, {}>", cs_base_type(p[0]), cs_base_type(p[1]), cs_base_type(p[2])),
        Paradigm::Tuple4 => format!("Tuple4<{}, {}, {}, {}>", cs_base_type(p[0]), cs_base_type(p[1]), cs_base_type(p[2]), cs_base_type(p[3])),
        Paradigm::List => format!("List<{}>", cs_base_type(p[0])),
        Paradigm::Set => format!("HashSet<{}>", cs_base_type(p[0])),
        Paradigm::Map => format!("Dictionary<{}, {}>", cs_base_type(p[0]), cs_base_type(p[1])),
        Paradigm::ListTuple2 => format!("List<Tuple2<{}, {}>>", cs_base_type(p[0]), cs_base_type(p[1])),
        Paradigm::ListTuple3 => format!("List<Tuple3<{}, {}, {}>>", cs_base_type(p[0]), cs_base_type(p[1]), cs_base_type(p[2])),
        Paradigm::ListTuple4 => format!("List<Tuple4<{}, {}, {}, {}>>", cs_base_type(p[0]), cs_base_type(p[1]), cs_base_type(p[2]), cs_base_type(p[3])),
        Paradigm::MapTuple2 => format!("Dictionary<{}, Tuple2<{}, {}>>", cs_base_type(p[0]), cs_base_type(p[1]), cs_base_type(p[2])),
        Paradigm::MapTuple3 => format!("Dictionary<{}, Tuple3<{}, {}, {}>>", cs_base_type(p[0]), cs_base_type(p[1]), cs_base_type(p[2]), cs_base_type(p[3])),
        Paradigm::MapTuple4 => format!("Dictionary<{}, Tuple4<{}, {}, {}, {}>>", cs_base_type(p[0]), cs_base_type(p[1]), cs_base_type(p[2]), cs_base_type(p[3]), cs_base_type(p[4])),
        Paradigm::MapList => format!("Dictionary<{}, List<{}>>", cs_base_type(p[0]), cs_base_type(p[1])),
        Paradigm::Ref => {
            if let Some(name) = &t.ref_name {
                if enum_names.contains(name) {
                    return format!("{}Enum", name);
                }
            }
            "int".to_string()
        }
    }
}

fn cs_field_default(tbl_type_str: &str, enum_names: &HashSet<String>) -> String {
    match TblType::parse(tbl_type_str) {
        Some(t) if t.paradigm == Paradigm::Base => cs_base_default(t.params[0]).to_string(),
        Some(t) if t.paradigm == Paradigm::Ref => {
            if let Some(rn) = &t.ref_name {
                if enum_names.contains(rn) {
                    return "default".to_string();
                }
            }
            "0".to_string()
        }
        Some(_) => "new()".to_string(),
        None => "\"\"".to_string(),
    }
}

/// raw 字符串变量名 → C# 表达式（结果是该字段的目标类型）。`sep` 是 `SepConfig` 变量名。
fn cs_parse_expr(raw_var: &str, t: &TblType, sep: &str, enum_names: &HashSet<String>) -> String {
    let p = &t.params;
    match &t.paradigm {
        Paradigm::Base => format!("{}({})", cs_base_parse_fn(p[0]), raw_var),

        Paradigm::Tuple2 => format!(
            "TblParse.ParseTuple2<{ta}, {tb}>({raw}, {sep}.Tuple2, {fa}, {fb})",
            ta=cs_base_type(p[0]), tb=cs_base_type(p[1]), raw=raw_var, sep=sep,
            fa=cs_base_parse_fn(p[0]), fb=cs_base_parse_fn(p[1])
        ),
        Paradigm::Tuple3 => format!(
            "TblParse.ParseTuple3<{ta}, {tb}, {tc}>({raw}, {sep}.Tuple3, {fa}, {fb}, {fc})",
            ta=cs_base_type(p[0]), tb=cs_base_type(p[1]), tc=cs_base_type(p[2]),
            raw=raw_var, sep=sep,
            fa=cs_base_parse_fn(p[0]), fb=cs_base_parse_fn(p[1]), fc=cs_base_parse_fn(p[2])
        ),
        Paradigm::Tuple4 => format!(
            "TblParse.ParseTuple4<{ta}, {tb}, {tc}, {td}>({raw}, {sep}.Tuple4, {fa}, {fb}, {fc}, {fd})",
            ta=cs_base_type(p[0]), tb=cs_base_type(p[1]), tc=cs_base_type(p[2]), td=cs_base_type(p[3]),
            raw=raw_var, sep=sep,
            fa=cs_base_parse_fn(p[0]), fb=cs_base_parse_fn(p[1]), fc=cs_base_parse_fn(p[2]), fd=cs_base_parse_fn(p[3])
        ),

        Paradigm::List => format!(
            "TblParse.ParseList<{t}>({raw}, {sep}.List, {f})",
            t=cs_base_type(p[0]), raw=raw_var, sep=sep, f=cs_base_parse_fn(p[0])
        ),
        Paradigm::Set => format!(
            "TblParse.ParseSet<{t}>({raw}, {sep}.Set, {f})",
            t=cs_base_type(p[0]), raw=raw_var, sep=sep, f=cs_base_parse_fn(p[0])
        ),
        Paradigm::Map => format!(
            "TblParse.ParseMap<{tk}, {tv}>({raw}, {sep}.MapKv, {sep}.MapEntry, {fk}, {fv})",
            tk=cs_base_type(p[0]), tv=cs_base_type(p[1]),
            raw=raw_var, sep=sep,
            fk=cs_base_parse_fn(p[0]), fv=cs_base_parse_fn(p[1])
        ),

        Paradigm::ListTuple2 => format!(
            "TblParse.ParseList<Tuple2<{ta}, {tb}>>({raw}, {sep}.ListTuple2List, s => TblParse.ParseTuple2<{ta}, {tb}>(s, {sep}.ListTuple2Tuple, {fa}, {fb}))",
            ta=cs_base_type(p[0]), tb=cs_base_type(p[1]),
            raw=raw_var, sep=sep,
            fa=cs_base_parse_fn(p[0]), fb=cs_base_parse_fn(p[1])
        ),
        Paradigm::ListTuple3 => format!(
            "TblParse.ParseList<Tuple3<{ta}, {tb}, {tc}>>({raw}, {sep}.ListTuple3List, s => TblParse.ParseTuple3<{ta}, {tb}, {tc}>(s, {sep}.ListTuple3Tuple, {fa}, {fb}, {fc}))",
            ta=cs_base_type(p[0]), tb=cs_base_type(p[1]), tc=cs_base_type(p[2]),
            raw=raw_var, sep=sep,
            fa=cs_base_parse_fn(p[0]), fb=cs_base_parse_fn(p[1]), fc=cs_base_parse_fn(p[2])
        ),
        Paradigm::ListTuple4 => format!(
            "TblParse.ParseList<Tuple4<{ta}, {tb}, {tc}, {td}>>({raw}, {sep}.ListTuple4List, s => TblParse.ParseTuple4<{ta}, {tb}, {tc}, {td}>(s, {sep}.ListTuple4Tuple, {fa}, {fb}, {fc}, {fd}))",
            ta=cs_base_type(p[0]), tb=cs_base_type(p[1]), tc=cs_base_type(p[2]), td=cs_base_type(p[3]),
            raw=raw_var, sep=sep,
            fa=cs_base_parse_fn(p[0]), fb=cs_base_parse_fn(p[1]), fc=cs_base_parse_fn(p[2]), fd=cs_base_parse_fn(p[3])
        ),

        Paradigm::MapTuple2 => format!(
            "TblParse.ParseMap<{tk}, Tuple2<{ta}, {tb}>>({raw}, {sep}.MapTuple2Kv, {sep}.MapTuple2Entry, {fk}, s => TblParse.ParseTuple2<{ta}, {tb}>(s, {sep}.MapTuple2Tuple, {fa}, {fb}))",
            tk=cs_base_type(p[0]), ta=cs_base_type(p[1]), tb=cs_base_type(p[2]),
            raw=raw_var, sep=sep,
            fk=cs_base_parse_fn(p[0]), fa=cs_base_parse_fn(p[1]), fb=cs_base_parse_fn(p[2])
        ),
        Paradigm::MapTuple3 => format!(
            "TblParse.ParseMap<{tk}, Tuple3<{ta}, {tb}, {tc}>>({raw}, {sep}.MapTuple3Kv, {sep}.MapTuple3Entry, {fk}, s => TblParse.ParseTuple3<{ta}, {tb}, {tc}>(s, {sep}.MapTuple3Tuple, {fa}, {fb}, {fc}))",
            tk=cs_base_type(p[0]), ta=cs_base_type(p[1]), tb=cs_base_type(p[2]), tc=cs_base_type(p[3]),
            raw=raw_var, sep=sep,
            fk=cs_base_parse_fn(p[0]), fa=cs_base_parse_fn(p[1]), fb=cs_base_parse_fn(p[2]), fc=cs_base_parse_fn(p[3])
        ),
        Paradigm::MapTuple4 => format!(
            "TblParse.ParseMap<{tk}, Tuple4<{ta}, {tb}, {tc}, {td}>>({raw}, {sep}.MapTuple4Kv, {sep}.MapTuple4Entry, {fk}, s => TblParse.ParseTuple4<{ta}, {tb}, {tc}, {td}>(s, {sep}.MapTuple4Tuple, {fa}, {fb}, {fc}, {fd}))",
            tk=cs_base_type(p[0]), ta=cs_base_type(p[1]), tb=cs_base_type(p[2]), tc=cs_base_type(p[3]), td=cs_base_type(p[4]),
            raw=raw_var, sep=sep,
            fk=cs_base_parse_fn(p[0]), fa=cs_base_parse_fn(p[1]), fb=cs_base_parse_fn(p[2]), fc=cs_base_parse_fn(p[3]), fd=cs_base_parse_fn(p[4])
        ),

        Paradigm::MapList => format!(
            "TblParse.ParseMap<{tk}, List<{tv}>>({raw}, {sep}.MapListKv, {sep}.MapListEntry, {fk}, s => TblParse.ParseList<{tv}>(s, {sep}.MapListItem, {fv}))",
            tk=cs_base_type(p[0]), tv=cs_base_type(p[1]),
            raw=raw_var, sep=sep,
            fk=cs_base_parse_fn(p[0]), fv=cs_base_parse_fn(p[1])
        ),

        Paradigm::Ref => {
            if let Some(name) = &t.ref_name {
                if enum_names.contains(name) {
                    return format!("({}Enum)TblParse.ParseInt32({})", name, raw_var);
                }
            }
            format!("TblParse.ParseInt32({})", raw_var)
        }
    }
}

fn gen_table_class(table: &Table, ns: &str, runtime: CSharpRuntime, enum_names: &HashSet<String>) -> String {
    let mut s = String::new();
    let cls = format!("{}Tpl", table.name);

    let fields: Vec<&FieldDef> = table.schema.fields.iter()
        .filter(|f| export_visible(&f.export, runtime))
        .collect();

    writeln!(s, "// Auto-generated by tablet. Do not edit.").unwrap();
    writeln!(s, "using System.Collections.Generic;").unwrap();
    writeln!(s).unwrap();
    writeln!(s, "namespace {}", ns).unwrap();
    writeln!(s, "{{").unwrap();
    writeln!(s, "    public sealed class {}", cls).unwrap();
    writeln!(s, "    {{").unwrap();
    for f in &fields {
        let typ = cs_field_type(&f.tbl_type, enum_names);
        let name = to_pascal_case(&f.name);
        let default = cs_field_default(&f.tbl_type, enum_names);
        writeln!(s, "        public {} {} = {};", typ, name, default).unwrap();
    }
    writeln!(s).unwrap();
    writeln!(s, "        static readonly Dictionary<int, {}> _data = new();", cls).unwrap();
    writeln!(s, "        public static {} Get(int id) => _data.TryGetValue(id, out var v) ? v : null;", cls).unwrap();
    writeln!(s, "        public static IReadOnlyDictionary<int, {}> All => _data;", cls).unwrap();
    writeln!(s).unwrap();
    writeln!(s, "        static {}() {{", cls).unwrap();
    writeln!(s, "            TblRegistry.Tables[\"{}\"] = (rows, sep) => {{", table.name).unwrap();
    writeln!(s, "                _data.Clear();").unwrap();
    writeln!(s, "                foreach (var r in rows) {{").unwrap();
    writeln!(s, "                    var obj = Parse(r, sep);").unwrap();
    writeln!(s, "                    _data[obj.Id] = obj;").unwrap();
    writeln!(s, "                }}").unwrap();
    writeln!(s, "            }};").unwrap();
    writeln!(s, "        }}").unwrap();
    writeln!(s).unwrap();
    writeln!(s, "        static {} Parse(Dictionary<string, string> row, SepConfig sep) {{", cls).unwrap();
    writeln!(s, "            var obj = new {}();", cls).unwrap();
    writeln!(s, "            string Get(string k) => row.TryGetValue(k, out var v) ? v : string.Empty;").unwrap();
    for f in &fields {
        let camel = to_camel_case(&f.name);
        let pascal = to_pascal_case(&f.name);
        match TblType::parse(&f.tbl_type) {
            Some(t) => {
                writeln!(s, "            {{ var raw = Get(\"{}\"); if (!string.IsNullOrEmpty(raw)) obj.{} = {}; }}",
                    camel, pascal, cs_parse_expr("raw", &t, "sep", enum_names)).unwrap();
            }
            None => {
                writeln!(s, "            obj.{} = Get(\"{}\");", pascal, camel).unwrap();
            }
        }
    }
    writeln!(s, "            return obj;").unwrap();
    writeln!(s, "        }}").unwrap();
    writeln!(s, "    }}").unwrap();
    writeln!(s, "}}").unwrap();
    s
}

fn gen_constant_class(constant: &Constant, ns: &str, runtime: CSharpRuntime, enum_names: &HashSet<String>) -> String {
    let mut s = String::new();
    let cls = format!("{}Tpl", constant.name);

    let entries: Vec<&ConstEntry> = constant.entries.iter()
        .filter(|e| export_visible(&e.export, runtime) && !e.name.is_empty())
        .collect();

    writeln!(s, "// Auto-generated by tablet. Do not edit.").unwrap();
    writeln!(s, "using System.Collections.Generic;").unwrap();
    writeln!(s).unwrap();
    writeln!(s, "namespace {}", ns).unwrap();
    writeln!(s, "{{").unwrap();
    writeln!(s, "    public sealed class {}", cls).unwrap();
    writeln!(s, "    {{").unwrap();
    for e in &entries {
        let typ = cs_field_type(&e.tbl_type, enum_names);
        let name = to_pascal_case(&e.name);
        let default = cs_field_default(&e.tbl_type, enum_names);
        writeln!(s, "        public {} {} = {};", typ, name, default).unwrap();
    }
    writeln!(s).unwrap();
    writeln!(s, "        static {} _data = new();", cls).unwrap();
    writeln!(s, "        public static {} Get() => _data;", cls).unwrap();
    writeln!(s).unwrap();
    writeln!(s, "        static {}() {{", cls).unwrap();
    writeln!(s, "            TblRegistry.Constants[\"{}\"] = (row, sep) => {{ _data = Parse(row, sep); }};", constant.name).unwrap();
    writeln!(s, "        }}").unwrap();
    writeln!(s).unwrap();
    writeln!(s, "        static {} Parse(Dictionary<string, string> row, SepConfig sep) {{", cls).unwrap();
    writeln!(s, "            var obj = new {}();", cls).unwrap();
    writeln!(s, "            string Get(string k) => row.TryGetValue(k, out var v) ? v : string.Empty;").unwrap();
    for e in &entries {
        let camel = to_camel_case(&e.name);
        let pascal = to_pascal_case(&e.name);
        match TblType::parse(&e.tbl_type) {
            Some(t) => {
                writeln!(s, "            {{ var raw = Get(\"{}\"); if (!string.IsNullOrEmpty(raw)) obj.{} = {}; }}",
                    camel, pascal, cs_parse_expr("raw", &t, "sep", enum_names)).unwrap();
            }
            None => {
                writeln!(s, "            obj.{} = Get(\"{}\");", pascal, camel).unwrap();
            }
        }
    }
    writeln!(s, "            return obj;").unwrap();
    writeln!(s, "        }}").unwrap();
    writeln!(s, "    }}").unwrap();
    writeln!(s, "}}").unwrap();
    s
}

fn gen_enum_file(enum_def: &EnumDef, ns: &str) -> String {
    let mut s = String::new();
    let type_name = format!("{}Enum", enum_def.name);

    let valid: Vec<&EnumEntry> = enum_def.entries.iter()
        .filter(|e| !e.id.is_empty() && !e.name.is_empty())
        .collect();

    writeln!(s, "// Auto-generated by tablet. Do not edit.").unwrap();
    writeln!(s).unwrap();
    writeln!(s, "namespace {}", ns).unwrap();
    writeln!(s, "{{").unwrap();
    writeln!(s, "    public enum {} : int", type_name).unwrap();
    writeln!(s, "    {{").unwrap();
    for e in &valid {
        let id = e.id.parse::<i64>().unwrap_or(0);
        writeln!(s, "        /// <summary>{}</summary>", escape_xml(&e.desc)).unwrap();
        writeln!(s, "        {} = {},", e.name, id).unwrap();
    }
    writeln!(s, "    }}").unwrap();
    writeln!(s).unwrap();
    writeln!(s, "    public static class {}Ext", type_name).unwrap();
    writeln!(s, "    {{").unwrap();
    writeln!(s, "        public static string GetDesc(this {} v) => v switch", type_name).unwrap();
    writeln!(s, "        {{").unwrap();
    for e in &valid {
        writeln!(s, "            {}.{} => \"{}\",", type_name, e.name, escape_cs(&e.desc)).unwrap();
    }
    writeln!(s, "            _ => \"\",").unwrap();
    writeln!(s, "        }};").unwrap();
    writeln!(s, "    }}").unwrap();
    writeln!(s, "}}").unwrap();
    s
}

fn escape_cs(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

pub fn export_all_csharp(project: &Project, runtime: CSharpRuntime) -> Result<super::ExportResult> {
    let export_cfg = &project.config.export;

    // 配置定位：按 runtime 取对应 section，提取 namespace 和 code_output
    let (code_output, ns) = match runtime {
        CSharpRuntime::Dotnet => {
            let cfg = export_cfg.server.as_ref()
                .and_then(|s| s.csharp_dotnet.as_ref());
            let output = cfg.and_then(|c| c.code_output.as_deref()).unwrap_or(runtime.default_code_output());
            let namespace = cfg.and_then(|c| c.namespace.as_deref()).unwrap_or(runtime.default_namespace());
            (output, namespace)
        }
        CSharpRuntime::Unity => {
            let cfg = export_cfg.client.as_ref()
                .and_then(|c| c.csharp_unity.as_ref());
            let output = cfg.and_then(|c| c.code_output.as_deref()).unwrap_or(runtime.default_code_output());
            let namespace = cfg.and_then(|c| c.namespace.as_deref()).unwrap_or(runtime.default_namespace());
            (output, namespace)
        }
        CSharpRuntime::Godot => {
            let cfg = export_cfg.client.as_ref()
                .and_then(|c| c.csharp_godot.as_ref());
            let output = cfg.and_then(|c| c.code_output.as_deref()).unwrap_or(runtime.default_code_output());
            let namespace = cfg.and_then(|c| c.namespace.as_deref()).unwrap_or(runtime.default_namespace());
            (output, namespace)
        }
    };

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

    collect(&output_dir, "Tbl.cs", &render(TPL_TBL));
    collect(&output_dir, "Loader.cs", &render(runtime.loader_tpl()));

    let enum_names: HashSet<String> = project.groups.iter()
        .flat_map(|g| g.enums.iter())
        .filter(|e| !e.deleted)
        .map(|e| e.name.clone())
        .collect();

    for group in &project.groups {
        for table in &group.tables {
            if table.deleted { continue; }
            let content = gen_table_class(table, ns, runtime, &enum_names);
            collect(&output_dir, &format!("{}Tpl.cs", table.name), &content);
        }
        for constant in &group.constants {
            if constant.deleted { continue; }
            let content = gen_constant_class(constant, ns, runtime, &enum_names);
            collect(&output_dir, &format!("{}Tpl.cs", constant.name), &content);
        }
        for enum_def in &group.enums {
            if enum_def.deleted { continue; }
            let content = gen_enum_file(enum_def, ns);
            collect(&output_dir, &format!("{}Enum.cs", enum_def.name), &content);
        }
    }

    let _ = runtime;
    super::sync_export_dir(&output_dir, "cs", collected)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_enums() -> HashSet<String> { HashSet::new() }

    #[test]
    fn base_types() {
        assert_eq!(cs_base_type(BaseType::Int), "int");
        assert_eq!(cs_base_type(BaseType::Long), "long");
        assert_eq!(cs_base_type(BaseType::Str), "string");
        assert_eq!(cs_base_type(BaseType::Bool), "bool");
    }

    #[test]
    fn field_type_paradigms() {
        let e = no_enums();
        assert_eq!(cs_field_type("int", &e), "int");
        assert_eq!(cs_field_type("str", &e), "string");
        assert_eq!(cs_field_type("Tuple2<int,str>", &e), "Tuple2<int, string>");
        assert_eq!(cs_field_type("List<int>", &e), "List<int>");
        assert_eq!(cs_field_type("Set<int>", &e), "HashSet<int>");
        assert_eq!(cs_field_type("Map<int,str>", &e), "Dictionary<int, string>");
        assert_eq!(cs_field_type("Map<str,List<int>>", &e), "Dictionary<string, List<int>>");
        assert_eq!(cs_field_type("@HeroBase", &e), "int");
    }

    #[test]
    fn ref_to_enum_uses_enum_type() {
        let mut e = HashSet::new();
        e.insert("HeroType".to_string());
        assert_eq!(cs_field_type("@HeroType", &e), "HeroTypeEnum");
    }

    #[test]
    fn parse_expr_for_base_and_list() {
        let t = TblType::parse("int").unwrap();
        assert_eq!(cs_parse_expr("raw", &t, "sep", &no_enums()), "TblParse.ParseInt32(raw)");

        let t = TblType::parse("List<int>").unwrap();
        let expr = cs_parse_expr("raw", &t, "sep", &no_enums());
        assert!(expr.contains("TblParse.ParseList<int>"));
        assert!(expr.contains("sep.List"));
    }

    #[test]
    fn parse_expr_for_map_list() {
        let t = TblType::parse("Map<str,List<int>>").unwrap();
        let expr = cs_parse_expr("raw", &t, "sep", &no_enums());
        assert!(expr.contains("TblParse.ParseMap<string, List<int>>"));
        assert!(expr.contains("sep.MapListKv"));
        assert!(expr.contains("sep.MapListEntry"));
        assert!(expr.contains("sep.MapListItem"));
    }

    #[test]
    fn ref_enum_parse_uses_cast() {
        let mut e = HashSet::new();
        e.insert("HeroType".to_string());
        let t = TblType::parse("@HeroType").unwrap();
        let expr = cs_parse_expr("raw", &t, "sep", &e);
        assert_eq!(expr, "(HeroTypeEnum)TblParse.ParseInt32(raw)");
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
            dirty: false, deleted: false, original: String::new(),
        };
        let out = gen_enum_file(&e, "Game.Config");
        assert!(out.contains("namespace Game.Config"));
        assert!(out.contains("public enum HeroTypeEnum : int"));
        assert!(out.contains("Warrior = 1,"));
        assert!(out.contains("Mage = 2,"));
        assert!(out.contains("HeroTypeEnumExt"));
        assert!(out.contains("\"战士\""));
    }

    #[test]
    fn runtime_picks_loader_tpl() {
        // 简单 sanity：三个 runtime 各拿到的 loader 内容互不相同
        let d = CSharpRuntime::Dotnet.loader_tpl();
        let u = CSharpRuntime::Unity.loader_tpl();
        let g = CSharpRuntime::Godot.loader_tpl();
        assert!(d != u);
        assert!(u != g);
        assert!(d != g);
        assert!(d.contains("System.Text.Json"));
        assert!(u.contains("UnityEngine"));
        assert!(g.contains("Godot"));
    }
}
