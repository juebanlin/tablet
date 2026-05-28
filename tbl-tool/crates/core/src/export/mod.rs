mod json;
mod java;
mod xml;
mod lua;

pub use json::export_all_json;
pub use java::export_all_java;
pub use xml::export_all_xml;
pub use lua::export_all_lua;

use serde_json::Value;
use crate::types::BaseType;

pub enum EmptyStrategy {
    Null,
    Empty,
    Omit,
}

impl EmptyStrategy {
    pub fn from_json_config(s: &str) -> Self {
        match s {
            "omit" => Self::Omit,
            _ => Self::Null,
        }
    }

    pub fn from_xml_config(s: &str) -> Self {
        match s {
            "omit" => Self::Omit,
            _ => Self::Empty,
        }
    }
}

pub enum LineEnding {
    Lf,
    Crlf,
}

impl LineEnding {
    pub fn from_config(s: &str) -> Self {
        match s {
            "crlf" => Self::Crlf,
            _ => Self::Lf,
        }
    }

    pub fn normalize(&self, content: &str) -> String {
        let lf = content.replace("\r\n", "\n");
        match self {
            Self::Lf => lf,
            Self::Crlf => lf.replace('\n', "\r\n"),
        }
    }
}

pub fn encode_content(content: &str, encoding: &str) -> Vec<u8> {
    match encoding {
        "utf-8" | "utf8" => content.as_bytes().to_vec(),
        _ => {
            let enc = encoding_rs::Encoding::for_label(encoding.as_bytes())
                .unwrap_or(encoding_rs::UTF_8);
            let (bytes, _, _) = enc.encode(content);
            bytes.into_owned()
        }
    }
}

pub struct ExportOptions {
    pub line_ending: LineEnding,
    pub encoding: String,
}

impl ExportOptions {
    pub fn encode(&self, content: &str) -> Vec<u8> {
        let normalized = self.line_ending.normalize(content);
        encode_content(&normalized, &self.encoding)
    }

    pub fn write_file(&self, path: &std::path::Path, content: &str) -> anyhow::Result<()> {
        std::fs::create_dir_all(path.parent().unwrap())?;
        let bytes = self.encode(content);
        std::fs::write(path, bytes)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum FileStatus {
    Added,
    Modified,
    Unchanged,
    Deleted,
}

#[derive(Debug, Clone)]
pub struct ExportFile {
    pub path: String,
    pub status: FileStatus,
}

#[derive(Debug, Clone)]
pub struct ExportResult {
    pub files: Vec<ExportFile>,
}

impl ExportResult {
    pub fn added(&self) -> usize { self.files.iter().filter(|f| f.status == FileStatus::Added).count() }
    pub fn modified(&self) -> usize { self.files.iter().filter(|f| f.status == FileStatus::Modified).count() }
    pub fn unchanged(&self) -> usize { self.files.iter().filter(|f| f.status == FileStatus::Unchanged).count() }
    pub fn deleted(&self) -> usize { self.files.iter().filter(|f| f.status == FileStatus::Deleted).count() }
}

pub fn sync_export_dir(
    output_dir: &std::path::Path,
    extension: &str,
    generated: Vec<(std::path::PathBuf, Vec<u8>)>,
) -> anyhow::Result<ExportResult> {
    use std::collections::HashSet;
    use std::path::PathBuf;

    let mut existing: HashSet<PathBuf> = HashSet::new();
    if output_dir.exists() {
        collect_files_recursive(output_dir, extension, &mut existing);
    }

    let mut files = Vec::new();

    for (path, content) in &generated {
        std::fs::create_dir_all(path.parent().unwrap())?;
        existing.remove(path);
        let display = normalize_path(path);

        if path.exists() {
            let old = std::fs::read(path)?;
            if old == *content {
                files.push(ExportFile { path: display, status: FileStatus::Unchanged });
            } else {
                std::fs::write(path, content)?;
                files.push(ExportFile { path: display, status: FileStatus::Modified });
            }
        } else {
            std::fs::write(path, content)?;
            files.push(ExportFile { path: display, status: FileStatus::Added });
        }
    }

    for old_path in &existing {
        std::fs::remove_file(old_path)?;
        files.push(ExportFile { path: normalize_path(old_path), status: FileStatus::Deleted });
    }

    remove_empty_dirs(output_dir);

    Ok(ExportResult { files })
}

fn normalize_path(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn collect_files_recursive(dir: &std::path::Path, ext: &str, out: &mut std::collections::HashSet<std::path::PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_files_recursive(&path, ext, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some(ext) {
                out.insert(path);
            }
        }
    }
}

fn remove_empty_dirs(dir: &std::path::Path) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                remove_empty_dirs(&path);
                let _ = std::fs::remove_dir(&path);
            }
        }
    }
}

pub fn to_camel_case(s: &str) -> String {
    let mut result = String::new();
    let mut upper_next = false;
    for ch in s.chars() {
        if ch == '_' {
            upper_next = true;
        } else if upper_next {
            result.push(ch.to_ascii_uppercase());
            upper_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}

pub fn to_pascal_case(s: &str) -> String {
    let camel = to_camel_case(s);
    let mut chars = camel.chars();
    match chars.next() {
        Some(c) => c.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

pub fn parse_base_value(raw: &str, bt: &BaseType) -> Value {
    match bt {
        BaseType::Int | BaseType::Long => {
            raw.parse::<i64>().map(Value::from).unwrap_or(Value::from(0))
        }
        BaseType::Float | BaseType::Double => {
            raw.parse::<f64>().map(Value::from).unwrap_or(Value::from(0.0))
        }
        BaseType::Bool => Value::from(raw == "true" || raw == "1"),
        BaseType::Str => Value::from(raw),
    }
}
