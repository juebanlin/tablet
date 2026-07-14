use std::collections::BTreeSet;
use std::fmt::Write;
use anyhow::Result;
use crate::model::*;
use crate::types::{SepKey, SeparatorsSection};
use super::{EmptyStrategy, LineEnding, to_camel_case};
use super::sep_meta::{collect_used_sep_keys_constant, collect_used_sep_keys_table};

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn attr_escape(s: &str) -> String {
    xml_escape(s)
}

fn is_server_export(export: &Export) -> bool {
    matches!(export, Export::ClientServer | Export::ServerOnly)
}

/// 仅按 `used_keys` 输出 sep_* attrs。空集返回空串 —— 根元素不带任何 sep_ attr。
fn sep_attrs(sep: &SeparatorsSection, used_keys: &BTreeSet<SepKey>) -> String {
    let mut s = String::new();
    for k in SepKey::ALL {
        if used_keys.contains(&k) {
            write!(s, " sep_{}=\"{}\"", k.as_export_key(), attr_escape(k.get(sep))).unwrap();
        }
    }
    s
}

pub fn export_table_xml(table: &Table, strategy: &EmptyStrategy, sep: &SeparatorsSection) -> String {
    let fields = &table.schema.fields;
    let export_cols: Vec<(usize, &FieldDef)> = fields.iter().enumerate()
        .filter(|(_, f)| is_server_export(&f.export))
        .collect();

    let used = collect_used_sep_keys_table(table);

    let mut s = String::new();
    writeln!(s, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>").unwrap();
    writeln!(s, "<list{}>", sep_attrs(sep, &used)).unwrap();

    for record in &table.records {
        writeln!(s, "  <item>").unwrap();
        for &(col, field) in &export_cols {
            let raw = record.get(col).map(|v| v.as_str()).unwrap_or("");
            let key = to_camel_case(&field.name);

            if raw.is_empty() {
                match strategy {
                    EmptyStrategy::Omit => continue,
                    _ => { writeln!(s, "    <{}></{}>", key, key).unwrap(); }
                }
            } else {
                writeln!(s, "    <{}>{}</{}>", key, xml_escape(raw), key).unwrap();
            }
        }
        writeln!(s, "  </item>").unwrap();
    }

    writeln!(s, "</list>").unwrap();
    s
}

pub fn export_constant_xml(constant: &Constant, strategy: &EmptyStrategy, sep: &SeparatorsSection) -> String {
    let used = collect_used_sep_keys_constant(constant);

    let mut s = String::new();
    writeln!(s, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>").unwrap();
    writeln!(s, "<const{}>", sep_attrs(sep, &used)).unwrap();

    for entry in &constant.entries {
        if !is_server_export(&entry.export) { continue; }
        if entry.name.is_empty() { continue; }

        let key = to_camel_case(&entry.name);

        if entry.value.is_empty() {
            match strategy {
                EmptyStrategy::Omit => continue,
                _ => { writeln!(s, "  <{}></{}>", key, key).unwrap(); }
            }
        } else {
            writeln!(s, "  <{}>{}</{}>", key, xml_escape(&entry.value), key).unwrap();
        }
    }

    writeln!(s, "</const>").unwrap();
    s
}

pub fn export_all_xml(project: &Project) -> Result<super::ExportResult> {
    let export_cfg = project.config.export.as_ref();

    let data_output = export_cfg
        .and_then(|e| e.server.as_ref())
        .and_then(|s| s.data_output.as_deref())
        .unwrap_or("gen/server/data");

    let strategy_str = export_cfg
        .and_then(|e| e.xml.as_ref())
        .and_then(|x| x.empty_as.map(|e| e.as_str()))
        .unwrap_or("empty");
    let strategy = EmptyStrategy::from_xml_config(strategy_str);

    let line_ending = LineEnding::from_config(
        export_cfg.and_then(|e| e.line_ending.map(|l| l.as_str()))
            .unwrap_or("lf")
    );
    let encoding = export_cfg.and_then(|e| e.encoding.map(|e| e.as_str()))
        .unwrap_or("utf-8").to_string();
    let opts = super::ExportOptions { line_ending, encoding };

    let sep = &project.config.separators;
    let output_dir = project.export_root().join(data_output).join("xml");
    let mut collected = Vec::new();

    for group in &project.groups {
        for table in &group.tables {
            if table.deleted { continue; }
            let xml = export_table_xml(table, &strategy, sep);
            let file_path = output_dir.join(format!("{}.xml", &table.name));
            collected.push((file_path, opts.encode(&xml)));
        }

        for constant in &group.constants {
            if constant.deleted { continue; }
            let xml = export_constant_xml(constant, &strategy, sep);
            let file_path = output_dir.join(format!("{}.xml", &constant.name));
            collected.push((file_path, opts.encode(&xml)));
        }
    }

    super::sync_export_dir(&output_dir, "xml", collected)
}

pub fn export_xml_filtered(project: &Project, filter: &super::DataFilter) -> Result<super::ExportResult> {
    let export_cfg = project.config.export.as_ref();

    let data_output = export_cfg
        .and_then(|e| e.server.as_ref())
        .and_then(|s| s.data_output.as_deref())
        .unwrap_or("gen/server/data");

    let strategy_str = export_cfg
        .and_then(|e| e.xml.as_ref())
        .and_then(|x| x.empty_as.map(|e| e.as_str()))
        .unwrap_or("empty");
    let strategy = EmptyStrategy::from_xml_config(strategy_str);

    let line_ending = LineEnding::from_config(
        export_cfg.and_then(|e| e.line_ending.map(|l| l.as_str()))
            .unwrap_or("lf")
    );
    let encoding = export_cfg.and_then(|e| e.encoding.map(|e| e.as_str()))
        .unwrap_or("utf-8").to_string();
    let opts = super::ExportOptions { line_ending, encoding };

    let sep = &project.config.separators;
    let output_dir = project.export_root().join(data_output).join("xml");
    let mut collected = Vec::new();

    for group in &project.groups {
        if !filter.matches_group(&group.name) { continue; }
        for table in &group.tables {
            if table.deleted { continue; }
            if !filter.matches_node(&table.name) { continue; }
            let xml = export_table_xml(table, &strategy, sep);
            let file_path = output_dir.join(format!("{}.xml", &table.name));
            collected.push((file_path, opts.encode(&xml)));
        }

        for constant in &group.constants {
            if constant.deleted { continue; }
            if !filter.matches_node(&constant.name) { continue; }
            let xml = export_constant_xml(constant, &strategy, sep);
            let file_path = output_dir.join(format!("{}.xml", &constant.name));
            collected.push((file_path, opts.encode(&xml)));
        }
    }

    super::sync_export_dir(&output_dir, "xml", collected)
}
