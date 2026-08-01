use std::fmt::Write;
use std::path::Path;
use crate::template::{BuiltinTemplates, TemplateSource};
use crate::tblschema::*;

/// 测试数据生成选项
#[derive(Debug, Clone)]
pub struct TestGenOptions {
    /// 使用空值测试 schema
    pub include_empty: bool,
    /// 数据行数（0 = 使用默认固定数据）
    pub rows: usize,
    /// 随机种子（0 = 固定数据，非 0 = 随机生成）
    pub seed: u64,
}

impl Default for TestGenOptions {
    fn default() -> Self { Self::full() }
}

impl TestGenOptions {
    pub fn full() -> Self {
        Self { include_empty: false, rows: 0, seed: 0 }
    }

    pub fn with_empty() -> Self {
        Self { include_empty: true, rows: 0, seed: 0 }
    }
}

/// 取内置模板 schema：当前只剩 `standard`。
/// `opts.include_empty` 现在仅影响 generate_data_rows 时是否注入空值，schema 来源不变。
fn builtin_schema(_opts: &TestGenOptions) -> TblSchema {
    BuiltinTemplates::new()
        .load_by_id("standard")
        .expect("内置模板缺失（standard）")
        .schema
}

// === 公开 API ===

/// 使用内置 schema 生成测试数据
pub fn generate_test_config(config_dir: &Path, opts: &TestGenOptions) {
    let schema = builtin_schema(opts);
    generate_from_schema(config_dir, &schema, opts);
}

/// 使用内置 schema 生成 TestMain.java
pub fn generate_test_main(workdir: &Path, opts: &TestGenOptions, pkg: &str, format: &str) {
    let schema = builtin_schema(opts);
    generate_test_main_from_schema(workdir, &schema, pkg, format);
}

/// 使用内置 schema 生成 Go 测试 main.go + go.mod
pub fn generate_test_main_go(workdir: &Path, opts: &TestGenOptions, pkg: &str, code_output: &str, format: &str) {
    let schema = builtin_schema(opts);
    generate_test_main_go_from_schema(workdir, &schema, pkg, code_output, format);
}

/// 根据 schema + options 生成 .tbl 文件
pub fn generate_from_schema(config_dir: &Path, schema: &TblSchema, opts: &TestGenOptions) {
    for sec in &schema.sections {
        let dir = config_dir.join(&sec.group);
        let _ = std::fs::create_dir_all(&dir);
        let content = generate_tbl_content(sec, opts);
        let _ = std::fs::write(dir.join(format!("{}.tbl", sec.name)), &content);
    }
}

/// 根据 schema 生成 TestMain.java
pub fn generate_test_main_from_schema(workdir: &Path, schema: &TblSchema, pkg: &str, format: &str) {
    let content = build_test_main(schema, pkg, format);
    let _ = std::fs::write(workdir.join("TestMain.java"), &content);
}

/// 根据 schema 生成 Go 测试 main.go + go.mod
///
/// 在 workdir 写入一份顶层 go.mod，并把 main.go 放在 workdir/test_main_go/。
/// 生成的 config 包通过 module path "tblmain/<code_output>/<pkg>" 被 main.go 引用。
pub fn generate_test_main_go_from_schema(workdir: &Path, schema: &TblSchema, pkg: &str, code_output: &str, format: &str) {
    let module = "tblmain";
    let go_mod = format!("module {}\n\ngo 1.21\n", module);
    let _ = std::fs::write(workdir.join("go.mod"), go_mod);

    let main_dir = workdir.join("test_main_go");
    let _ = std::fs::create_dir_all(&main_dir);

    let import_path = format!(
        "{}/{}/{}",
        module,
        code_output.trim_end_matches('/').replace('\\', "/"),
        pkg
    );
    let main_go = build_test_main_go(schema, pkg, &import_path, format);
    let _ = std::fs::write(main_dir.join("main.go"), &main_go);
}

// === .tbl 内容生成 ===

fn generate_tbl_content(sec: &SchemaSection, opts: &TestGenOptions) -> String {
    match sec.mode {
        SchemaMode::Table => generate_table_tbl(sec, opts),
        SchemaMode::Constant => generate_const_tbl(sec),
        SchemaMode::Enum => generate_enum_tbl(sec),
    }
}

fn generate_enum_tbl(sec: &SchemaSection) -> String {
    let mut s = String::new();
    writeln!(s, "#!tbl v2").unwrap();
    writeln!(s, "#mode enum").unwrap();
    writeln!(s, "---").unwrap();
    // 新范式：enum 行数据全在 preset（id | name | desc）
    for row in &sec.preset {
        let id   = row.first().map(String::as_str).unwrap_or("");
        let name = row.get(1) .map(String::as_str).unwrap_or("");
        let desc = row.get(2) .map(String::as_str).unwrap_or("");
        writeln!(s, "{}|{}|{}", id, name, desc).unwrap();
    }
    s
}

fn generate_table_tbl(sec: &SchemaSection, opts: &TestGenOptions) -> String {
    let mut s = String::new();
    writeln!(s, "#!tbl v2").unwrap();
    writeln!(s, "#mode table").unwrap();
    writeln!(s, "#desc {}", sec.fields.iter().map(|f| f.desc.as_str()).collect::<Vec<_>>().join("|")).unwrap();
    writeln!(s, "#export {}", sec.fields.iter().map(|f| f.export_display()).collect::<Vec<_>>().join("|")).unwrap();
    writeln!(s, "#type {}", sec.fields.iter().map(|f| f.tbl_type.as_str()).collect::<Vec<_>>().join("|")).unwrap();
    writeln!(s, "#field {}", sec.fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>().join("|")).unwrap();
    writeln!(s, "---").unwrap();

    let rows = generate_data_rows(sec, opts);
    for row in &rows {
        writeln!(s, "{}", row.join("|")).unwrap();
    }
    s
}

fn generate_const_tbl(sec: &SchemaSection) -> String {
    let mut s = String::new();
    writeln!(s, "#!tbl v2").unwrap();
    writeln!(s, "#mode constant").unwrap();
    writeln!(s, "---").unwrap();
    // 新范式：constant 行数据全在 preset（name | type | value | export | desc）
    // 内部 preset 的 export 列是 code（cs / s / c / -），落到 .tbl 时换成显示文案
    for row in &sec.preset {
        let name      = row.first()  .map(String::as_str).unwrap_or("");
        let tbl_type  = row.get(1)   .map(String::as_str).unwrap_or("");
        let value     = row.get(2)   .map(String::as_str).unwrap_or("");
        let export_cd = row.get(3)   .map(String::as_str).unwrap_or("cs");
        let desc      = row.get(4)   .map(String::as_str).unwrap_or("");
        let export_disp = match export_cd {
            "cs" | "" => "前后端",
            "c" => "客户端",
            "s" => "服务器",
            "-" => "不导出",
            _ => "前后端",
        };
        writeln!(s, "{}|{}|{}|{}|{}", name, tbl_type, value, export_disp, desc).unwrap();
    }
    s
}

// === 数据行生成 ===

fn generate_data_rows(sec: &SchemaSection, opts: &TestGenOptions) -> Vec<Vec<String>> {
    // 默认：直接吐 schema 自带的 preset，schema 是 "标品配置" 的唯一真相。
    if opts.seed == 0 && opts.rows == 0 && !opts.include_empty {
        return preset_rows_aligned(sec);
    }
    // include_empty 但仍是 fixed 模式：在 preset 之上吹空非关键字段，扩出"测空"行。
    if opts.seed == 0 && opts.rows == 0 {
        return generate_fixed_rows(sec, opts);
    }
    // 随机模式：用列类型生成 N 行
    generate_random_rows(sec, opts)
}

/// 把 sec.preset 按 fields 列数对齐：缺失列补空串、超出列截断。
fn preset_rows_aligned(sec: &SchemaSection) -> Vec<Vec<String>> {
    let n = sec.fields.len();
    sec.preset.iter().map(|row| {
        let mut r: Vec<String> = row.clone();
        r.resize(n, String::new());
        r
    }).collect()
}

fn generate_fixed_rows(sec: &SchemaSection, opts: &TestGenOptions) -> Vec<Vec<String>> {
    // 以 preset 为基底：偶数行原样保留；奇数行把非关键字段吹空（"含空值测试"）。
    let base = preset_rows_aligned(sec);
    if !opts.include_empty || base.is_empty() {
        return base;
    }
    base.into_iter().enumerate().map(|(i, row)| {
        if i % 2 == 0 { return row; }
        row.into_iter().enumerate().map(|(col, val)| {
            let fname = sec.fields.get(col).map(|f| f.name.as_str()).unwrap_or("");
            if fname == "id" || fname == "name" { val }
            else if fname == "desc" { "测试空值".to_string() }
            else { String::new() }
        }).collect()
    }).collect()
}

struct SimpleRng(u64);

impl SimpleRng {
    fn new(seed: u64) -> Self { Self(seed) }
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    fn range(&mut self, min: i64, max: i64) -> i64 {
        let span = (max - min) as u64;
        if span == 0 { return min; }
        min + (self.next() % span) as i64
    }
    fn bool_chance(&mut self, pct: u32) -> bool {
        (self.next() % 100) < pct as u64
    }
}

fn generate_random_rows(sec: &SchemaSection, opts: &TestGenOptions) -> Vec<Vec<String>> {
    let mut rng = SimpleRng::new(if opts.seed > 0 { opts.seed } else { 42 });
    let row_count = if opts.rows > 0 { opts.rows } else { 10 };
    let names = ["战士", "法师", "弓手", "刺客", "牧师", "骑士", "猎人", "术士", "武僧", "德鲁伊"];

    let mut rows = Vec::new();
    for i in 0..row_count {
        let mut row = Vec::new();
        for f in &sec.fields {
            let val = if f.name == "id" {
                format!("{}", 1001 + i as i64)
            } else if f.name == "name" {
                names[i % names.len()].to_string()
            } else if opts.include_empty && rng.bool_chance(30) {
                String::new()
            } else {
                random_value_for_type(&f.tbl_type, &mut rng)
            };
            row.push(val);
        }
        rows.push(row);
    }
    rows
}

fn random_value_for_type(tbl_type: &str, rng: &mut SimpleRng) -> String {
    match tbl_type {
        "int" => rng.range(1, 200).to_string(),
        "long" => rng.range(1000, 99999).to_string(),
        "float" | "double" => format!("{:.1}", rng.range(1, 100) as f64 / 10.0),
        "bool" => if rng.bool_chance(50) { "true" } else { "false" }.to_string(),
        "str" => {
            let words = ["fire", "ice", "wind", "earth", "light", "dark"];
            let i = rng.range(0, words.len() as i64 - 1) as usize;
            words[i].to_string()
        }
        "txt" => format!("text_{}", rng.range(1, 999)),
        t if t.starts_with("List<") => {
            let count = rng.range(1, 5) as usize;
            (0..count).map(|_| rng.range(1, 100).to_string()).collect::<Vec<_>>().join(";")
        }
        t if t.starts_with("Tuple2") => {
            format!("{},{}", rng.range(1, 100), rng.range(1, 100))
        }
        t if t.starts_with("Tuple3") => {
            format!("{},{},{}", rng.range(1, 100), rng.range(1, 100), rng.range(1, 100))
        }
        _ => rng.range(1, 100).to_string(),
    }
}

// === TestMain.java 生成 ===

fn to_camel(s: &str) -> String {
    let mut result = String::new();
    let mut upper = false;
    for ch in s.chars() {
        if ch == '_' { upper = true; }
        else if upper { result.push(ch.to_ascii_uppercase()); upper = false; }
        else { result.push(ch); }
    }
    result
}

fn to_pascal(s: &str) -> String {
    let c = to_camel(s);
    let mut chars = c.chars();
    match chars.next() {
        Some(ch) => ch.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

fn java_format_spec(tbl_type: &str) -> &'static str {
    match tbl_type {
        "int" | "long" => "%d",
        "float" | "double" => "%f",
        "bool" => "%b",
        _ => "%s",
    }
}

fn build_test_main(schema: &TblSchema, pkg: &str, format: &str) -> String {
    let mut s = String::new();
    writeln!(s, "import {}.*;", pkg).unwrap();
    writeln!(s, "import {}.tpl.*;", pkg).unwrap();
    writeln!(s, "import java.util.Map;").unwrap();
    writeln!(s).unwrap();
    writeln!(s, "public class TestMain {{").unwrap();
    writeln!(s, "    public static void main(String[] args) {{").unwrap();
    writeln!(s, "        String dataDir = args.length > 0 ? args[0] : \"gen/server/data\";").unwrap();
    if format == "xml" {
        writeln!(s, "        TplHolder.init(dataDir);").unwrap();
    } else {
        writeln!(s, "        TplHolder.initJson(dataDir);").unwrap();
    }

    for sec in &schema.sections {
        let cls = match sec.mode {
            SchemaMode::Enum => format!("{}Enum", sec.name),
            _ => format!("{}Tpl", sec.name),
        };
        writeln!(s).unwrap();
        writeln!(s, "        System.out.println(\"=== {} ===\");", sec.name).unwrap();

        match sec.mode {
            SchemaMode::Table => {
                let server_fields: Vec<&SchemaField> = sec.fields.iter()
                    .filter(|f| f.is_server_export() && f.name != "id")
                    .collect();

                writeln!(s, "        Map<Integer, {}> map{} = TplHolder.getAll({}.class);", cls, sec.name, cls).unwrap();
                writeln!(s, "        for (var entry : map{}.entrySet()) {{", sec.name).unwrap();
                writeln!(s, "            var h = entry.getValue();").unwrap();

                let mut fmt_parts = vec![format!("id={}", java_format_spec("int"))];
                let mut arg_parts = vec!["h.getId()".to_string()];
                for f in &server_fields {
                    fmt_parts.push(format!("{}={}", to_camel(&f.name), java_format_spec(&f.tbl_type)));
                    arg_parts.push(format!("h.get{}()", to_pascal(&f.name)));
                }

                writeln!(s, "            System.out.printf(\"{}%n\",", fmt_parts.join(" ")).unwrap();
                writeln!(s, "                {});", arg_parts.join(", ")).unwrap();
                writeln!(s, "        }}").unwrap();
            }
            SchemaMode::Constant => {
                let server_entries: Vec<&SchemaField> = sec.fields.iter()
                    .filter(|f| f.is_server_export())
                    .collect();

                writeln!(s, "        var gc = TplHolder.getConst({}.class);", cls).unwrap();

                if !server_entries.is_empty() {
                    let mut fmt_parts = Vec::new();
                    let mut arg_parts = Vec::new();
                    for f in &server_entries {
                        fmt_parts.push(format!("{}={}", to_camel(&f.name), java_format_spec(&f.tbl_type)));
                        arg_parts.push(format!("gc.get{}()", to_pascal(&f.name)));
                    }

                    writeln!(s, "        System.out.printf(\"{}%n\",", fmt_parts.join(" ")).unwrap();
                    writeln!(s, "            {});", arg_parts.join(", ")).unwrap();
                } else {
                    writeln!(s, "        System.out.println(\"(no server fields)\");").unwrap();
                }
            }
            SchemaMode::Enum => {
                // 枚举打印：列出所有条目 (id, name, desc)
                writeln!(s, "        for ({} v : {}.values()) {{", cls, cls).unwrap();
                writeln!(s, "            System.out.printf(\"id=%d name=%s desc=%s%n\", v.id, v.name(), v.desc);").unwrap();
                writeln!(s, "        }}").unwrap();
            }
        }
    }

    writeln!(s, "    }}").unwrap();
    writeln!(s, "}}").unwrap();
    s
}

// === Go main.go 生成 ===

fn go_format_spec(tbl_type: &str) -> &'static str {
    match tbl_type {
        "int" | "long" => "%d",
        "float" | "double" => "%g",
        "bool" => "%t",
        "str" => "%s",
        _ => "%v",
    }
}

fn build_test_main_go(schema: &TblSchema, pkg: &str, import_path: &str, format: &str) -> String {
    let mut s = String::new();
    writeln!(s, "package main").unwrap();
    writeln!(s).unwrap();
    writeln!(s, "import (").unwrap();
    writeln!(s, "\t\"fmt\"").unwrap();
    writeln!(s, "\t\"os\"").unwrap();
    writeln!(s).unwrap();
    writeln!(s, "\tcfg \"{}\"", import_path).unwrap();
    writeln!(s, ")").unwrap();
    writeln!(s).unwrap();
    let _ = pkg;

    writeln!(s, "func main() {{").unwrap();
    writeln!(s, "\tdataDir := \"gen/server/data\"").unwrap();
    writeln!(s, "\tif len(os.Args) > 1 {{").unwrap();
    writeln!(s, "\t\tdataDir = os.Args[1]").unwrap();
    writeln!(s, "\t}}").unwrap();
    if format == "xml" {
        writeln!(s, "\tif err := cfg.Init(dataDir); err != nil {{ panic(err) }}").unwrap();
    } else {
        writeln!(s, "\tif err := cfg.InitJSON(dataDir); err != nil {{ panic(err) }}").unwrap();
    }

    for sec in &schema.sections {
        writeln!(s).unwrap();
        writeln!(s, "\tfmt.Println(\"=== {} ===\")", sec.name).unwrap();

        match sec.mode {
            SchemaMode::Table => {
                let server_fields: Vec<&SchemaField> = sec.fields.iter()
                    .filter(|f| f.is_server_export() && f.name != "id")
                    .collect();

                // ID 排序输出，保证稳定
                writeln!(s, "\t{{").unwrap();
                writeln!(s, "\t\tall := cfg.GetAll{}()", sec.name).unwrap();
                writeln!(s, "\t\tids := make([]int32, 0, len(all))").unwrap();
                writeln!(s, "\t\tfor id := range all {{ ids = append(ids, id) }}").unwrap();
                writeln!(s, "\t\tsortInt32(ids)").unwrap();
                writeln!(s, "\t\tfor _, id := range ids {{").unwrap();
                writeln!(s, "\t\t\th := all[id]").unwrap();

                let mut fmt_parts = vec![format!("id={}", go_format_spec("int"))];
                let mut arg_parts = vec!["h.Id".to_string()];
                for f in &server_fields {
                    fmt_parts.push(format!("{}={}", to_camel(&f.name), go_format_spec(&f.tbl_type)));
                    arg_parts.push(format!("h.{}", to_pascal(&f.name)));
                }

                writeln!(s, "\t\t\tfmt.Printf(\"{}\\n\", {})",
                    fmt_parts.join(" "),
                    arg_parts.join(", ")
                ).unwrap();
                writeln!(s, "\t\t}}").unwrap();
                writeln!(s, "\t}}").unwrap();
            }
            SchemaMode::Constant => {
                let server_entries: Vec<&SchemaField> = sec.fields.iter()
                    .filter(|f| f.is_server_export())
                    .collect();

                writeln!(s, "\t{{").unwrap();
                writeln!(s, "\t\tc := cfg.Get{}()", sec.name).unwrap();

                if !server_entries.is_empty() {
                    let mut fmt_parts = Vec::new();
                    let mut arg_parts = Vec::new();
                    for f in &server_entries {
                        fmt_parts.push(format!("{}={}", to_camel(&f.name), go_format_spec(&f.tbl_type)));
                        arg_parts.push(format!("c.{}", to_pascal(&f.name)));
                    }

                    writeln!(s, "\t\tfmt.Printf(\"{}\\n\", {})",
                    fmt_parts.join(" "),
                    arg_parts.join(", ")
                ).unwrap();
                } else {
                    writeln!(s, "\t\t_ = c").unwrap();
                    writeln!(s, "\t\tfmt.Println(\"(no server fields)\")").unwrap();
                }
                writeln!(s, "\t}}").unwrap();
            }
            SchemaMode::Enum => {
                // sec.fields: tbl_type 借位存 id, name 存条目名
                writeln!(s, "\t{{").unwrap();
                for f in &sec.fields {
                    writeln!(s, "\t\tfmt.Printf(\"id=%d name=%s desc=%s\\n\", int32(cfg.{}Enum_{}), \"{}\", cfg.{}Enum_{}.Desc())",
                        sec.name, f.name, f.name, sec.name, f.name).unwrap();
                }
                writeln!(s, "\t}}").unwrap();
            }
        }
    }

    writeln!(s, "}}").unwrap();
    writeln!(s).unwrap();
    writeln!(s, "func sortInt32(a []int32) {{").unwrap();
    writeln!(s, "\tfor i := 1; i < len(a); i++ {{").unwrap();
    writeln!(s, "\t\tfor j := i; j > 0 && a[j-1] > a[j]; j-- {{").unwrap();
    writeln!(s, "\t\t\ta[j-1], a[j] = a[j], a[j-1]").unwrap();
    writeln!(s, "\t\t}}").unwrap();
    writeln!(s, "\t}}").unwrap();
    writeln!(s, "}}").unwrap();
    s
}
