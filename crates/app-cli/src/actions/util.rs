//! util 子命令：无项目上下文的文件工具。

use std::path::Path;

use anyhow::Result;
use tablet_core::tbl::{parse_tbl, TblFile};
use tablet_core::tblschema::{parse_tblschema, merge_schemas, serialize_tblschema};
use tablet_core::types::{SepKey, SeparatorsSection};

pub fn run_parse_tbl(path: &Path) -> Result<String> {
    let tbl = parse_tbl(path)?;
    Ok(format_tbl_file(&tbl))
}

pub fn run_parse_schema(path: &Path) -> Result<String> {
    let text = std::fs::read_to_string(path)?;
    let schema = parse_tblschema(&text)?;
    Ok(format_schema(&schema))
}

pub fn run_merge_schema(paths: &[std::path::PathBuf]) -> Result<String> {
    let mut schemas = Vec::new();
    for p in paths {
        let text = std::fs::read_to_string(p)?;
        schemas.push(parse_tblschema(&text)?);
    }
    let merged = merge_schemas(&schemas)?;
    Ok(serialize_tblschema(&merged))
}

fn format_tbl_file(tbl: &TblFile) -> String {
    match tbl {
        TblFile::Table(t) => {
            let mut out = String::new();
            out.push_str(&format!("{{\"type\":\"table\",\"name\":\"{}\",\"fields\":[", t.name));
            for (i, f) in t.schema.fields.iter().enumerate() {
                if i > 0 { out.push(','); }
                out.push_str(&format!(
                    "{{\"name\":\"{}\",\"type\":\"{}\",\"export\":\"{}\",\"desc\":\"{}\"}}",
                    escape_json(&f.name), escape_json(&f.tbl_type),
                    f.export.display(), escape_json(&f.desc)
                ));
            }
            out.push_str(&format!("],\"records_count\":{}}}", t.records.len()));
            out
        }
        TblFile::Constant(c) => {
            let mut out = String::new();
            out.push_str(&format!("{{\"type\":\"constant\",\"name\":\"{}\",\"entries\":[", c.name));
            for (i, e) in c.entries.iter().enumerate() {
                if i > 0 { out.push(','); }
                out.push_str(&format!(
                    "{{\"name\":\"{}\",\"type\":\"{}\",\"value\":\"{}\",\"export\":\"{}\",\"desc\":\"{}\"}}",
                    escape_json(&e.name), escape_json(&e.tbl_type),
                    escape_json(&e.value), e.export.display(), escape_json(&e.desc)
                ));
            }
            out.push_str("]}");
            out
        }
        TblFile::Enum(e) => {
            let mut out = String::new();
            out.push_str(&format!("{{\"type\":\"enum\",\"name\":\"{}\",\"entries\":[", e.name));
            for (i, entry) in e.entries.iter().enumerate() {
                if i > 0 { out.push(','); }
                out.push_str(&format!(
                    "{{\"id\":\"{}\",\"name\":\"{}\",\"desc\":\"{}\"}}",
                    escape_json(&entry.id), escape_json(&entry.name), escape_json(&entry.desc)
                ));
            }
            out.push_str("]}");
            out
        }
    }
}

fn format_schema(schema: &tablet_core::tblschema::TblSchema) -> String {
    use tablet_core::tblschema::SchemaMode;
    let mut out = String::new();
    out.push_str(&format!("{{\"meta\":{{\"id\":\"{}\",\"name\":\"{}\"}},\"sections\":[",
        escape_json(&schema.meta.id), escape_json(&schema.meta.name)));
    for (i, s) in schema.sections.iter().enumerate() {
        if i > 0 { out.push(','); }
        let kind = match s.mode {
            SchemaMode::Table => "table",
            SchemaMode::Constant => "constant",
            SchemaMode::Enum => "enum",
        };
        out.push_str(&format!(
            "{{\"group\":\"{}\",\"name\":\"{}\",\"kind\":\"{}\"}}",
            escape_json(&s.group), escape_json(&s.name), kind
        ));
    }
    out.push_str("]}");
    out
}

fn escape_json(s: &str) -> String {
    s.replace('\\', "\\\\")
     .replace('"', "\\\"")
     .replace('\n', "\\n")
     .replace('\r', "\\r")
     .replace('\t', "\\t")
}

// ─── validate ──────────────────────────────────────────────────────────────

pub fn run_validate_tbl(
    path: &Path,
    config_path: Option<&Path>,
    schema_path: Option<&Path>,
    sep_overrides: &[String],
) -> Result<Vec<String>> {
    let sep = build_separators(config_path, schema_path, sep_overrides)?;
    let tbl = parse_tbl(path)?;
    let errors = match &tbl {
        TblFile::Table(t) => {
            tablet_core::validate::validate_table(t, &sep, None)
        }
        TblFile::Constant(c) => {
            tablet_core::validate::validate_constant(c, &sep, false, None)
        }
        TblFile::Enum(e) => {
            tablet_core::validate::validate_enum(e)
        }
    };
    let msgs: Vec<String> = errors.iter().map(|e| e.message.clone()).collect();
    Ok(msgs)
}

pub fn run_validate_type(
    type_expr: &str,
    value: &str,
    config_path: Option<&Path>,
    schema_path: Option<&Path>,
    sep_overrides: &[String],
) -> Result<Option<String>> {
    let sep = build_separators(config_path, schema_path, sep_overrides)?;
    let parsed = tablet_core::types::TblType::parse(type_expr)
        .ok_or_else(|| anyhow::anyhow!("无法解析类型: {}", type_expr))?;
    Ok(parsed.validate_value(value, &sep))
}

fn build_separators(
    config_path: Option<&Path>,
    schema_path: Option<&Path>,
    sep_overrides: &[String],
) -> Result<SeparatorsSection> {
    let mut sep = SeparatorsSection::default();

    if let Some(path) = config_path {
        let text = std::fs::read_to_string(path)?;
        #[derive(serde::Deserialize)]
        struct Partial {
            #[serde(default)]
            separators: SeparatorsSection,
        }
        let parsed: Partial = toml::from_str(&text)?;
        sep = parsed.separators;
    }

    if let Some(path) = schema_path {
        let text = std::fs::read_to_string(path)?;
        let schema = parse_tblschema(&text)?;
        sep = schema.separators;
    }

    for kv in sep_overrides {
        if let Some((key, val)) = kv.split_once('=') {
            if let Some(sk) = SepKey::from_directive_key(key.trim()) {
                sk.set(&mut sep, val.trim().to_string());
            } else {
                anyhow::bail!("未知的分隔符 key: {}", key.trim());
            }
        } else {
            anyhow::bail!("分隔符格式错误，应为 KEY=VALUE: {}", kv);
        }
    }

    Ok(sep)
}

// ─── convert ───────────────────────────────────────────────────────────────

pub fn run_tbl_to_xlsx(tbl_path: &Path, output: &Path) -> Result<()> {
    let tbl = parse_tbl(tbl_path)?;
    let bytes = match &tbl {
        TblFile::Table(t) => tablet_core::excel::export_table_book(t)?,
        TblFile::Constant(c) => tablet_core::excel::export_constant_book(c)?,
        TblFile::Enum(e) => tablet_core::excel::export_enum_book(e)?,
    };
    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(output, &bytes)?;
    Ok(())
}

pub fn run_xlsx_to_tbl(_xlsx_path: &Path, _schema_path: &Path, _output_dir: &Path) -> Result<usize> {
    // xlsx-to-tbl 需要完整的 Group 结构做 header 校验，
    // 独立模式下需要从 schema 构建空骨架 Group，暂未实现。
    anyhow::bail!("xlsx-to-tbl 尚未实现（需要从 schema 构建空骨架 Group）")
}

pub fn run_scaffold(schema_path: &Path, output_dir: &Path) -> Result<()> {
    let text = std::fs::read_to_string(schema_path)?;
    let schema = parse_tblschema(&text)?;
    tablet_core::template::instantiate_template(&schema, output_dir)?;
    Ok(())
}

pub fn run_gen_test(
    lang: &str,
    format: &str,
    schema_path: &Path,
    output_dir: &Path,
    package: &str,
    code_output: &str,
) -> Result<()> {
    let text = std::fs::read_to_string(schema_path)?;
    let schema = parse_tblschema(&text)?;
    std::fs::create_dir_all(output_dir)?;

    match lang {
        "java" => {
            tablet_core::test_util::generate_test_main_from_schema(output_dir, &schema, package, format);
        }
        "go" => {
            tablet_core::test_util::generate_test_main_go_from_schema(output_dir, &schema, package, code_output, format);
        }
        other => {
            anyhow::bail!("不支持的测试语言: {}（支持 java / go）", other);
        }
    }
    Ok(())
}

// ─── auxiliary ─────────────────────────────────────────────────────────────

pub fn run_diff(a_path: &Path, b_path: &Path) -> Result<String> {
    let a = parse_tbl(a_path)?;
    let b = parse_tbl(b_path)?;
    Ok(format_diff(&a, &b))
}

pub fn run_fmt(path: &Path, in_place: bool) -> Result<Option<String>> {
    if path.is_dir() {
        let entries: Vec<_> = std::fs::read_dir(path)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|x| x == "tbl").unwrap_or(false))
            .collect();
        for entry in entries {
            fmt_single_file(&entry.path(), true)?;
        }
        return Ok(None);
    }
    fmt_single_file(path, in_place)
}

fn fmt_single_file(path: &Path, in_place: bool) -> Result<Option<String>> {
    use tablet_core::tbl::{serialize_table, serialize_constant, serialize_enum};
    let tbl = parse_tbl(path)?;
    let output = match &tbl {
        TblFile::Table(t) => serialize_table(t),
        TblFile::Constant(c) => serialize_constant(c),
        TblFile::Enum(e) => serialize_enum(e),
    };
    if in_place {
        std::fs::write(path, &output)?;
        Ok(None)
    } else {
        Ok(Some(output))
    }
}

pub fn run_stat(path: &Path) -> Result<String> {
    if path.is_dir() {
        return stat_dir(path);
    }
    stat_file(path)
}

fn stat_file(path: &Path) -> Result<String> {
    let tbl = parse_tbl(path)?;
    let mut out = String::new();
    out.push_str(&format!("文件: {}\n", path.display()));
    match &tbl {
        TblFile::Table(t) => {
            let fields = &t.schema.fields;
            out.push_str("类型: table\n");
            out.push_str(&format!("字段: {}\n", fields.len()));
            out.push_str(&format!("数据行: {}\n", t.records.len()));
            let total_cells = t.records.len() * fields.len();
            let empty_cells = t.records.iter()
                .flat_map(|r| r.iter())
                .filter(|c| c.is_empty())
                .count();
            if total_cells > 0 {
                out.push_str(&format!("空值: {} ({:.1}%)\n", empty_cells, empty_cells as f64 / total_cells as f64 * 100.0));
            }
        }
        TblFile::Constant(c) => {
            out.push_str("类型: constant\n");
            out.push_str(&format!("条目: {}\n", c.entries.len()));
        }
        TblFile::Enum(e) => {
            out.push_str("类型: enum\n");
            out.push_str(&format!("条目: {}\n", e.entries.len()));
        }
    }
    Ok(out)
}

fn stat_dir(dir: &Path) -> Result<String> {
    let mut tables = 0usize;
    let mut constants = 0usize;
    let mut enums = 0usize;
    let mut total_rows = 0usize;
    let mut total_files = 0usize;

    visit_tbl_files(dir, &mut |path| {
        if let Ok(tbl) = parse_tbl(path) {
            total_files += 1;
            match &tbl {
                TblFile::Table(t) => { tables += 1; total_rows += t.records.len(); }
                TblFile::Constant(c) => { constants += 1; total_rows += c.entries.len(); }
                TblFile::Enum(e) => { enums += 1; total_rows += e.entries.len(); }
            }
        }
    })?;

    let mut out = String::new();
    out.push_str(&format!("目录: {}\n", dir.display()));
    out.push_str(&format!("文件: {} (table×{}, constant×{}, enum×{})\n", total_files, tables, constants, enums));
    out.push_str(&format!("总数据行: {}\n", total_rows));
    Ok(out)
}

fn visit_tbl_files(dir: &Path, f: &mut dyn FnMut(&Path)) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            visit_tbl_files(&path, f)?;
        } else if path.extension().map(|x| x == "tbl").unwrap_or(false) {
            f(&path);
        }
    }
    Ok(())
}

fn format_diff(a: &TblFile, b: &TblFile) -> String {
    let mut out = String::new();
    match (a, b) {
        (TblFile::Table(ta), TblFile::Table(tb)) => {
            // Structure diff
            let fa = &ta.schema.fields;
            let fb = &tb.schema.fields;
            if fa.len() != fb.len() {
                out.push_str(&format!("结构差异: 列数 {} → {}\n", fa.len(), fb.len()));
            }
            for (i, (a_f, b_f)) in fa.iter().zip(fb.iter()).enumerate() {
                if a_f.name != b_f.name || a_f.tbl_type != b_f.tbl_type {
                    out.push_str(&format!("  ~ col {}: {} {} → {} {}\n",
                        i, a_f.tbl_type, a_f.name, b_f.tbl_type, b_f.name));
                }
            }
            // Data diff
            let max_rows = ta.records.len().max(tb.records.len());
            let mut diff_count = 0;
            for i in 0..max_rows {
                let ra = ta.records.get(i);
                let rb = tb.records.get(i);
                if ra != rb {
                    diff_count += 1;
                    if diff_count <= 20 {
                        match (ra, rb) {
                            (None, Some(_)) => out.push_str(&format!("  行 {}: (新增)\n", i + 1)),
                            (Some(_), None) => out.push_str(&format!("  行 {}: (删除)\n", i + 1)),
                            (Some(_), Some(_)) => out.push_str(&format!("  行 {}: (修改)\n", i + 1)),
                            _ => {}
                        }
                    }
                }
            }
            if diff_count > 20 {
                out.push_str(&format!("  ...及另外 {} 行差异\n", diff_count - 20));
            }
            if diff_count == 0 && out.is_empty() {
                out.push_str("无差异\n");
            } else if diff_count > 0 {
                out.push_str(&format!("\n数据差异: {} 行\n", diff_count));
            }
        }
        _ => {
            out.push_str("类型不同或不支持的对比组合\n");
        }
    }
    out
}
