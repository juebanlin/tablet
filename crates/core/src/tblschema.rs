use std::path::Path;
use anyhow::{Result, bail};
use crate::model::*;
use crate::tbl_str;
use crate::types::SeparatorsSection;

#[derive(Debug, Clone, Default)]
pub struct SchemaMetadata {
    pub id: String,         // 路径标识：[a-z0-9_-]{1,32}；缺省时由调用方按文件名兜底
    pub name: String,       // 显示文本（含中文）；缺省时 = id
    pub category: String,   // 分类筛选（test / slg / rpg / ...）
    pub version: String,    // semver
    /// 项目身份：创建时间。模板侧通常为空。
    pub created_at: String,
    /// 项目身份：来源模板 id。手动新建 / 模板自身为空。
    pub source_template: String,
    /// 项目身份：来源模板版本。
    pub source_template_version: String,
    /// derive 字段：任一 section.preset 非空 → true。serialize 前由调用方刷新；
    /// 反序列化阶段跟随 # @meta has_preset 行（缺省 false）。
    pub has_preset: bool,
}

#[derive(Debug, Clone, Default)]
pub struct TblSchema {
    pub meta: SchemaMetadata,
    /// 分隔符配置（25 个叶子键，对齐 `SeparatorsSection`）。
    /// 缺省 = `SeparatorsSection::default()`；schema 中 `# @sep` 行覆写。
    /// 加载链路把它复制到 `ProjectConfig.separators`（`merge_config` 时以 schema 为准），
    /// 作为运行时唯一来源。
    pub separators: SeparatorsSection,
    pub sections: Vec<SchemaSection>,
}

#[derive(Debug, Clone, Default)]
pub struct SchemaSection {
    pub group: String,
    pub name: String,
    pub mode: SchemaMode,
    /// Table 段：列声明；Constant / Enum 段：永远空（entries 在项目 .tbl 里，preset 例外）。
    pub fields: Vec<SchemaField>,
    /// 可选预设数据（与 .tbl `---` 之后行同形态）：
    /// - Table：每行 = 一条 record（按列序）
    /// - Constant：每行 = `name | type | value | export | desc`
    /// - Enum：每行 = `id | name | desc`
    /// 仅在「带预设」的 schema 文件 / 导出选项下出现；apply_schema_to_project 按 with_preset 决定是否灌入项目。
    pub preset: Vec<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum SchemaMode {
    #[default]
    Table,
    Constant,
    Enum,
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

/// 校验 metadata id 是否合法 ([a-z0-9_-]{1,32})。
pub fn is_valid_metadata_id(id: &str) -> bool {
    if id.is_empty() || id.len() > 32 {
        return false;
    }
    id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

pub fn parse_tblschema(content: &str) -> Result<TblSchema> {
    let mut sections = Vec::new();
    let mut current: Option<SchemaSection> = None;
    let mut meta = SchemaMetadata::default();
    let mut separators = SeparatorsSection::default();
    let mut seen_section = false;
    let mut in_preset = false;

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if trimmed.starts_with('#') {
            // `# @preset` 进入预设数据模式；之后的非 `#` / 非 `[` 行按 `|` 切作为预设行。
            // 仅识别独立 `# @preset`（后接空白或行尾），其它带后缀写法当注释。
            if let Some(rest) = trimmed.trim_start_matches('#').trim_start().strip_prefix("@preset") {
                if rest.trim().is_empty() {
                    if current.is_none() {
                        bail!("line {}: `# @preset` 必须出现在某个 [section] 之后", line_num + 1);
                    }
                    in_preset = true;
                    continue;
                }
            }
            // 仅在第一个 [group/Name] 之前的 `# @meta key: value` / `# @sep key = value` 行视作 directive；
            // 之后的 `#` 行一律视作普通注释。
            if !seen_section {
                if let Some((key, value)) = parse_meta_line(trimmed) {
                    match key.as_str() {
                        "id" => meta.id = value,
                        "name" => meta.name = value,
                        "category" => meta.category = value,
                        "version" => meta.version = value,
                        "created_at" => meta.created_at = value,
                        "source_template" => meta.source_template = value,
                        "source_template_version" => meta.source_template_version = value,
                        "has_preset" => meta.has_preset = value.eq_ignore_ascii_case("true"),
                        _ => { /* 未知 key：忽略，前向兼容 */ }
                    }
                } else if let Some((key, value)) = parse_sep_line(trimmed) {
                    apply_sep_kv(&mut separators, &key, &value);
                }
            }
            continue;
        }

        if trimmed.starts_with('[') {
            seen_section = true;
            in_preset = false;
            if let Some(sec) = current.take() {
                validate_section(&sec, line_num)?;
                sections.push(sec);
            }
            current = Some(parse_section_header(trimmed, line_num)?);
        } else if let Some(ref mut sec) = current {
            if in_preset {
                // preset 行：按 `|` 切并 decode，trim 单元格。空行已在外层过滤。
                // 走 tbl_str：让用户在 schema 里用 `\|` 表示字面竖线、`\n` 表示换行等。
                // 这里统一按 Str 类型 decode（Atom 类型的值不含转义字符，decode 是零成本 fast-path）。
                let cells: Vec<String> = tbl_str::split_row(trimmed)
                    .into_iter()
                    .map(|s| tbl_str::decode(s.trim(), tbl_str::FieldKind::Str))
                    .collect();
                sec.preset.push(cells);
            } else {
                match sec.mode {
                    SchemaMode::Table => {
                        let field = parse_field_line(trimmed, line_num, &sec.mode)?;
                        sec.fields.push(field);
                    }
                    SchemaMode::Constant | SchemaMode::Enum => {
                        let mode_name = if matches!(sec.mode, SchemaMode::Constant) { "constant" } else { "enum" };
                        bail!(
                            "line {}: [{}/{}] {} 段不允许直接的数据行（如需预设值请放进 `# @preset` 块）",
                            line_num + 1, sec.group, sec.name, mode_name
                        );
                    }
                }
            }
        } else {
            bail!("line {}: field outside section", line_num + 1);
        }
    }

    if let Some(sec) = current {
        validate_section(&sec, content.lines().count())?;
        sections.push(sec);
    }

    // name 缺省 = id；id 缺省由调用方按文件名 stem 兜底（这里不强制）
    if meta.name.is_empty() {
        meta.name = meta.id.clone();
    }

    // has_preset 永远从 sections 重算，忽略 @meta 行声明
    meta.has_preset = sections.iter().any(|s| !s.preset.is_empty());

    Ok(TblSchema { meta, separators, sections })
}

/// 解析 `# @meta key: value` 行；返回 (key, value)。
/// 接受形式：
///   `# @meta id: full`
///   `#@meta name: 完整测试模板`
/// key 大小写敏感；后者覆盖前者由调用方处理。
fn parse_meta_line(line: &str) -> Option<(String, String)> {
    // 去掉前导 '#'，再去空白
    let body = line.trim_start_matches('#').trim();
    let after = body.strip_prefix("@meta")?.trim_start();
    let (key, value) = after.split_once(':')?;
    Some((key.trim().to_string(), value.trim().to_string()))
}

/// 解析 `# @sep key = value` 行；返回 (key, value)。
/// 接受 `# @sep List = ;` / `# @sep Map.entry = ,` / `# @sep Map_Tuple2.kv = :` 形式。
/// 与 `parse_meta_line` 区别：分隔符值通过 `=` 切（避免与 `Map.kv` 中 ':' 冲突），
/// 等号两侧 trim，但**值本身不再 trim 内部空格**——保留原样以便支持空白类分隔符。
fn parse_sep_line(line: &str) -> Option<(String, String)> {
    let body = line.trim_start_matches('#').trim();
    let after = body.strip_prefix("@sep")?.trim_start();
    let (key, value) = after.split_once('=')?;
    Some((key.trim().to_string(), value.trim_start().trim_end_matches('\n').to_string()))
}

/// 把单个 `# @sep` 行写入 `SeparatorsSection`。未知 key 忽略（向前兼容）。
fn apply_sep_kv(sep: &mut SeparatorsSection, key: &str, value: &str) {
    if let Some(k) = crate::types::SepKey::from_directive_key(key) {
        k.set(sep, value.to_string());
    }
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
        "enum" => SchemaMode::Enum,
        _ => bail!("line {}: mode must be 'table' / 'constant' / 'enum'", line_num + 1),
    };

    // ignore index= option (backward compat, index is always "id")
    Ok(SchemaSection {
        group: group.trim().to_string(),
        name: name.trim().to_string(),
        mode,
        fields: Vec::new(),
        preset: Vec::new(),
    })
}

fn parse_field_line(line: &str, line_num: usize, mode: &SchemaMode) -> Result<SchemaField> {
    let parts: Vec<&str> = line.split('|').collect();
    match mode {
        SchemaMode::Enum => {
            // enum 数据行：id | name | desc
            if parts.len() < 2 {
                bail!("line {}: enum row needs at least id|name", line_num + 1);
            }
            Ok(SchemaField {
                name: parts[1].trim().to_string(),
                tbl_type: parts[0].trim().to_string(), // 借用 tbl_type 字段存储 id
                export: String::new(),
                desc: parts.get(2).map(|s| s.trim().to_string()).unwrap_or_default(),
            })
        }
        _ => {
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
    }
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

    // 多文件合并不挂任何 metadata（合并产物没有单一来源 id）。调用方如有需要自行赋 meta。
    Ok(TblSchema { meta: SchemaMetadata::default(), separators: SeparatorsSection::default(), sections: all_sections })
}

pub fn serialize_tblschema(schema: &TblSchema) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    writeln!(s, "#!tblschema v1").unwrap();

    // metadata：仅非空字段输出
    if !schema.meta.id.is_empty() {
        writeln!(s, "# @meta id: {}", schema.meta.id).unwrap();
    }
    if !schema.meta.name.is_empty() && schema.meta.name != schema.meta.id {
        writeln!(s, "# @meta name: {}", schema.meta.name).unwrap();
    }
    if !schema.meta.category.is_empty() {
        writeln!(s, "# @meta category: {}", schema.meta.category).unwrap();
    }
    if !schema.meta.version.is_empty() {
        writeln!(s, "# @meta version: {}", schema.meta.version).unwrap();
    }
    if !schema.meta.created_at.is_empty() {
        writeln!(s, "# @meta created_at: {}", schema.meta.created_at).unwrap();
    }
    if !schema.meta.source_template.is_empty() {
        writeln!(s, "# @meta source_template: {}", schema.meta.source_template).unwrap();
    }
    if !schema.meta.source_template_version.is_empty() {
        writeln!(s, "# @meta source_template_version: {}", schema.meta.source_template_version).unwrap();
    }
    // has_preset 是 derive 字段：以实际 sections 为准
    let has_preset = schema.sections.iter().any(|s| !s.preset.is_empty());
    if has_preset {
        writeln!(s, "# @meta has_preset: true").unwrap();
    }

    // separators：仅输出与默认不同的字段，避免普通 schema 满屏 25 行
    write_sep_diff(&mut s, &schema.separators);

    for sec in &schema.sections {
        writeln!(s).unwrap();
        let mode = match sec.mode {
            SchemaMode::Table => "table",
            SchemaMode::Constant => "constant",
            SchemaMode::Enum => "enum",
        };
        writeln!(s, "[{}/{}] {}", sec.group, sec.name, mode).unwrap();

        // 仅 Table 段输出列声明；Constant / Enum 段是「结构在项目 .tbl 里」，schema 只留段头。
        if matches!(sec.mode, SchemaMode::Table) {
            for f in &sec.fields {
                writeln!(s, "{} | {} | {} | {}", f.name, f.tbl_type, f.export, f.desc).unwrap();
            }
        }

        // 预设数据 sub-block
        if !sec.preset.is_empty() {
            writeln!(s, "# @preset").unwrap();
            for row in &sec.preset {
                writeln!(s, "{}", row.join(" | ")).unwrap();
            }
        }
    }
    s
}

/// 把 SeparatorsSection 中与默认不同的字段写成 `# @sep` 行。全部默认 = 不输出。
fn write_sep_diff(s: &mut String, sep: &SeparatorsSection) {
    use std::fmt::Write;
    use crate::types::SepKey;
    let d = SeparatorsSection::default();
    for k in SepKey::ALL {
        let v = k.get(sep);
        if v != k.get(&d) {
            writeln!(s, "# @sep {} = {}", k.as_directive_key(), v).unwrap();
        }
    }
}

/// 把 project 反向编码成 TblSchema：
/// - Table 段：列定义照写；with_preset=true 时把 records 作为 preset 行
/// - Constant 段：fields 永远空（新范式）；with_preset=true 时 entries → preset
/// - Enum 段：同 Constant
///
/// meta 留空（调用方按导出场景填）。
pub fn schema_from_project(groups: &[Group], with_preset: bool) -> TblSchema {
    let mut sections = Vec::new();
    for group in groups {
        for table in &group.tables {
            if table.deleted { continue; }
            let fields = table.schema.fields.iter().map(|f| SchemaField {
                name: f.name.clone(),
                tbl_type: f.tbl_type.clone(),
                export: export_to_code(&f.export),
                desc: f.desc.clone(),
            }).collect();
            let preset = if with_preset {
                table.records.iter().map(|r| r.clone()).collect()
            } else {
                Vec::new()
            };
            sections.push(SchemaSection {
                group: group.name.clone(),
                name: table.name.clone(),
                mode: SchemaMode::Table,
                fields,
                preset,
            });
        }
        for constant in &group.constants {
            if constant.deleted { continue; }
            // 新范式：constant section 的 schema fields 永远空（结构信息全在项目 .tbl）
            let preset = if with_preset {
                constant.entries.iter()
                    .filter(|e| !e.name.is_empty())
                    .map(|e| vec![
                        e.name.clone(),
                        e.tbl_type.clone(),
                        e.value.clone(),
                        export_to_code(&e.export),
                        e.desc.clone(),
                    ])
                    .collect()
            } else {
                Vec::new()
            };
            sections.push(SchemaSection {
                group: group.name.clone(),
                name: constant.name.clone(),
                mode: SchemaMode::Constant,
                fields: Vec::new(),
                preset,
            });
        }
        for enum_def in &group.enums {
            if enum_def.deleted { continue; }
            let preset = if with_preset {
                enum_def.entries.iter()
                    .filter(|e| !e.id.is_empty() || !e.name.is_empty())
                    .map(|e| vec![e.id.clone(), e.name.clone(), e.desc.clone()])
                    .collect()
            } else {
                Vec::new()
            };
            sections.push(SchemaSection {
                group: group.name.clone(),
                name: enum_def.name.clone(),
                mode: SchemaMode::Enum,
                fields: Vec::new(),
                preset,
            });
        }
    }
    TblSchema { meta: SchemaMetadata::default(), separators: SeparatorsSection::default(), sections }
}

fn export_to_code(e: &Export) -> String {
    e.code().to_string()
}

/// 把 schema sections 应用到 project：
/// - Table 段写入 / 替换列定义；with_preset=true 时把 preset 行作为 records 灌入
/// - Constant / Enum 段：新范式下 schema 段头里不再带 entries，因此行数据来自 preset
///     - with_preset=true：preset 行 → entries
///     - with_preset=false：entries 留空（用户在 UI 里手动添加）
///
/// 返回 (added_nodes, overwritten_nodes)。
pub fn apply_schema_to_project(
    groups: &mut Vec<Group>,
    sections: &[SchemaSection],
    config_dir: &Path,
    with_preset: bool,
) -> (usize, usize) {
    let mut added = 0usize;
    let mut overwritten = 0usize;

    for sec in sections {
        let group = match groups.iter_mut().find(|g| g.name == sec.group) {
            Some(g) => g,
            None => {
                let dir = config_dir.join(&sec.group);
                groups.push(Group {
                    name: sec.group.clone(),
                    dir,
                    tables: Vec::new(),
                    constants: Vec::new(),
                    enums: Vec::new(),
                    is_new: true,
                });
                groups.last_mut().unwrap()
            }
        };

        match sec.mode {
            SchemaMode::Table => {
                let fields: Vec<FieldDef> = sec.fields.iter().map(|f| FieldDef {
                    name: f.name.clone(),
                    desc: f.desc.clone(),
                    tbl_type: f.tbl_type.clone(),
                    export: Export::from_str(&f.export),
                }).collect();
                let new_len = fields.len();

                let records: Vec<Vec<String>> = if with_preset {
                    sec.preset.iter().map(|row| {
                        let mut r = row.clone();
                        r.resize(new_len, String::new()); // 多余/缺失列对齐
                        r
                    }).collect()
                } else {
                    Vec::new()
                };

                if let Some(table) = group.tables.iter_mut().find(|t| t.name == sec.name) {
                    let old_len = table.schema.fields.len();
                    table.schema.fields = fields;
                    if with_preset {
                        // preset 覆盖现有 records（与 overwritten 语义一致）
                        table.records = records;
                    } else {
                        // 仅同步列结构，保留现有 records 但按新列数 resize
                        for row in &mut table.records {
                            row.resize(new_len, String::new());
                            if new_len < old_len { row.truncate(new_len); }
                        }
                    }
                    table.dirty = true;
                    overwritten += 1;
                } else {
                    let path = group.dir.join(format!("{}.tbl", sec.name));
                    group.tables.push(Table {
                        name: sec.name.clone(),
                        path,
                        schema: TableSchema { fields },
                        records,
                        dirty: true,
                        deleted: false,
                        original: String::new(),
                    });
                    added += 1;
                }
            }
            SchemaMode::Constant => {
                // preset 行：name | type | value | export | desc
                let entries: Vec<ConstEntry> = if with_preset {
                    sec.preset.iter().map(|row| ConstEntry {
                        name:     row.first()  .cloned().unwrap_or_default(),
                        tbl_type: row.get(1)   .cloned().unwrap_or_default(),
                        value:    row.get(2)   .cloned().unwrap_or_default(),
                        export:   Export::from_str(row.get(3).map(String::as_str).unwrap_or("cs")),
                        desc:     row.get(4)   .cloned().unwrap_or_default(),
                    }).collect()
                } else {
                    Vec::new()
                };

                if let Some(constant) = group.constants.iter_mut().find(|c| c.name == sec.name) {
                    if with_preset {
                        constant.entries = entries;
                    } // 不带预设：保留现有 entries，仅记一次 overwritten
                    constant.dirty = true;
                    overwritten += 1;
                } else {
                    let path = group.dir.join(format!("{}.tbl", sec.name));
                    group.constants.push(Constant {
                        name: sec.name.clone(),
                        path,
                        entries,
                        dirty: true,
                        deleted: false,
                        original: String::new(),
                    });
                    added += 1;
                }
            }
            SchemaMode::Enum => {
                // preset 行：id | name | desc
                let entries: Vec<EnumEntry> = if with_preset {
                    sec.preset.iter().map(|row| EnumEntry {
                        id:   row.first().cloned().unwrap_or_default(),
                        name: row.get(1) .cloned().unwrap_or_default(),
                        desc: row.get(2) .cloned().unwrap_or_default(),
                    }).collect()
                } else {
                    Vec::new()
                };

                if let Some(en) = group.enums.iter_mut().find(|e| e.name == sec.name) {
                    if with_preset {
                        en.entries = entries;
                    }
                    en.dirty = true;
                    overwritten += 1;
                } else {
                    let path = group.dir.join(format!("{}.tbl", sec.name));
                    group.enums.push(EnumDef {
                        name: sec.name.clone(),
                        path,
                        entries,
                        dirty: true,
                        deleted: false,
                        original: String::new(),
                    });
                    added += 1;
                }
            }
        }
    }

    (added, overwritten)
}

/// 解析后给缺失的 metadata 兜底：id 走文件名 stem，name 等于 id（如未填）。
/// 调用方在已知文件路径时使用：例如 `LocalTemplates` 扫目录时把 `<stem>` 灌进去。
pub fn fill_metadata_defaults(schema: &mut TblSchema, file_stem: &str) {
    if schema.meta.id.is_empty() {
        schema.meta.id = file_stem.to_string();
    }
    if schema.meta.name.is_empty() {
        schema.meta.name = schema.meta.id.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tblschema_with_sep_lines() {
        // # @sep 行覆写默认；未列出的字段保持默认；未知 key 忽略；列出位置在第一个 [section] 之前。
        let src = r#"#!tblschema v1
# @meta id: sep-test
# @sep Map.entry = ,
# @sep List = |
# @sep Map_Tuple3.kv = >
# @sep Unknown.key = ?

[g/T] table
id | int | cs | x
"#;
        let s = parse_tblschema(src).expect("parse");
        assert_eq!(s.separators.map.entry, ",");
        assert_eq!(s.separators.list, "|");
        assert_eq!(s.separators.map_tuple3.kv, ">");
        // 未覆写字段 = 默认
        assert_eq!(s.separators.set, ";");
        assert_eq!(s.separators.tuple2, ",");
        assert_eq!(s.separators.map.kv, ":");
    }

    #[test]
    fn serialize_tblschema_omits_default_sep() {
        // 全默认 separators → serialize 不应包含任何 # @sep 行
        let mut schema = TblSchema::default();
        schema.meta.id = "no-sep".to_string();
        schema.sections.push(SchemaSection {
            group: "g".to_string(),
            name: "T".to_string(),
            mode: SchemaMode::Table,
            fields: vec![SchemaField {
                name: "id".to_string(), tbl_type: "int".to_string(),
                export: "cs".to_string(), desc: "x".to_string(),
            }],
            preset: vec![],
        });
        let out = serialize_tblschema(&schema);
        assert!(!out.contains("# @sep"), "默认值不应输出 sep 行: {}", out);
    }

    #[test]
    fn serialize_tblschema_writes_modified_sep_round_trip() {
        // 改 Map.entry → 写盘 → 重新 parse → 仍是逗号
        let mut schema = TblSchema::default();
        schema.meta.id = "rt".to_string();
        schema.separators.map.entry = ",".to_string();
        schema.separators.tuple3 = "/".to_string();
        schema.sections.push(SchemaSection {
            group: "g".to_string(),
            name: "T".to_string(),
            mode: SchemaMode::Table,
            fields: vec![SchemaField {
                name: "id".to_string(), tbl_type: "int".to_string(),
                export: "cs".to_string(), desc: "x".to_string(),
            }],
            preset: vec![],
        });
        let out = serialize_tblschema(&schema);
        assert!(out.contains("# @sep Map.entry = ,"), "应写出 Map.entry: {}", out);
        assert!(out.contains("# @sep Tuple3 = /"), "应写出 Tuple3: {}", out);
        // List 是默认值 → 不应该出现
        assert!(!out.contains("# @sep List ="));

        let parsed = parse_tblschema(&out).expect("re-parse");
        assert_eq!(parsed.separators.map.entry, ",");
        assert_eq!(parsed.separators.tuple3, "/");
        assert_eq!(parsed.separators.list, ";"); // 仍是默认
    }

    #[test]
    fn parses_meta_lines() {
        let src = r#"#!tblschema v1
# @meta id: full
# @meta name: 完整测试模板
# @meta category: test
# @meta version: 1.0.0

[hero/HeroBase] table
id   | int | cs | 英雄ID
name | str | cs | 名称
"#;
        let s = parse_tblschema(src).expect("parse");
        assert_eq!(s.meta.id, "full");
        assert_eq!(s.meta.name, "完整测试模板");
        assert_eq!(s.meta.category, "test");
        assert_eq!(s.meta.version, "1.0.0");
        assert_eq!(s.sections.len(), 1);
    }

    #[test]
    fn meta_lines_only_before_first_section() {
        let src = r#"#!tblschema v1
# @meta id: a

[g/N] table
id | int | cs | x
# @meta name: should-be-ignored
y  | str | cs | y
"#;
        let s = parse_tblschema(src).expect("parse");
        assert_eq!(s.meta.id, "a");
        assert_eq!(s.meta.name, "a", "name 缺省 = id，第二段后的 @meta 应被当注释忽略");
    }

    #[test]
    fn meta_later_key_wins() {
        let src = r#"#!tblschema v1
# @meta id: a
# @meta id: b

[g/N] table
id | int | cs | x
"#;
        let s = parse_tblschema(src).expect("parse");
        assert_eq!(s.meta.id, "b");
    }

    #[test]
    fn meta_unknown_key_ignored() {
        let src = r#"#!tblschema v1
# @meta id: a
# @meta future_field: 42

[g/N] table
id | int | cs | x
"#;
        let s = parse_tblschema(src).expect("parse");
        assert_eq!(s.meta.id, "a");
    }

    #[test]
    fn legacy_no_meta_parses() {
        let src = r#"#!tblschema v1

[g/N] table
id | int | cs | x
"#;
        let s = parse_tblschema(src).expect("parse");
        assert_eq!(s.meta.id, "");
        assert_eq!(s.meta.name, "");
        assert_eq!(s.sections.len(), 1);
    }

    #[test]
    fn fill_metadata_defaults_uses_stem() {
        let mut s = TblSchema::default();
        fill_metadata_defaults(&mut s, "myfile");
        assert_eq!(s.meta.id, "myfile");
        assert_eq!(s.meta.name, "myfile");

        // id 已存在 → 不覆盖；name 仍按 id 兜底
        let mut s2 = TblSchema::default();
        s2.meta.id = "explicit".to_string();
        fill_metadata_defaults(&mut s2, "stem");
        assert_eq!(s2.meta.id, "explicit");
        assert_eq!(s2.meta.name, "explicit");
    }

    #[test]
    fn id_validation() {
        assert!(is_valid_metadata_id("full"));
        assert!(is_valid_metadata_id("slg-base"));
        assert!(is_valid_metadata_id("slg_base_2"));
        assert!(is_valid_metadata_id("a"));
        assert!(is_valid_metadata_id(&"a".repeat(32)));

        assert!(!is_valid_metadata_id(""));
        assert!(!is_valid_metadata_id(&"a".repeat(33)));
        assert!(!is_valid_metadata_id("Full"));            // 不接受大写
        assert!(!is_valid_metadata_id("foo bar"));          // 空格
        assert!(!is_valid_metadata_id("foo.bar"));          // 点
        assert!(!is_valid_metadata_id("中文"));
    }

    #[test]
    fn serialize_round_trips_meta() {
        let mut s = TblSchema::default();
        s.meta.id = "demo".to_string();
        s.meta.name = "演示".to_string();
        s.meta.category = "test".to_string();
        s.meta.version = "0.1.0".to_string();
        s.sections.push(SchemaSection {
            group: "g".to_string(),
            name: "N".to_string(),
            mode: SchemaMode::Table,
            fields: vec![SchemaField {
                name: "id".to_string(),
                tbl_type: "int".to_string(),
                export: "cs".to_string(),
                desc: "x".to_string(),
            }],
            preset: Vec::new(),
        });
        let txt = serialize_tblschema(&s);
        assert!(txt.contains("# @meta id: demo"));
        assert!(txt.contains("# @meta name: 演示"));
        assert!(txt.contains("# @meta category: test"));
        assert!(txt.contains("# @meta version: 0.1.0"));

        let back = parse_tblschema(&txt).expect("re-parse");
        assert_eq!(back.meta.id, "demo");
        assert_eq!(back.meta.name, "演示");
        assert_eq!(back.meta.category, "test");
        assert_eq!(back.meta.version, "0.1.0");
        assert_eq!(back.sections.len(), 1);
    }

    #[test]
    fn parses_preset_block_for_table() {
        let src = r#"#!tblschema v1
[hero/HeroBase] table
id   | int | cs | 英雄ID
name | str | cs | 名称
# @preset
1 | Alice
2 | Bob
"#;
        let s = parse_tblschema(src).expect("parse");
        assert_eq!(s.sections.len(), 1);
        assert_eq!(s.sections[0].fields.len(), 2);
        assert_eq!(s.sections[0].preset.len(), 2);
        assert_eq!(s.sections[0].preset[0], vec!["1", "Alice"]);
        assert_eq!(s.sections[0].preset[1], vec!["2", "Bob"]);
        assert!(s.meta.has_preset);
    }

    #[test]
    fn parses_preset_block_for_constant() {
        let src = r#"#!tblschema v1
[global/GlobalConst] constant
# @preset
max_level | int | 100 | cs | 最大等级
"#;
        let s = parse_tblschema(src).expect("parse");
        assert_eq!(s.sections.len(), 1);
        assert_eq!(s.sections[0].mode, SchemaMode::Constant);
        assert!(s.sections[0].fields.is_empty(), "constant 段 fields 应为空");
        assert_eq!(s.sections[0].preset.len(), 1);
        assert_eq!(s.sections[0].preset[0], vec!["max_level", "int", "100", "cs", "最大等级"]);
        assert!(s.meta.has_preset);
    }

    #[test]
    fn parses_preset_block_for_enum() {
        let src = r#"#!tblschema v1
[hero/HeroType] enum
# @preset
1 | Warrior | 战士
2 | Mage    | 法师
"#;
        let s = parse_tblschema(src).expect("parse");
        assert_eq!(s.sections.len(), 1);
        assert_eq!(s.sections[0].mode, SchemaMode::Enum);
        assert_eq!(s.sections[0].preset.len(), 2);
        assert!(s.meta.has_preset);
    }

    #[test]
    fn constant_section_rejects_inline_entries() {
        let src = r#"#!tblschema v1
[global/GlobalConst] constant
max_level | int | cs | 最大等级
"#;
        let err = parse_tblschema(src).expect_err("constant 段不允许直接 entry 行");
        let msg = err.to_string();
        assert!(msg.contains("constant"), "msg = {}", msg);
        assert!(msg.contains("@preset"), "msg = {}", msg);
    }

    #[test]
    fn enum_section_rejects_inline_entries() {
        let src = r#"#!tblschema v1
[hero/HeroType] enum
1 | Warrior | 战士
"#;
        let err = parse_tblschema(src).expect_err("enum 段不允许直接 entry 行");
        let msg = err.to_string();
        assert!(msg.contains("enum"), "msg = {}", msg);
        assert!(msg.contains("@preset"), "msg = {}", msg);
    }

    #[test]
    fn preset_terminates_at_next_section() {
        let src = r#"#!tblschema v1
[a/T] table
id | int | cs | x
# @preset
1 | foo
[b/U] table
id | int | cs | y
"#;
        let s = parse_tblschema(src).expect("parse");
        assert_eq!(s.sections.len(), 2);
        assert_eq!(s.sections[0].preset.len(), 1);
        assert_eq!(s.sections[1].preset.len(), 0);
        assert_eq!(s.sections[1].fields.len(), 1);
    }

    #[test]
    fn has_preset_recomputed_from_sections() {
        // schema 文件里写 has_preset: true，但实际 sections.preset 全空 → 重算为 false
        let src = r#"#!tblschema v1
# @meta has_preset: true

[g/N] table
id | int | cs | x
"#;
        let s = parse_tblschema(src).expect("parse");
        assert!(!s.meta.has_preset, "应该按 sections 重算");
    }

    #[test]
    fn serialize_round_trips_preset() {
        let schema = TblSchema {
            meta: SchemaMetadata { id: "demo".into(), name: "demo".into(), ..Default::default() },
            separators: Default::default(),
            sections: vec![
                SchemaSection {
                    group: "hero".into(),
                    name: "HeroBase".into(),
                    mode: SchemaMode::Table,
                    fields: vec![
                        SchemaField { name: "id".into(), tbl_type: "int".into(), export: "cs".into(), desc: "ID".into() },
                        SchemaField { name: "name".into(), tbl_type: "str".into(), export: "cs".into(), desc: "名".into() },
                    ],
                    preset: vec![
                        vec!["1".into(), "Alice".into()],
                        vec!["2".into(), "Bob".into()],
                    ],
                },
                SchemaSection {
                    group: "global".into(),
                    name: "GlobalConst".into(),
                    mode: SchemaMode::Constant,
                    fields: vec![],
                    preset: vec![
                        vec!["max_level".into(), "int".into(), "100".into(), "cs".into(), "最大等级".into()],
                    ],
                },
                SchemaSection {
                    group: "hero".into(),
                    name: "HeroType".into(),
                    mode: SchemaMode::Enum,
                    fields: vec![],
                    preset: vec![
                        vec!["1".into(), "WARRIOR".into(), "战士".into()],
                    ],
                },
            ],
        };
        let txt = serialize_tblschema(&schema);
        assert!(txt.contains("# @meta has_preset: true"), "txt = {}", txt);
        assert!(txt.contains("# @preset"));
        let back = parse_tblschema(&txt).expect("re-parse");
        assert_eq!(back.sections.len(), 3);
        assert!(back.meta.has_preset);
        assert_eq!(back.sections[0].preset.len(), 2);
        assert_eq!(back.sections[0].preset[0], vec!["1", "Alice"]);
        assert_eq!(back.sections[1].mode, SchemaMode::Constant);
        assert_eq!(back.sections[1].preset.len(), 1);
        assert_eq!(back.sections[1].preset[0], vec!["max_level", "int", "100", "cs", "最大等级"]);
        assert_eq!(back.sections[2].mode, SchemaMode::Enum);
        assert_eq!(back.sections[2].preset.len(), 1);
    }

    #[test]
    fn serialize_constant_enum_no_inline_entries() {
        // Constant / Enum 段在 schema 文件里只剩段头（哪怕 fields 非空也不输出，
        // 因为新范式 entries 全在项目 .tbl 里）。
        let schema = TblSchema {
            meta: SchemaMetadata::default(),
            separators: Default::default(),
            sections: vec![
                SchemaSection {
                    group: "g".into(),
                    name: "C".into(),
                    mode: SchemaMode::Constant,
                    fields: vec![SchemaField {
                        name: "ignored".into(), tbl_type: "int".into(), export: "cs".into(), desc: "x".into()
                    }],
                    preset: vec![],
                },
                SchemaSection {
                    group: "g".into(),
                    name: "E".into(),
                    mode: SchemaMode::Enum,
                    fields: vec![SchemaField {
                        name: "WAR".into(), tbl_type: "1".into(), export: String::new(), desc: "".into()
                    }],
                    preset: vec![],
                },
            ],
        };
        let txt = serialize_tblschema(&schema);
        // 段头有
        assert!(txt.contains("[g/C] constant"));
        assert!(txt.contains("[g/E] enum"));
        // entry 行没有
        assert!(!txt.contains("ignored"));
        assert!(!txt.contains("WAR"));
        // 没有 preset 块
        assert!(!txt.contains("@preset"));
        assert!(!txt.contains("has_preset"));
    }
}
