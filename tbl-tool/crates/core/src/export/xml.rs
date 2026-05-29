use std::fmt::Write;
use anyhow::Result;
use crate::model::*;
use crate::types::SeparatorsSection;
use super::{EmptyStrategy, LineEnding, to_camel_case};

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

fn sep_attrs(sep: &SeparatorsSection) -> String {
    let mut s = String::new();
    write!(s, " sep_list=\"{}\"", attr_escape(&sep.list)).unwrap();
    write!(s, " sep_set=\"{}\"", attr_escape(&sep.set)).unwrap();
    write!(s, " sep_tuple2=\"{}\"", attr_escape(&sep.tuple2)).unwrap();
    write!(s, " sep_tuple3=\"{}\"", attr_escape(&sep.tuple3)).unwrap();
    write!(s, " sep_tuple4=\"{}\"", attr_escape(&sep.tuple4)).unwrap();
    write!(s, " sep_map_kv=\"{}\"", attr_escape(&sep.map.kv)).unwrap();
    write!(s, " sep_map_entry=\"{}\"", attr_escape(&sep.map.entry)).unwrap();
    s
}

pub fn export_table_xml(table: &Table, strategy: &EmptyStrategy, sep: &SeparatorsSection) -> String {
    let fields = &table.schema.fields;
    let export_cols: Vec<(usize, &FieldDef)> = fields.iter().enumerate()
        .filter(|(_, f)| is_server_export(&f.export))
        .collect();

    let mut s = String::new();
    writeln!(s, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>").unwrap();
    writeln!(s, "<list{}>", sep_attrs(sep)).unwrap();

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
    let mut s = String::new();
    writeln!(s, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>").unwrap();
    writeln!(s, "<const{}>", sep_attrs(sep)).unwrap();

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
        .and_then(|x| x.empty_as.as_deref())
        .unwrap_or("empty");
    let strategy = EmptyStrategy::from_xml_config(strategy_str);

    let line_ending = LineEnding::from_config(
        export_cfg.and_then(|e| e.xml.as_ref()).and_then(|x| x.line_ending.as_deref())
            .or_else(|| export_cfg.and_then(|e| e.line_ending.as_deref()))
            .unwrap_or("lf")
    );
    let encoding = export_cfg.and_then(|e| e.xml.as_ref()).and_then(|x| x.encoding.as_deref())
        .or_else(|| export_cfg.and_then(|e| e.encoding.as_deref()))
        .unwrap_or("utf-8").to_string();
    let opts = super::ExportOptions { line_ending, encoding };

    let sep = &project.config.separators;
    let output_dir = project.workdir.join(data_output).join("xml");
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
