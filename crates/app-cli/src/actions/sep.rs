//! sep show: 展示分隔符配置（25 键）。
//!
//! 每个 leaf 独立回退：从高优先级往低找，取第一个「写过」它的来源。
//! 优先级（高 → 低）：`--schema`（.tblschema）→ `--config`（tablet.toml）
//! → 当前项目 schema → 内置默认。
//!
//! 注意这里**不是**「全局默认值与项目值合并」——`tablet.toml [separators]` 早已降级为
//! 「新建空项目时拷贝一次的模板」（@01.7.4），运行期项目的分隔符只有一个来源：
//! 该项目自己的 `project.tblschema`。`--config` 传的是**指定文件**，不是全局配置。

use std::path::Path;

use anyhow::Result;
use tablet_core::tblschema::parse_tblschema;
use tablet_core::types::{SepKey, SeparatorsSection};

/// 当前项目这一层。带上 id 是为了多项目工作区里能说清「展示的是哪个项目」。
pub struct ProjectSep<'a> {
    pub id: &'a str,
    pub separators: &'a SeparatorsSection,
}

#[derive(serde::Serialize)]
pub struct SepEntry {
    pub key: &'static str,
    pub value: String,
    pub source: &'static str,
}

#[derive(serde::Serialize)]
pub struct SepShowSummary {
    /// 有项目上下文时是解析到的项目 id（`--project` 或 active），否则 None。
    pub project: Option<String>,
    pub entries: Vec<SepEntry>,
}

fn load_config_separators(path: &Path) -> Result<SeparatorsSection> {
    let text = std::fs::read_to_string(path)?;
    #[derive(serde::Deserialize)]
    struct Partial {
        #[serde(default)]
        separators: SeparatorsSection,
    }
    Ok(toml::from_str::<Partial>(&text)?.separators)
}

fn load_schema_separators(path: &Path) -> Result<SeparatorsSection> {
    let text = std::fs::read_to_string(path)?;
    Ok(parse_tblschema(&text)?.separators)
}

pub fn run_sep_show(
    defaults_only: bool,
    project: Option<ProjectSep<'_>>,
    config_path: Option<&Path>,
    schema_path: Option<&Path>,
) -> Result<SepShowSummary> {
    let builtin = SeparatorsSection::default();
    let project_id = project.as_ref().map(|p| p.id.to_string());

    // 一律以「内置默认」打底；--defaults 时到此为止，不看任何来源
    let mut layers: Vec<(SeparatorsSection, &'static str)> = vec![(builtin.clone(), "默认")];
    if !defaults_only {
        if let Some(p) = &project {
            layers.push((p.separators.clone(), "项目"));
        }
        if let Some(path) = config_path {
            layers.push((load_config_separators(path)?, "config"));
        }
        if let Some(path) = schema_path {
            layers.push((load_schema_separators(path)?, "schema"));
        }
    }

    let entries = SepKey::ALL.iter().map(|k| {
        let default_val = k.get(&builtin);
        let (value, source) = layers.iter().skip(1)
            .filter(|(section, _)| k.get(section) != default_val)
            .last()
            .map(|(section, label)| (k.get(section).to_string(), *label))
            .unwrap_or_else(|| (default_val.to_string(), "默认"));
        SepEntry { key: k.as_directive_key(), value, source }
    }).collect();

    Ok(SepShowSummary { project: project_id, entries })
}
