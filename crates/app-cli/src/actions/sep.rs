//! sep show: 展示分隔符配置（25 键），支持 --defaults / --config / --schema。

use std::path::Path;

use anyhow::Result;
use tablet_core::tblschema::parse_tblschema;
use tablet_core::types::{SepKey, SeparatorsSection};

#[derive(serde::Serialize)]
pub struct SepEntry {
    pub key: &'static str,
    pub value: String,
    pub source: &'static str,
}

#[derive(serde::Serialize)]
pub struct SepShowSummary {
    pub entries: Vec<SepEntry>,
}

pub fn run_sep_show(
    defaults_only: bool,
    config_path: Option<&Path>,
    schema_path: Option<&Path>,
) -> Result<SepShowSummary> {
    let builtin = SeparatorsSection::default();

    if defaults_only {
        let entries = SepKey::ALL.iter().map(|k| SepEntry {
            key: k.as_directive_key(),
            value: k.get(&builtin).to_string(),
            source: "默认",
        }).collect();
        return Ok(SepShowSummary { entries });
    }

    let mut from_config: Option<SeparatorsSection> = None;
    if let Some(path) = config_path {
        let text = std::fs::read_to_string(path)?;
        #[derive(serde::Deserialize)]
        struct Partial {
            #[serde(default)]
            separators: SeparatorsSection,
        }
        let parsed: Partial = toml::from_str(&text)?;
        from_config = Some(parsed.separators);
    }

    let mut from_schema: Option<SeparatorsSection> = None;
    if let Some(path) = schema_path {
        let text = std::fs::read_to_string(path)?;
        let schema = parse_tblschema(&text)?;
        from_schema = Some(schema.separators);
    }

    let entries = SepKey::ALL.iter().map(|k| {
        let default_val = k.get(&builtin);
        let (value, source) = if let Some(ref ss) = from_schema {
            let v = k.get(ss);
            if v != default_val {
                (v.to_string(), "schema")
            } else if let Some(ref cs) = from_config {
                let cv = k.get(cs);
                if cv != default_val {
                    (cv.to_string(), "config")
                } else {
                    (default_val.to_string(), "默认")
                }
            } else {
                (v.to_string(), "默认")
            }
        } else if let Some(ref cs) = from_config {
            let cv = k.get(cs);
            if cv != default_val {
                (cv.to_string(), "config")
            } else {
                (default_val.to_string(), "默认")
            }
        } else {
            (default_val.to_string(), "默认")
        };
        SepEntry { key: k.as_directive_key(), value, source }
    }).collect();

    Ok(SepShowSummary { entries })
}
