use std::fmt::Write;
use anyhow::Result;
use crate::model::*;
use crate::types::*;
use super::{to_camel_case, to_pascal_case, LineEnding};

const TPL_ITPL: &str = include_str!("../../templates/java/ITpl.java");
const TPL_ICONST: &str = include_str!("../../templates/java/IConstTpl.java");
const TPL_IJSON_PARSER: &str = include_str!("../../templates/java/IJsonParser.java");
const TPL_SIMPLE_JSON_PARSER: &str = include_str!("../../templates/java/SimpleJsonParser.java");
const TPL_UTIL: &str = include_str!("../../templates/java/TplUtil.java");
const TPL_HOLDER: &str = include_str!("../../templates/java/TplHolder.java");
const TPL_TUPLE2: &str = include_str!("../../templates/java/Tuple2.java");
const TPL_TUPLE3: &str = include_str!("../../templates/java/Tuple3.java");
const TPL_TUPLE4: &str = include_str!("../../templates/java/Tuple4.java");

fn is_server_export(export: &Export) -> bool {
    matches!(export, Export::ClientServer | Export::ServerOnly)
}

fn java_field_type(tbl_type_str: &str) -> String {
    TblType::parse(tbl_type_str)
        .map(|t| t.java_decl())
        .unwrap_or_else(|| "String".to_string())
}

fn needs_tuple_import(tbl_type_str: &str) -> bool {
    matches!(TblType::parse(tbl_type_str).map(|t| t.paradigm),
        Some(Paradigm::Tuple2) | Some(Paradigm::Tuple3) | Some(Paradigm::Tuple4) |
        Some(Paradigm::ListTuple2) | Some(Paradigm::ListTuple3) | Some(Paradigm::ListTuple4) |
        Some(Paradigm::MapTuple2) | Some(Paradigm::MapTuple3) | Some(Paradigm::MapTuple4))
}

// PLACEHOLDER_JAVA_RS_PART2

fn gen_table_tpl(table: &Table, pkg: &str, group: &str) -> String {
    let mut s = String::new();
    writeln!(s, "package {}.{};", pkg, group).unwrap();
    writeln!(s).unwrap();
    writeln!(s, "import {}.ITpl;", pkg).unwrap();
    writeln!(s, "import java.util.*;").unwrap();

    let fields: Vec<&FieldDef> = table.schema.fields.iter()
        .filter(|f| is_server_export(&f.export))
        .collect();

    if fields.iter().any(|f| needs_tuple_import(&f.tbl_type)) {
        writeln!(s, "import {}.types.*;", pkg).unwrap();
    }
    writeln!(s).unwrap();
    writeln!(s, "public class {}Tpl implements ITpl {{", table.name).unwrap();

    for f in &fields {
        let jtype = java_field_type(&f.tbl_type);
        let name = to_camel_case(&f.name);
        writeln!(s, "    private {} {};", jtype, name).unwrap();
    }
    writeln!(s).unwrap();

    let index_field = to_camel_case(&table.schema.index);
    writeln!(s, "    @Override").unwrap();
    writeln!(s, "    public int getId() {{ return {}; }}", index_field).unwrap();
    writeln!(s).unwrap();

    for f in &fields {
        if f.name == table.schema.index { continue; }
        let jtype = java_field_type(&f.tbl_type);
        let camel = to_camel_case(&f.name);
        let pascal = to_pascal_case(&f.name);
        writeln!(s, "    public {} get{}() {{ return {}; }}", jtype, pascal, camel).unwrap();
    }
    writeln!(s, "}}").unwrap();
    s
}

fn gen_constant_tpl(constant: &Constant, pkg: &str, group: &str) -> String {
    let mut s = String::new();
    writeln!(s, "package {}.{};", pkg, group).unwrap();
    writeln!(s).unwrap();
    writeln!(s, "import {}.IConstTpl;", pkg).unwrap();
    writeln!(s, "import java.util.*;").unwrap();

    let entries: Vec<&ConstEntry> = constant.entries.iter()
        .filter(|e| is_server_export(&e.export) && !e.name.is_empty())
        .collect();

    if entries.iter().any(|e| needs_tuple_import(&e.tbl_type)) {
        writeln!(s, "import {}.types.*;", pkg).unwrap();
    }
    writeln!(s).unwrap();
    writeln!(s, "public class {}Tpl implements IConstTpl {{", constant.name).unwrap();

    for e in &entries {
        let jtype = java_field_type(&e.tbl_type);
        let name = to_camel_case(&e.name);
        writeln!(s, "    private {} {};", jtype, name).unwrap();
    }
    writeln!(s).unwrap();

    for e in &entries {
        let jtype = java_field_type(&e.tbl_type);
        let camel = to_camel_case(&e.name);
        let pascal = to_pascal_case(&e.name);
        writeln!(s, "    public {} get{}() {{ return {}; }}", jtype, pascal, camel).unwrap();
    }
    writeln!(s, "}}").unwrap();
    s
}

// PLACEHOLDER_JAVA_RS_PART3

pub fn export_all_java(project: &Project) -> Result<Vec<String>> {
    let export_cfg = project.config.export.as_ref();
    let server = export_cfg.and_then(|e| e.server.as_ref());

    let code_output = server
        .and_then(|s| s.code_output.as_deref())
        .unwrap_or("gen/server/code");
    let pkg = server
        .and_then(|s| s.package.as_deref())
        .unwrap_or("com.game.config");

    let line_ending = LineEnding::from_config(
        export_cfg.and_then(|e| e.line_ending.as_deref()).unwrap_or("lf")
    );

    let pkg_path = pkg.replace('.', "/");
    let output_dir = project.workdir.join(code_output).join(&pkg_path);
    let types_dir = output_dir.join("types");
    std::fs::create_dir_all(&types_dir)?;

    let mut generated = Vec::new();

    let write_file = |dir: &std::path::Path, name: &str, content: &str, gen: &mut Vec<String>| -> Result<()> {
        let path = dir.join(name);
        let normalized = line_ending.normalize(content);
        std::fs::write(&path, normalized.as_bytes())?;
        gen.push(path.display().to_string());
        Ok(())
    };

    let render = |tpl: &str| tpl.replace("{{PACKAGE}}", pkg);

    write_file(&output_dir, "ITpl.java", &render(TPL_ITPL), &mut generated)?;
    write_file(&output_dir, "IConstTpl.java", &render(TPL_ICONST), &mut generated)?;
    write_file(&output_dir, "IJsonParser.java", &render(TPL_IJSON_PARSER), &mut generated)?;
    write_file(&output_dir, "SimpleJsonParser.java", &render(TPL_SIMPLE_JSON_PARSER), &mut generated)?;
    write_file(&output_dir, "TplUtil.java", &render(TPL_UTIL), &mut generated)?;
    write_file(&types_dir, "Tuple2.java", &render(TPL_TUPLE2), &mut generated)?;
    write_file(&types_dir, "Tuple3.java", &render(TPL_TUPLE3), &mut generated)?;
    write_file(&types_dir, "Tuple4.java", &render(TPL_TUPLE4), &mut generated)?;

    let mut register_lines = String::new();
    for group in &project.groups {
        let group_dir = output_dir.join(&group.name);
        std::fs::create_dir_all(&group_dir)?;

        for table in &group.tables {
            if table.deleted { continue; }
            let content = gen_table_tpl(table, pkg, &group.name);
            let filename = format!("{}Tpl.java", &table.name);
            write_file(&group_dir, &filename, &content, &mut generated)?;
            writeln!(register_lines, "        registry.put(\"{}/{}.json\", {}.{}.{}Tpl.class);",
                group.name, table.name, pkg, group.name, table.name).unwrap();
        }

        for constant in &group.constants {
            if constant.deleted { continue; }
            let content = gen_constant_tpl(constant, pkg, &group.name);
            let filename = format!("{}Tpl.java", &constant.name);
            write_file(&group_dir, &filename, &content, &mut generated)?;
            writeln!(register_lines, "        registry.put(\"{}/{}.json\", {}.{}.{}Tpl.class);",
                group.name, constant.name, pkg, group.name, constant.name).unwrap();
        }
    }

    let holder_content = render(TPL_HOLDER).replace("{{REGISTER_LIST}}", register_lines.trim_end());
    write_file(&output_dir, "TplHolder.java", &holder_content, &mut generated)?;

    Ok(generated)
}
