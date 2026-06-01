//! 项目模板（Schema Template）模块。
//!
//! 文档对应：@02 项目模板 / @08 S15-B。
//!
//! 模板 = `.tblschema` 文件（多 section 的结构定义，无数据）。
//! 该模块定义模板的统一抽象 `TemplateSource`，并提供：
//!
//! - 内置模板源（`BuiltinTemplates`，由 `crates/core/schemas/*.tblschema` 通过 `include_str!` 嵌入）
//! - 本地模板源（`LocalTemplates`，扫程序根目录 `<binary-dir>/tblschema/`）
//! - `instantiate_template`：把 schema 翻译为空 .tbl 文件集合 + project.tblschema，落到 target_dir
//!
//! UI 端的"模板库"对话框按统一接口列出多个 source，详见 @04.6.6。

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::tblschema::{
    serialize_tblschema, SchemaMode, SchemaSection, TblSchema,
};

mod builtin;
mod local;

pub use builtin::BuiltinTemplates;
pub use local::{default_local_dir, LocalTemplates};

/// 模板的轻量描述（不含 schema 内容；列表展示用）。
#[derive(Debug, Clone)]
pub struct TemplateMeta {
    pub id: String,
    pub name: String,
    pub category: String,
    pub version: String,
    /// 来源标签：`"builtin"` / `"local"` / `"remote"`。便于 UI 区分。
    pub source: &'static str,
}

impl TemplateMeta {
    pub fn from_schema(schema: &TblSchema, source: &'static str) -> Self {
        Self {
            id: schema.meta.id.clone(),
            name: if schema.meta.name.is_empty() {
                schema.meta.id.clone()
            } else {
                schema.meta.name.clone()
            },
            category: schema.meta.category.clone(),
            version: schema.meta.version.clone(),
            source,
        }
    }
}

/// 模板的完整内容：原始 .tblschema 文本 + 解析后的 schema。
#[derive(Debug, Clone)]
pub struct TemplateContent {
    pub meta: TemplateMeta,
    pub raw: String,
    pub schema: TblSchema,
}

/// 模板源统一接口。
///
/// 所有实现的 `list()` / `load_by_id()` 都不应触碰文件系统外的内容（如网络），
/// 网络源单独有 `RemoteTemplates`（S15-H）。
pub trait TemplateSource {
    /// 列出该源下所有模板的元信息。
    fn list(&self) -> Vec<TemplateMeta>;

    /// 按 id 加载模板完整内容。找不到返回 None。
    fn load_by_id(&self, id: &str) -> Option<TemplateContent>;
}

/// 把 schema 实例化为目标目录下的项目骨架：
/// - `<target>/project.tblschema` 写入完整 schema（含 meta）
/// - `<target>/config/<group>/<Name>.tbl` 按 mode 写入空骨架
///   - Table：表头 4 行写好，数据行为空
///   - Constant：每个常量行 `name | type | <empty> | export | desc`
///   - Enum：枚举条目直接写入（来自 schema）
///
/// target_dir 必须不存在或为空目录。失败回滚（不写半成品）。
pub fn instantiate_template(schema: &TblSchema, target_dir: &Path) -> Result<()> {
    if target_dir.exists() {
        let mut entries = std::fs::read_dir(target_dir)
            .with_context(|| format!("读取目标目录失败: {}", target_dir.display()))?;
        if entries.next().is_some() {
            bail!("目标目录非空: {}", target_dir.display());
        }
    } else {
        std::fs::create_dir_all(target_dir)
            .with_context(|| format!("创建目标目录失败: {}", target_dir.display()))?;
    }

    // 先 dry-run 全部内容到内存，确认无错才落盘——失败时尽量不留残留。
    let schema_text = serialize_tblschema(schema);

    let mut planned: Vec<(PathBuf, String)> = Vec::new();
    planned.push((target_dir.join(crate::project::PROJECT_SCHEMA_FILE), schema_text));

    let config_dir = target_dir.join("config");
    for sec in &schema.sections {
        let dir = config_dir.join(&sec.group);
        let path = dir.join(format!("{}.tbl", sec.name));
        let content = render_tbl_skeleton(sec);
        planned.push((path, content));
    }

    for (path, content) in &planned {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("创建目录失败: {}", parent.display()))?;
        }
        std::fs::write(path, content)
            .with_context(|| format!("写入失败: {}", path.display()))?;
    }

    Ok(())
}

fn render_tbl_skeleton(sec: &SchemaSection) -> String {
    match sec.mode {
        SchemaMode::Table => render_table_skeleton(sec),
        SchemaMode::Constant => render_constant_skeleton(sec),
        SchemaMode::Enum => render_enum_skeleton(sec),
    }
}

fn render_table_skeleton(sec: &SchemaSection) -> String {
    let mut s = String::new();
    writeln!(s, "#!tbl v2").unwrap();
    writeln!(s, "#mode table").unwrap();
    writeln!(
        s,
        "#desc {}",
        sec.fields
            .iter()
            .map(|f| f.desc.as_str())
            .collect::<Vec<_>>()
            .join("|")
    )
    .unwrap();
    writeln!(
        s,
        "#export {}",
        sec.fields
            .iter()
            .map(|f| f.export_display())
            .collect::<Vec<_>>()
            .join("|")
    )
    .unwrap();
    writeln!(
        s,
        "#type {}",
        sec.fields
            .iter()
            .map(|f| f.tbl_type.as_str())
            .collect::<Vec<_>>()
            .join("|")
    )
    .unwrap();
    writeln!(
        s,
        "#field {}",
        sec.fields
            .iter()
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>()
            .join("|")
    )
    .unwrap();
    writeln!(s, "---").unwrap();
    s
}

fn render_constant_skeleton(sec: &SchemaSection) -> String {
    let mut s = String::new();
    writeln!(s, "#!tbl v2").unwrap();
    writeln!(s, "#mode constant").unwrap();
    writeln!(s, "---").unwrap();
    for f in &sec.fields {
        // value 留空；export 缺省 → "前后端"
        let export = if f.export.is_empty() { "cs" } else { f.export.as_str() };
        writeln!(s, "{}|{}||{}|{}", f.name, f.tbl_type, display_export(export), f.desc).unwrap();
    }
    s
}

fn render_enum_skeleton(sec: &SchemaSection) -> String {
    let mut s = String::new();
    writeln!(s, "#!tbl v2").unwrap();
    writeln!(s, "#mode enum").unwrap();
    writeln!(s, "---").unwrap();
    for f in &sec.fields {
        // sec.fields: tbl_type 借位存 id；name 存条目名；desc 存描述
        writeln!(s, "{}|{}|{}", f.tbl_type, f.name, f.desc).unwrap();
    }
    s
}

fn display_export(code: &str) -> &str {
    match code {
        "cs" | "" => "前后端",
        "c" => "客户端",
        "s" => "服务器",
        "-" => "不导出",
        _ => "前后端",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tblschema::{fill_metadata_defaults, parse_tblschema, SchemaField, SchemaMetadata};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn unique_tmp(label: &str) -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("tblschema_tpl_{}_{}_{}", label, std::process::id(), n))
    }

    fn make_schema_with(meta_id: &str) -> TblSchema {
        let mut meta = SchemaMetadata::default();
        meta.id = meta_id.to_string();
        meta.name = format!("name-{}", meta_id);
        meta.version = "0.1.0".to_string();
        TblSchema {
            meta,
            sections: vec![
                SchemaSection {
                    group: "hero".to_string(),
                    name: "HeroBase".to_string(),
                    mode: SchemaMode::Table,
                    fields: vec![
                        SchemaField {
                            name: "id".to_string(),
                            tbl_type: "int".to_string(),
                            export: "cs".to_string(),
                            desc: "英雄ID".to_string(),
                        },
                        SchemaField {
                            name: "hp".to_string(),
                            tbl_type: "int".to_string(),
                            export: "s".to_string(),
                            desc: "血量".to_string(),
                        },
                    ],
                },
                SchemaSection {
                    group: "hero".to_string(),
                    name: "HeroType".to_string(),
                    mode: SchemaMode::Enum,
                    fields: vec![
                        SchemaField {
                            name: "WARRIOR".to_string(),
                            tbl_type: "1".to_string(),
                            export: String::new(),
                            desc: "战士".to_string(),
                        },
                        SchemaField {
                            name: "MAGE".to_string(),
                            tbl_type: "2".to_string(),
                            export: String::new(),
                            desc: "法师".to_string(),
                        },
                    ],
                },
                SchemaSection {
                    group: "global".to_string(),
                    name: "GlobalConst".to_string(),
                    mode: SchemaMode::Constant,
                    fields: vec![SchemaField {
                        name: "max_level".to_string(),
                        tbl_type: "int".to_string(),
                        export: "cs".to_string(),
                        desc: "最大等级".to_string(),
                    }],
                },
            ],
        }
    }

    #[test]
    fn instantiate_writes_skeleton_files() {
        let target = unique_tmp("ok");
        let schema = make_schema_with("demo");
        instantiate_template(&schema, &target).expect("instantiate");

        // project.tblschema 写入了带 meta 的内容
        let schema_text =
            std::fs::read_to_string(target.join("project.tblschema")).expect("read schema");
        assert!(schema_text.contains("# @meta id: demo"));
        assert!(schema_text.contains("[hero/HeroBase] table"));

        // Table 骨架：4 行表头 + ---，没有数据行
        let hero_base = std::fs::read_to_string(target.join("config/hero/HeroBase.tbl"))
            .expect("read HeroBase");
        assert!(hero_base.contains("#mode table"));
        assert!(hero_base.contains("#field id|hp"));
        assert!(hero_base.trim().ends_with("---"));

        // Enum 骨架：写入条目
        let hero_type = std::fs::read_to_string(target.join("config/hero/HeroType.tbl"))
            .expect("read HeroType");
        assert!(hero_type.contains("1|WARRIOR|战士"));
        assert!(hero_type.contains("2|MAGE|法师"));

        // Constant 骨架：value 留空
        let global =
            std::fs::read_to_string(target.join("config/global/GlobalConst.tbl"))
                .expect("read const");
        assert!(global.contains("#mode constant"));
        assert!(global.contains("max_level|int||前后端|最大等级"));

        let _ = std::fs::remove_dir_all(&target);
    }

    #[test]
    fn instantiate_rejects_non_empty_dir() {
        let target = unique_tmp("nonempty");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("squat"), "x").unwrap();

        let schema = make_schema_with("demo");
        let err = instantiate_template(&schema, &target).expect_err("should fail");
        assert!(err.to_string().contains("非空"));

        let _ = std::fs::remove_dir_all(&target);
    }

    #[test]
    fn instantiate_creates_missing_dir() {
        let target = unique_tmp("missing");
        // 不预先创建
        let schema = make_schema_with("demo");
        instantiate_template(&schema, &target).expect("instantiate");
        assert!(target.join("project.tblschema").exists());
        let _ = std::fs::remove_dir_all(&target);
    }

    #[test]
    fn template_meta_from_schema_uses_id_when_name_missing() {
        let mut schema = TblSchema::default();
        schema.meta.id = "x".to_string();
        // name 为空
        let m = TemplateMeta::from_schema(&schema, "builtin");
        assert_eq!(m.id, "x");
        assert_eq!(m.name, "x");
    }

    #[test]
    fn fill_defaults_used_in_legacy_parse() {
        // 确认上层调用 fill_metadata_defaults 后，TemplateMeta::from_schema 能拿到 id
        let mut schema = parse_tblschema("#!tblschema v1\n[g/N] table\nid|int|cs|x\n").unwrap();
        fill_metadata_defaults(&mut schema, "stem");
        let m = TemplateMeta::from_schema(&schema, "local");
        assert_eq!(m.id, "stem");
        assert_eq!(m.name, "stem");
    }
}
