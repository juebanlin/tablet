use std::path::Path;
use anyhow::Result;
use crate::model::*;
use crate::tbl::{self, TblFile};

const DEFAULT_CONFIG: &str = r#"[project]
# 项目名称
name = "my-game"
# .tbl 配置文件目录（相对于本文件所在目录）
config_dir = "config"
# 临时缓存目录（xlsx 缓存等）
cache_dir = ".tbl-cache"

[export.server]
# 后端语言
lang = "java"
# Java 包名
package = "com.game.config"
# 后端数据文件输出目录
data_output = "gen/server/data"
# 后端模板类输出目录
code_output = "gen/server/code"

[export.client]
# 前端语言
lang = "lua"
# 前端文件输出目录
output = "gen/client"

[ui]
# 点击空白区域时自动保存当前编辑 (false 则只有回车才保存)
auto_commit_on_blur = true
# 编辑单元格时实时验证 (true 则每次修改立即检查)
realtime_validate = false
# 日志文件级别: debug, info, warn, error
log_level = "debug"

[separators]
# 类型分隔符配置（按范式），默认值覆盖绝大多数场景
Tuple2 = ","
Tuple3 = ","
Tuple4 = ","
List = ";"
Set = ";"

[separators.Map]
kv = ":"
entry = ";"

[separators.List_Tuple2]
tuple = ","
list = ";"

[separators.List_Tuple3]
tuple = ","
list = ";"

[separators.List_Tuple4]
tuple = ","
list = ";"

[separators.Map_Tuple2]
kv = ":"
tuple = ","
entry = ";"

[separators.Map_Tuple3]
kv = ":"
tuple = ","
entry = ";"

[separators.Map_Tuple4]
kv = ":"
tuple = ","
entry = ";"

[separators.Map_List]
kv = ":"
item = ","
entry = ";"
"#;

pub fn load_project(workdir: &Path) -> Result<Project> {
    let config_path = workdir.join(crate::CONFIG_FILE);

    if !config_path.exists() {
        std::fs::write(&config_path, DEFAULT_CONFIG)?;
        println!("已生成默认配置文件: {}", config_path.display());
    }

    let config_str = std::fs::read_to_string(&config_path)?;
    let config: ProjectConfig = toml::from_str(&config_str)?;

    let config_dir = workdir.join(&config.project.config_dir);
    let mut groups = Vec::new();

    if config_dir.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(&config_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .collect();
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            let dir = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let group = load_group(&name, &dir)?;
            groups.push(group);
        }
    } else {
        std::fs::create_dir_all(&config_dir)?;
        println!("已创建配置目录: {}", config_dir.display());
    }

    Ok(Project { workdir: workdir.to_path_buf(), config, groups })
}

fn load_group(name: &str, dir: &Path) -> Result<Group> {
    let mut tables = Vec::new();
    let mut constants = Vec::new();

    let mut entries: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "tbl"))
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        match tbl::parse_tbl(&path) {
            Ok(TblFile::Table(t)) => tables.push(t),
            Ok(TblFile::Constant(c)) => constants.push(c),
            Err(e) => eprintln!("warn: failed to parse {}: {}", path.display(), e),
        }
    }

    Ok(Group {
        name: name.to_string(),
        dir: dir.to_path_buf(),
        tables,
        constants,
        is_new: false,
    })
}
