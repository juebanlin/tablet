use anyhow::{Result, bail};

#[derive(Debug, Clone)]
pub struct TblSchema {
    pub sections: Vec<SchemaSection>,
}

#[derive(Debug, Clone)]
pub struct SchemaSection {
    pub group: String,
    pub name: String,
    pub mode: SchemaMode,
    pub fields: Vec<SchemaField>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SchemaMode {
    Table,
    Constant,
}

#[derive(Debug, Clone)]
pub struct SchemaField {
    pub name: String,
    pub tbl_type: String,
    pub export: String,
    pub desc: String,
}

impl SchemaField {
    pub fn export_display(&self) -> &str {
        match self.export.as_str() {
            "cs" | "" => "前后端",
            "c" => "客户端",
            "s" => "服务器",
            "-" => "不导出",
            _ => "前后端",
        }
    }

    pub fn is_server_export(&self) -> bool {
        matches!(self.export.as_str(), "cs" | "" | "s")
    }
}

pub fn parse_tblschema(content: &str) -> Result<TblSchema> {
    let mut sections = Vec::new();
    let mut current: Option<SchemaSection> = None;

    for (line_num, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with('[') {
            if let Some(sec) = current.take() {
                validate_section(&sec, line_num)?;
                sections.push(sec);
            }
            current = Some(parse_section_header(line, line_num)?);
        } else if let Some(ref mut sec) = current {
            let field = parse_field_line(line, line_num)?;
            sec.fields.push(field);
        } else {
            bail!("line {}: field outside section", line_num + 1);
        }
    }

    if let Some(sec) = current {
        validate_section(&sec, content.lines().count())?;
        sections.push(sec);
    }

    Ok(TblSchema { sections })
}

fn parse_section_header(line: &str, line_num: usize) -> Result<SchemaSection> {
    let end_bracket = line.find(']').unwrap_or(0);
    let path = &line[1..end_bracket];
    let rest = line[end_bracket + 1..].trim();

    let (group, name) = path.split_once('/')
        .ok_or_else(|| anyhow::anyhow!("line {}: section must be [group/Name]", line_num + 1))?;

    let parts: Vec<&str> = rest.split_whitespace().collect();
    let mode_str = parts.first().copied().unwrap_or("");
    let mode = match mode_str {
        "table" => SchemaMode::Table,
        "constant" => SchemaMode::Constant,
        _ => bail!("line {}: mode must be 'table' or 'constant'", line_num + 1),
    };

    // ignore index= option (backward compat, index is always "id")
    Ok(SchemaSection {
        group: group.trim().to_string(),
        name: name.trim().to_string(),
        mode,
        fields: Vec::new(),
    })
}

fn parse_field_line(line: &str, line_num: usize) -> Result<SchemaField> {
    let parts: Vec<&str> = line.split('|').collect();
    if parts.len() < 3 {
        bail!("line {}: field needs at least name|type|export", line_num + 1);
    }

    Ok(SchemaField {
        name: parts[0].trim().to_string(),
        tbl_type: parts[1].trim().to_string(),
        export: parts[2].trim().to_string(),
        desc: parts.get(3).map(|s| s.trim().to_string()).unwrap_or_default(),
    })
}

fn validate_section(sec: &SchemaSection, _line_num: usize) -> Result<()> {
    let mut names = std::collections::HashSet::new();
    for f in &sec.fields {
        if !names.insert(&f.name) {
            bail!("[{}/{}]: duplicate field '{}'", sec.group, sec.name, f.name);
        }
    }
    Ok(())
}

pub fn merge_schemas(schemas: &[TblSchema]) -> Result<TblSchema> {
    let mut all_sections = Vec::new();
    let mut keys = std::collections::HashSet::new();

    for schema in schemas {
        for sec in &schema.sections {
            let key = format!("{}/{}", sec.group, sec.name);
            if !keys.insert(key.clone()) {
                bail!("duplicate section: [{}]", key);
            }
            all_sections.push(sec.clone());
        }
    }

    Ok(TblSchema { sections: all_sections })
}
