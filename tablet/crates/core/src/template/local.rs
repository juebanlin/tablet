//! 本地模板源：扫描程序根目录 `<binary-dir>/tblschema/` 下的 `.tblschema` 文件。
//!
//! 用户拷贝模板文件即可扩展模板库，不依赖工具版本。
//!
//! 解析失败的文件会被静默跳过（带 log 提示）；缺 metadata 的文件按文件名 stem 兜底。

use std::path::PathBuf;

use super::{TemplateContent, TemplateMeta, TemplateSource};
use crate::tblschema::{fill_metadata_defaults, parse_tblschema};

#[derive(Debug, Clone)]
pub struct LocalTemplates {
    /// 模板目录的绝对路径。不存在视作空源。
    pub root: PathBuf,
}

impl LocalTemplates {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// 扫描目录并解析。每条返回 (file_path, TemplateContent)；解析失败的文件不在结果里。
    fn scan(&self) -> Vec<(PathBuf, TemplateContent)> {
        let mut out = Vec::new();
        if !self.root.is_dir() {
            return out;
        }

        let entries = match std::fs::read_dir(&self.root) {
            Ok(e) => e,
            Err(_) => return out,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("tblschema") {
                continue;
            }

            let raw = match std::fs::read_to_string(&path) {
                Ok(s) => s,
                Err(_) => continue,
            };

            let mut schema = match parse_tblschema(&raw) {
                Ok(s) => s,
                Err(_) => continue,
            };

            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            fill_metadata_defaults(&mut schema, &stem);

            let meta = TemplateMeta::from_schema(&schema, "local");
            out.push((path, TemplateContent { meta, raw, schema }));
        }

        out
    }
}

impl TemplateSource for LocalTemplates {
    fn list(&self) -> Vec<TemplateMeta> {
        self.scan().into_iter().map(|(_, c)| c.meta).collect()
    }

    fn load_by_id(&self, id: &str) -> Option<TemplateContent> {
        self.scan()
            .into_iter()
            .find(|(_, c)| c.meta.id == id)
            .map(|(_, c)| c)
    }
}

/// 推断默认本地模板目录：可执行文件所在目录下的 `tblschema/`。
///
/// 失败（如取不到 current_exe）时返回空 PathBuf，调用方自行降级处理。
pub fn default_local_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .map(|d| d.join("tblschema"))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn unique_tmp(label: &str) -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("tblschema_local_{}_{}_{}", label, std::process::id(), n))
    }

    #[test]
    fn empty_dir_returns_no_templates() {
        let dir = unique_tmp("empty");
        std::fs::create_dir_all(&dir).unwrap();
        let src = LocalTemplates::new(&dir);
        assert!(src.list().is_empty());
        assert!(src.load_by_id("anything").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_dir_returns_no_templates() {
        let dir = unique_tmp("missing");
        // 故意不创建
        let src = LocalTemplates::new(&dir);
        assert!(src.list().is_empty());
    }

    #[test]
    fn picks_up_meta_and_falls_back_to_stem() {
        let dir = unique_tmp("scan");
        std::fs::create_dir_all(&dir).unwrap();

        // 有 meta 的模板
        std::fs::write(
            dir.join("with-meta.tblschema"),
            "#!tblschema v1\n# @meta id: explicit\n# @meta name: 显式名\n\n[g/N] table\nid|int|cs|x\n",
        )
        .unwrap();

        // 无 meta 的模板（按文件名 stem 兜底）
        std::fs::write(
            dir.join("legacy.tblschema"),
            "#!tblschema v1\n\n[g/N] table\nid|int|cs|x\n",
        )
        .unwrap();

        // 非 .tblschema 文件应被忽略
        std::fs::write(dir.join("notes.txt"), "ignored").unwrap();

        let src = LocalTemplates::new(&dir);
        let mut ids: Vec<String> = src.list().iter().map(|m| m.id.clone()).collect();
        ids.sort();
        assert_eq!(ids, vec!["explicit".to_string(), "legacy".to_string()]);

        let c = src.load_by_id("explicit").unwrap();
        assert_eq!(c.meta.name, "显式名");
        assert_eq!(c.meta.source, "local");

        let c = src.load_by_id("legacy").unwrap();
        assert_eq!(c.meta.name, "legacy"); // name 兜底 = id

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn malformed_template_is_skipped_not_fatal() {
        let dir = unique_tmp("malformed");
        std::fs::create_dir_all(&dir).unwrap();

        std::fs::write(
            dir.join("good.tblschema"),
            "#!tblschema v1\n# @meta id: ok\n\n[g/N] table\nid|int|cs|x\n",
        )
        .unwrap();
        // 缺 mode → 解析失败
        std::fs::write(
            dir.join("bad.tblschema"),
            "#!tblschema v1\n[g/N] not-a-mode\nid|int|cs|x\n",
        )
        .unwrap();

        let src = LocalTemplates::new(&dir);
        let ids: Vec<String> = src.list().iter().map(|m| m.id.clone()).collect();
        assert_eq!(ids, vec!["ok".to_string()]);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
