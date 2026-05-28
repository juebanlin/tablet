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
    fn default() -> Self {
        Self::full()
    }
}

impl TestGenOptions {
    pub fn full() -> Self {
        Self {
            include_empty: false,
            include_collection: true,
            include_tuple: true,
            include_constant: true,
            rows: 0,
            seed: 0,
        }
    }

    pub fn with_empty() -> Self {
        Self {
            include_empty: true,
            include_collection: false,
            include_tuple: false,
            include_constant: false,
            rows: 0,
            seed: 0,
        }
    }
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

/// 根据选项生成测试 .tbl 文件到指定目录
pub fn generate_test_config(config_dir: &Path, opts: &TestGenOptions) {
    let hero_dir = config_dir.join("hero");
    let _ = std::fs::create_dir_all(&hero_dir);

    let hero_content = if opts.seed > 0 || opts.rows > 0 {
        build_hero_table_random(opts)
    } else {
        build_hero_table_fixed(opts)
    };
    let _ = std::fs::write(hero_dir.join("HeroBase.tbl"), &hero_content);

    if opts.include_constant {
        let global_dir = config_dir.join("global");
        let _ = std::fs::create_dir_all(&global_dir);
        let global_content = build_global_const(opts);
        let _ = std::fs::write(global_dir.join("GlobalConst.tbl"), &global_content);
    }
}

/// 生成 TestMain.java 验证代码到工作目录
pub fn generate_test_main(workdir: &Path, opts: &TestGenOptions, pkg: &str) {
    let content = build_test_main(opts, pkg);
    let _ = std::fs::write(workdir.join("TestMain.java"), &content);
}

fn build_test_main(opts: &TestGenOptions, pkg: &str) -> String {
    let mut s = String::new();
    writeln!(s, "import {}.*;", pkg).unwrap();
    writeln!(s, "import {}.hero.*;", pkg).unwrap();
    if opts.include_constant {
        writeln!(s, "import {}.global.*;", pkg).unwrap();
    }
    writeln!(s, "import java.util.Map;").unwrap();
    writeln!(s).unwrap();
    writeln!(s, "public class TestMain {{").unwrap();
    writeln!(s, "    public static void main(String[] args) {{").unwrap();
    writeln!(s, "        String dataDir = args.length > 0 ? args[0] : \"gen/server/data\";").unwrap();
    writeln!(s, "        TplHolder.init(dataDir);").unwrap();
    writeln!(s).unwrap();

    if opts.include_empty {
        writeln!(s, "        System.out.println(\"=== HeroBase (omit strategy) ===\");").unwrap();
        writeln!(s, "        Map<Integer, HeroBaseTpl> heroes = TplHolder.getAll(HeroBaseTpl.class);").unwrap();
        writeln!(s, "        for (var entry : heroes.entrySet()) {{").unwrap();
        writeln!(s, "            var h = entry.getValue();").unwrap();
        writeln!(s, "            System.out.printf(\"id=%d name=%s hp=%d desc=[%s]%n\",").unwrap();
        writeln!(s, "                h.getId(), h.getName(), h.getHp(), h.getDesc());").unwrap();
        writeln!(s, "        }}").unwrap();
    } else if opts.include_collection {
        writeln!(s, "        System.out.println(\"=== HeroBase ===\");").unwrap();
        writeln!(s, "        Map<Integer, HeroBaseTpl> heroes = TplHolder.getAll(HeroBaseTpl.class);").unwrap();
        writeln!(s, "        for (var entry : heroes.entrySet()) {{").unwrap();
        writeln!(s, "            var h = entry.getValue();").unwrap();
        writeln!(s, "            System.out.printf(\"id=%d name=%s hp=%d skills=%s%n\",").unwrap();
        writeln!(s, "                h.getId(), h.getName(), h.getHp(), h.getSkills());").unwrap();
        writeln!(s, "        }}").unwrap();
    } else {
        writeln!(s, "        System.out.println(\"=== HeroBase ===\");").unwrap();
        writeln!(s, "        Map<Integer, HeroBaseTpl> heroes = TplHolder.getAll(HeroBaseTpl.class);").unwrap();
        writeln!(s, "        for (var entry : heroes.entrySet()) {{").unwrap();
        writeln!(s, "            var h = entry.getValue();").unwrap();
        writeln!(s, "            System.out.printf(\"id=%d name=%s hp=%d%n\",").unwrap();
        writeln!(s, "                h.getId(), h.getName(), h.getHp());").unwrap();
        writeln!(s, "        }}").unwrap();
    }

    if opts.include_constant {
        writeln!(s).unwrap();
        writeln!(s, "        System.out.println(\"=== GlobalConst ===\");").unwrap();
        writeln!(s, "        var gc = TplHolder.getConst(GlobalConstTpl.class);").unwrap();
        if opts.include_tuple {
            writeln!(s, "        System.out.printf(\"maxLevel=%d serverName=%s startPos=%s%n\",").unwrap();
            writeln!(s, "            gc.getMaxLevel(), gc.getServerName(), gc.getStartPos());").unwrap();
        } else {
            writeln!(s, "        System.out.printf(\"maxLevel=%d serverName=%s%n\",").unwrap();
            writeln!(s, "            gc.getMaxLevel(), gc.getServerName());").unwrap();
        }
    }

    writeln!(s, "    }}").unwrap();
    writeln!(s, "}}").unwrap();
    s
}

fn build_hero_table_fixed(opts: &TestGenOptions) -> String {
    let mut s = String::new();
    writeln!(s, "#!tbl v2").unwrap();
    writeln!(s, "#mode table").unwrap();
    writeln!(s, "#index id").unwrap();

    if opts.include_empty {
        writeln!(s, "#desc 英雄ID|名称|血量|描述").unwrap();
        writeln!(s, "#type int|str|int|str").unwrap();
        writeln!(s, "#export 前后端|前后端|服务器|前后端").unwrap();
        writeln!(s, "#field id|name|hp|desc").unwrap();
        writeln!(s, "---").unwrap();
        writeln!(s, "1001|战士|100|").unwrap();
        writeln!(s, "1002|法师||测试空血量").unwrap();
    } else if opts.include_collection {
        writeln!(s, "#desc 英雄ID|名称|血量|技能组").unwrap();
        writeln!(s, "#type int|str|int|List<int>").unwrap();
        writeln!(s, "#export 前后端|前后端|服务器|前后端").unwrap();
        writeln!(s, "#field id|name|hp|skills").unwrap();
        writeln!(s, "---").unwrap();
        writeln!(s, "1001|战士|100|1;2;3").unwrap();
        writeln!(s, "1002|法师|80|4;5").unwrap();
        writeln!(s, "1003|弓手|90|6;7;8").unwrap();
    } else {
        writeln!(s, "#desc 英雄ID|名称|血量").unwrap();
        writeln!(s, "#type int|str|int").unwrap();
        writeln!(s, "#export 前后端|前后端|服务器").unwrap();
        writeln!(s, "#field id|name|hp").unwrap();
        writeln!(s, "---").unwrap();
        writeln!(s, "1001|战士|100").unwrap();
        writeln!(s, "1002|法师|80").unwrap();
    }
    s
}

fn build_hero_table_random(opts: &TestGenOptions) -> String {
    let mut rng = SimpleRng::new(if opts.seed > 0 { opts.seed } else { 42 });
    let rows = if opts.rows > 0 { opts.rows } else { 10 };
    let names = ["战士", "法师", "弓手", "刺客", "牧师", "骑士", "猎人", "术士", "武僧", "德鲁伊"];

    let mut s = String::new();
    writeln!(s, "#!tbl v2").unwrap();
    writeln!(s, "#mode table").unwrap();
    writeln!(s, "#index id").unwrap();

    if opts.include_empty && opts.include_collection {
        writeln!(s, "#desc 英雄ID|名称|血量|技能组|描述").unwrap();
        writeln!(s, "#type int|str|int|List<int>|str").unwrap();
        writeln!(s, "#export 前后端|前后端|服务器|前后端|前后端").unwrap();
        writeln!(s, "#field id|name|hp|skills|desc").unwrap();
        writeln!(s, "---").unwrap();
        for i in 0..rows {
            let id = 1001 + i as i64;
            let name = names[i % names.len()];
            let hp = if opts.include_empty && rng.bool_chance(30) {
                String::new()
            } else {
                rng.range(50, 200).to_string()
            };
            let skills = if opts.include_empty && rng.bool_chance(20) {
                String::new()
            } else {
                let count = rng.range(1, 5) as usize;
                (0..count).map(|_| rng.range(1, 100).to_string()).collect::<Vec<_>>().join(";")
            };
            let desc = if opts.include_empty && rng.bool_chance(50) {
                String::new()
            } else {
                format!("描述{}", i + 1)
            };
            writeln!(s, "{}|{}|{}|{}|{}", id, name, hp, skills, desc).unwrap();
        }
    } else if opts.include_collection {
        writeln!(s, "#desc 英雄ID|名称|血量|技能组").unwrap();
        writeln!(s, "#type int|str|int|List<int>").unwrap();
        writeln!(s, "#export 前后端|前后端|服务器|前后端").unwrap();
        writeln!(s, "#field id|name|hp|skills").unwrap();
        writeln!(s, "---").unwrap();
        for i in 0..rows {
            let id = 1001 + i as i64;
            let name = names[i % names.len()];
            let hp = rng.range(50, 200);
            let count = rng.range(1, 5) as usize;
            let skills: Vec<String> = (0..count).map(|_| rng.range(1, 100).to_string()).collect();
            writeln!(s, "{}|{}|{}|{}", id, name, hp, skills.join(";")).unwrap();
        }
    } else if opts.include_empty {
        writeln!(s, "#desc 英雄ID|名称|血量|描述").unwrap();
        writeln!(s, "#type int|str|int|str").unwrap();
        writeln!(s, "#export 前后端|前后端|服务器|前后端").unwrap();
        writeln!(s, "#field id|name|hp|desc").unwrap();
        writeln!(s, "---").unwrap();
        for i in 0..rows {
            let id = 1001 + i as i64;
            let name = names[i % names.len()];
            let hp = if rng.bool_chance(30) { String::new() } else { rng.range(50, 200).to_string() };
            let desc = if rng.bool_chance(50) { String::new() } else { format!("描述{}", i + 1) };
            writeln!(s, "{}|{}|{}|{}", id, name, hp, desc).unwrap();
        }
    } else {
        writeln!(s, "#desc 英雄ID|名称|血量").unwrap();
        writeln!(s, "#type int|str|int").unwrap();
        writeln!(s, "#export 前后端|前后端|服务器").unwrap();
        writeln!(s, "#field id|name|hp").unwrap();
        writeln!(s, "---").unwrap();
        for i in 0..rows {
            let id = 1001 + i as i64;
            let name = names[i % names.len()];
            let hp = rng.range(50, 200);
            writeln!(s, "{}|{}|{}", id, name, hp).unwrap();
        }
    }
    s
}

fn build_global_const(opts: &TestGenOptions) -> String {
    let mut s = String::new();
    writeln!(s, "#!tbl v2").unwrap();
    writeln!(s, "#mode constant").unwrap();
    writeln!(s, "---").unwrap();
    writeln!(s, "max_level|int|100|前后端|最大等级").unwrap();
    if opts.include_tuple {
        writeln!(s, "start_pos|Tuple2<int,int>|5,10|前后端|出生坐标").unwrap();
    }
    writeln!(s, "server_name|str|test-server|服务器|服务器名称").unwrap();
    s
}
