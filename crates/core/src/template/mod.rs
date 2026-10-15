//! 项目模板（Schema Template）模块。
//!
//! 文档对应：@02 项目模板 / @07 S15-B。
//!
//! 模板 = `.tblschema` 文件（多 section 的结构定义，无数据）。
//! 该模块定义模板的统一抽象 `TemplateSource`，并提供：
//!
//! - 内置模板源（`BuiltinTemplates`，由 `crates/core/schemas/*.tblschema` 通过 `include_str!` 嵌入）
//! - 本地模板源（`LocalTemplates`，扫程序根目录 `<binary-dir>/tblschema/`）
//! - `instantiate_template`：把 schema 翻译为空 .tbl 文件集合 + project.tblschema，落到 target_dir
//!
//! UI 端的"模板库"对话框按统一接口列出多个 source，详见 @04.6.6。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::tblschema::{
    serialize_tblschema, TblSchema,
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

/// 把 schema 展开成空骨架落盘：`<target>/project.tblschema` + `<target>/config/<group>/<name>.tbl`。
///
/// 用于「从模板 / schema 建项目」的落盘，以及 CLI 的 `util scaffold`（输出到任意目录，
/// 不进 `projects/`）。
///
/// **`.tbl` 文本只有一个生成器**：这里走 `apply_schema_to_project`（sections → 内存节点）
/// 再交给 `tbl::serialize_*`，和界面上保存出来的文件逐字一致。以前这里另有一套
/// `render_*_skeleton`，两份渲染器已经漂了——`export` 一边写短码一边写中文（解析等价，
/// 因为 `Export::from_str` 两种都收），而 `desc` 一边转义一边不转义。
///
/// target_dir 必须不存在或为空目录。先全部渲染到内存再落盘，失败不留半成品。
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

    // 与建项目同一条路：sections → 内存节点（preset 作为初始数据灌入）
    let mut groups: Vec<crate::model::Group> = Vec::new();
    let config_dir = target_dir.join("config");
    crate::tblschema::apply_schema_to_project(&mut groups, &schema.sections, &config_dir, true);

    // 实例化后的 schema 不保留 preset（数据已经进了 .tbl）
    let mut schema_without_preset = schema.clone();
    for sec in &mut schema_without_preset.sections {
        sec.preset.clear();
    }

    let mut planned: Vec<(PathBuf, String)> = Vec::new();
    planned.push((
        target_dir.join(crate::project::PROJECT_SCHEMA_FILE),
        serialize_tblschema(&schema_without_preset),
    ));
    for g in &groups {
        for t in &g.tables { planned.push((t.path.clone(), crate::tbl::serialize_table(t))); }
        for c in &g.constants { planned.push((c.path.clone(), crate::tbl::serialize_constant(c))); }
        for e in &g.enums { planned.push((e.path.clone(), crate::tbl::serialize_enum(e))); }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tblschema::{
        fill_metadata_defaults, parse_tblschema, SchemaField, SchemaMetadata, SchemaMode, SchemaSection,
    };
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
            separators: Default::default(),
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
                    preset: Vec::new(),
                    display: None,
                },
                SchemaSection {
                    group: "hero".to_string(),
                    name: "HeroType".to_string(),
                    mode: SchemaMode::Enum,
                    // 新范式：enum 段头不带 fields，行数据全在 preset
                    fields: Vec::new(),
                    preset: vec![
                        vec!["1".to_string(), "WARRIOR".to_string(), "战士".to_string()],
                        vec!["2".to_string(), "MAGE".to_string(),    "法师".to_string()],
                    ],
                    display: None,
                },
                SchemaSection {
                    group: "global".to_string(),
                    name: "GlobalConst".to_string(),
                    mode: SchemaMode::Constant,
                    // 新范式：constant 段头不带 fields，行数据全在 preset
                    fields: Vec::new(),
                    preset: vec![vec![
                        "max_level".to_string(),
                        "int".to_string(),
                        "100".to_string(),
                        "cs".to_string(),
                        "最大等级".to_string(),
                    ]],
                    display: None,
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

        // 验证：实例化后的 project.tblschema 不应包含 # @preset 块
        assert!(!schema_text.contains("# @preset"),
            "项目实例化后 project.tblschema 不应包含 preset 数据");

        // Table 骨架：4 行表头 + ---，没有数据行
        let hero_base = std::fs::read_to_string(target.join("config/hero/HeroBase.tbl"))
            .expect("read HeroBase");
        assert!(hero_base.contains("#mode table"));
        assert!(hero_base.contains("@field hp"));
        assert!(hero_base.contains("@field id"));
        assert!(hero_base.trim().contains("---"));

        // Enum 骨架：写入条目
        let hero_type = std::fs::read_to_string(target.join("config/hero/HeroType.tbl"))
            .expect("read HeroType");
        assert!(hero_type.contains("1|WARRIOR|战士"));
        assert!(hero_type.contains("2|MAGE|法师"));

        // Constant 骨架：从 preset 灌入预设 value。
        // export 写的是**短码** `cs`——与 `tbl::serialize_constant` 一致（以前这里
        // 写的是展示用的中文"前后端"）。两种写法 `Export::from_str` 都收，所以这里
        // 顺便把它读回来断言语义，而不是只比字面。
        let global =
            std::fs::read_to_string(target.join("config/global/GlobalConst.tbl"))
                .expect("read const");
        assert!(global.contains("#mode constant"));
        assert!(global.contains("max_level|int|100|cs|最大等级"), "got:\n{}", global);
        let back = crate::tbl::parse_tbl(&target.join("config/global/GlobalConst.tbl"))
            .expect("parse const");
        match back {
            crate::tbl::TblFile::Constant(c) => assert_eq!(
                c.entries[0].export, crate::model::Export::ClientServer,
                "export 短码应当解析回双端导出"
            ),
            _ => panic!("应为 Constant"),
        }

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
    fn instantiate_writes_table_preset_records() {
        let target = unique_tmp("table_preset");
        let mut schema = TblSchema::default();
        schema.meta.id = "demo".into();
        schema.sections.push(SchemaSection {
            group: "hero".into(),
            name: "HeroBase".into(),
            mode: SchemaMode::Table,
            fields: vec![
                SchemaField { name: "id".into(),   tbl_type: "int".into(), export: "cs".into(), desc: "ID".into() },
                SchemaField { name: "name".into(), tbl_type: "str".into(), export: "cs".into(), desc: "名".into() },
            ],
            preset: vec![
                vec!["1".into(), "Alice".into()],
                vec!["2".into(), "Bob".into()],
            ],
            display: None,
        });
        instantiate_template(&schema, &target).expect("instantiate");
        let txt = std::fs::read_to_string(target.join("config/hero/HeroBase.tbl")).expect("read tbl");
        assert!(txt.contains("---"));
        assert!(txt.contains("[1]"));
        assert!(txt.contains("  name:Alice"));
        assert!(txt.contains("[2]"));
        assert!(txt.contains("  name:Bob"));
        let _ = std::fs::remove_dir_all(&target);
    }

    #[test]
    fn instantiate_writes_enum_preset() {
        let target = unique_tmp("enum_preset");
        let mut schema = TblSchema::default();
        schema.meta.id = "demo".into();
        schema.sections.push(SchemaSection {
            group: "hero".into(),
            name: "HeroType".into(),
            mode: SchemaMode::Enum,
            fields: Vec::new(),
            preset: vec![
                vec!["1".into(), "WAR".into(), "战士".into()],
                vec!["2".into(), "MAGE".into(), "法师".into()],
            ],
            display: None,
        });
        instantiate_template(&schema, &target).expect("instantiate");
        let txt = std::fs::read_to_string(target.join("config/hero/HeroType.tbl")).expect("read");
        assert!(txt.contains("1|WAR|战士"));
        assert!(txt.contains("2|MAGE|法师"));
        let _ = std::fs::remove_dir_all(&target);
    }

    #[test]
    fn instantiate_constant_no_preset_writes_only_header() {
        // 新范式：constant 段头不带 fields，没 preset 时项目里只有空 constant 节点
        let target = unique_tmp("const_empty");
        let mut schema = TblSchema::default();
        schema.meta.id = "demo".into();
        schema.sections.push(SchemaSection {
            group: "g".into(),
            name: "C".into(),
            mode: SchemaMode::Constant,
            fields: Vec::new(),
            preset: Vec::new(),
            display: None,
        });
        instantiate_template(&schema, &target).expect("instantiate");
        let txt = std::fs::read_to_string(target.join("config/g/C.tbl")).expect("read");
        assert!(txt.contains("#mode constant"));
        assert!(txt.trim().ends_with("---"), "txt = {:?}", txt);
        let _ = std::fs::remove_dir_all(&target);
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
