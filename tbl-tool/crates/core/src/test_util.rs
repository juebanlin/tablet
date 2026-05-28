use std::fmt::Write;
use std::path::Path;

/// 测试数据生成选项
#[derive(Debug, Clone)]
pub struct TestGenOptions {
    /// 包含空值字段（部分行的部分字段留空）
    pub include_empty: bool,
    /// 包含集合类型（List, Set）
    pub include_collection: bool,
    /// 包含 Tuple 类型
    pub include_tuple: bool,
    /// 包含 Constant 配置
    pub include_constant: bool,
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
        Self { include_empty: false, include_collection: true, include_tuple: true, include_constant: true, rows: 0, seed: 0 }
    }

    pub fn with_empty() -> Self {
        Self { include_empty: true, include_collection: false, include_tuple: false, include_constant: false, rows: 0, seed: 0 }
    }
}

// === Schema 定义：测试数据的唯一真相源 ===

struct TableSchema {
    group: &'static str,
    name: &'static str,
    index: &'static str,
    fields: Vec<FieldSchema>,
}

struct FieldSchema {
    name: &'static str,
    tbl_type: &'static str,
    export: &'static str,
    desc: &'static str,
}

struct ConstSchema {
    group: &'static str,
    name: &'static str,
    entries: Vec<ConstEntrySchema>,
}

struct ConstEntrySchema {
    name: &'static str,
    tbl_type: &'static str,
    value: &'static str,
    export: &'static str,
    desc: &'static str,
}

struct TestData {
    tables: Vec<(TableSchema, Vec<Vec<&'static str>>)>,
    constants: Vec<ConstSchema>,
}

fn build_schema(opts: &TestGenOptions) -> TestData {
    let mut tables = Vec::new();
    let mut constants = Vec::new();

    // HeroBase table
    let hero = if opts.include_empty && opts.include_collection {
        let schema = TableSchema {
            group: "hero", name: "HeroBase", index: "id",
            fields: vec![
                FieldSchema { name: "id", tbl_type: "int", export: "前后端", desc: "英雄ID" },
                FieldSchema { name: "name", tbl_type: "str", export: "前后端", desc: "名称" },
                FieldSchema { name: "hp", tbl_type: "int", export: "服务器", desc: "血量" },
                FieldSchema { name: "skills", tbl_type: "List<int>", export: "前后端", desc: "技能组" },
                FieldSchema { name: "desc", tbl_type: "str", export: "前后端", desc: "描述" },
            ],
        };
        let rows = vec![
            vec!["1001", "战士", "100", "1;2;3", "主角"],
            vec!["1002", "法师", "", "4;5", ""],
            vec!["1003", "弓手", "90", "", "远程"],
        ];
        (schema, rows)
    } else if opts.include_empty {
        let schema = TableSchema {
            group: "hero", name: "HeroBase", index: "id",
            fields: vec![
                FieldSchema { name: "id", tbl_type: "int", export: "前后端", desc: "英雄ID" },
                FieldSchema { name: "name", tbl_type: "str", export: "前后端", desc: "名称" },
                FieldSchema { name: "hp", tbl_type: "int", export: "服务器", desc: "血量" },
                FieldSchema { name: "desc", tbl_type: "str", export: "前后端", desc: "描述" },
            ],
        };
        let rows = vec![
            vec!["1001", "战士", "100", ""],
            vec!["1002", "法师", "", "测试空血量"],
        ];
        (schema, rows)
    } else if opts.include_collection {
        let schema = TableSchema {
            group: "hero", name: "HeroBase", index: "id",
            fields: vec![
                FieldSchema { name: "id", tbl_type: "int", export: "前后端", desc: "英雄ID" },
                FieldSchema { name: "name", tbl_type: "str", export: "前后端", desc: "名称" },
                FieldSchema { name: "hp", tbl_type: "int", export: "服务器", desc: "血量" },
                FieldSchema { name: "skills", tbl_type: "List<int>", export: "前后端", desc: "技能组" },
            ],
        };
        let rows = vec![
            vec!["1001", "战士", "100", "1;2;3"],
            vec!["1002", "法师", "80", "4;5"],
            vec!["1003", "弓手", "90", "6;7;8"],
        ];
        (schema, rows)
    } else {
        let schema = TableSchema {
            group: "hero", name: "HeroBase", index: "id",
            fields: vec![
                FieldSchema { name: "id", tbl_type: "int", export: "前后端", desc: "英雄ID" },
                FieldSchema { name: "name", tbl_type: "str", export: "前后端", desc: "名称" },
                FieldSchema { name: "hp", tbl_type: "int", export: "服务器", desc: "血量" },
            ],
        };
        let rows = vec![
            vec!["1001", "战士", "100"],
            vec!["1002", "法师", "80"],
        ];
        (schema, rows)
    };
    tables.push(hero);

    // GlobalConst
    if opts.include_constant {
        let mut entries = vec![
            ConstEntrySchema { name: "max_level", tbl_type: "int", value: "100", export: "前后端", desc: "最大等级" },
        ];
        if opts.include_tuple {
            entries.push(ConstEntrySchema { name: "start_pos", tbl_type: "Tuple2<int,int>", value: "5,10", export: "前后端", desc: "出生坐标" });
        }
        entries.push(ConstEntrySchema { name: "server_name", tbl_type: "str", value: "test-server", export: "服务器", desc: "服务器名称" });
        constants.push(ConstSchema { group: "global", name: "GlobalConst", entries });
    }

    TestData { tables, constants }
}

// === 从 Schema 生成 .tbl 文件 ===

fn schema_to_tbl_table(schema: &TableSchema, rows: &[Vec<&str>]) -> String {
    let mut s = String::new();
    writeln!(s, "#!tbl v2").unwrap();
    writeln!(s, "#mode table").unwrap();
    writeln!(s, "#index {}", schema.index).unwrap();
    writeln!(s, "#desc {}", schema.fields.iter().map(|f| f.desc).collect::<Vec<_>>().join("|")).unwrap();
    writeln!(s, "#type {}", schema.fields.iter().map(|f| f.tbl_type).collect::<Vec<_>>().join("|")).unwrap();
    writeln!(s, "#export {}", schema.fields.iter().map(|f| f.export).collect::<Vec<_>>().join("|")).unwrap();
    writeln!(s, "#field {}", schema.fields.iter().map(|f| f.name).collect::<Vec<_>>().join("|")).unwrap();
    writeln!(s, "---").unwrap();
    for row in rows {
        writeln!(s, "{}", row.join("|")).unwrap();
    }
    s
}

fn schema_to_tbl_const(schema: &ConstSchema) -> String {
    let mut s = String::new();
    writeln!(s, "#!tbl v2").unwrap();
    writeln!(s, "#mode constant").unwrap();
    writeln!(s, "---").unwrap();
    for e in &schema.entries {
        writeln!(s, "{}|{}|{}|{}|{}", e.name, e.tbl_type, e.value, e.export, e.desc).unwrap();
    }
    s
}

// === 从 Schema 生成 TestMain.java ===

fn is_server_field(export: &str) -> bool {
    export == "前后端" || export == "服务器"
}

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

fn schema_to_test_main(data: &TestData, pkg: &str) -> String {
    let mut s = String::new();
    writeln!(s, "import {}.*;", pkg).unwrap();

    let groups: Vec<&str> = data.tables.iter().map(|(t, _)| t.group)
        .chain(data.constants.iter().map(|c| c.group))
        .collect::<std::collections::HashSet<_>>().into_iter().collect();
    for g in &groups {
        writeln!(s, "import {}.{}.*;", pkg, g).unwrap();
    }
    writeln!(s, "import java.util.Map;").unwrap();
    writeln!(s).unwrap();
    writeln!(s, "public class TestMain {{").unwrap();
    writeln!(s, "    public static void main(String[] args) {{").unwrap();
    writeln!(s, "        String dataDir = args.length > 0 ? args[0] : \"gen/server/data\";").unwrap();
    writeln!(s, "        TplHolder.init(dataDir);").unwrap();

    for (schema, _) in &data.tables {
        let cls = format!("{}Tpl", schema.name);
        let server_fields: Vec<&FieldSchema> = schema.fields.iter()
            .filter(|f| is_server_field(f.export) && f.name != schema.index)
            .collect();

        writeln!(s).unwrap();
        writeln!(s, "        System.out.println(\"=== {} ===\");", schema.name).unwrap();
        writeln!(s, "        Map<Integer, {}> map{} = TplHolder.getAll({}.class);", cls, schema.name, cls).unwrap();
        writeln!(s, "        for (var entry : map{}.entrySet()) {{", schema.name).unwrap();
        writeln!(s, "            var h = entry.getValue();").unwrap();

        let mut fmt_parts = vec![format!("id={}", java_format_spec("int"))];
        let mut arg_parts = vec!["h.getId()".to_string()];
        for f in &server_fields {
            let spec = java_format_spec(f.tbl_type);
            fmt_parts.push(format!("{}={}", to_camel(f.name), spec));
            arg_parts.push(format!("h.get{}()", to_pascal(f.name)));
        }

        writeln!(s, "            System.out.printf(\"{}%n\",", fmt_parts.join(" ")).unwrap();
        writeln!(s, "                {});", arg_parts.join(", ")).unwrap();
        writeln!(s, "        }}").unwrap();
    }

    for cschema in &data.constants {
        let cls = format!("{}Tpl", cschema.name);
        let server_entries: Vec<&ConstEntrySchema> = cschema.entries.iter()
            .filter(|e| is_server_field(e.export))
            .collect();

        writeln!(s).unwrap();
        writeln!(s, "        System.out.println(\"=== {} ===\");", cschema.name).unwrap();
        writeln!(s, "        var gc = TplHolder.getConst({}.class);", cls).unwrap();

        let mut fmt_parts = Vec::new();
        let mut arg_parts = Vec::new();
        for e in &server_entries {
            let spec = java_format_spec(e.tbl_type);
            fmt_parts.push(format!("{}={}", to_camel(e.name), spec));
            arg_parts.push(format!("gc.get{}()", to_pascal(e.name)));
        }

        writeln!(s, "        System.out.printf(\"{}%n\",", fmt_parts.join(" ")).unwrap();
        writeln!(s, "            {});", arg_parts.join(", ")).unwrap();
    }

    writeln!(s, "    }}").unwrap();
    writeln!(s, "}}").unwrap();
    s
}

// === 公开 API ===

/// 生成完整测试环境：.tbl 数据 + TestMain.java
pub fn generate_test_config(config_dir: &Path, opts: &TestGenOptions) {
    let data = build_schema(opts);

    for (schema, rows) in &data.tables {
        let dir = config_dir.join(schema.group);
        let _ = std::fs::create_dir_all(&dir);
        let content = if opts.seed > 0 || opts.rows > 0 {
            build_random_table(schema, opts)
        } else {
            schema_to_tbl_table(schema, rows)
        };
        let _ = std::fs::write(dir.join(format!("{}.tbl", schema.name)), &content);
    }

    for cschema in &data.constants {
        let dir = config_dir.join(cschema.group);
        let _ = std::fs::create_dir_all(&dir);
        let content = schema_to_tbl_const(cschema);
        let _ = std::fs::write(dir.join(format!("{}.tbl", cschema.name)), &content);
    }
}

/// 生成 TestMain.java
pub fn generate_test_main(workdir: &Path, opts: &TestGenOptions, pkg: &str) {
    let data = build_schema(opts);
    let content = schema_to_test_main(&data, pkg);
    let _ = std::fs::write(workdir.join("TestMain.java"), &content);
}

// === 随机数据生成 ===

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

fn build_random_table(schema: &TableSchema, opts: &TestGenOptions) -> String {
    let mut rng = SimpleRng::new(if opts.seed > 0 { opts.seed } else { 42 });
    let rows = if opts.rows > 0 { opts.rows } else { 10 };
    let names = ["战士", "法师", "弓手", "刺客", "牧师", "骑士", "猎人", "术士", "武僧", "德鲁伊"];

    let mut data_rows: Vec<Vec<&str>> = Vec::new();
    let mut owned: Vec<Vec<String>> = Vec::new();

    for i in 0..rows {
        let mut row = Vec::new();
        for f in &schema.fields {
            let val = match f.name {
                "id" => format!("{}", 1001 + i as i64),
                "name" => names[i % names.len()].to_string(),
                _ if opts.include_empty && rng.bool_chance(30) => String::new(),
                _ => match f.tbl_type {
                    "int" => rng.range(1, 200).to_string(),
                    "str" => format!("{}_{}", f.name, i + 1),
                    t if t.starts_with("List<") => {
                        let count = rng.range(1, 5) as usize;
                        (0..count).map(|_| rng.range(1, 100).to_string()).collect::<Vec<_>>().join(";")
                    }
                    _ => rng.range(1, 100).to_string(),
                },
            };
            row.push(val);
        }
        owned.push(row);
    }

    for row in &owned {
        data_rows.push(row.iter().map(|s| s.as_str()).collect());
    }

    schema_to_tbl_table(schema, &data_rows)
}
