//! excel_sync 单测共用的夹具。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::model::{Export, FieldDef, Group, Table, TableSchema};
use crate::tbl::DEFAULT_TBL_VERSION;

/// 两份夹具共用的字段：一列 int、一列 str。
pub const FIELD_SPEC: [(&str, &str, &str); 2] =
    [("id", "编号", "int"), ("name", "名字", "str")];

pub fn fields() -> Vec<FieldDef> {
    FIELD_SPEC.iter().map(|(n, d, t)| FieldDef {
        name: n.to_string(),
        desc: d.to_string(),
        tbl_type: t.to_string(),
        export: Export::ClientServer,
    }).collect()
}

pub fn table(name: &str, records: Vec<Vec<&str>>) -> Table {
    let f = fields();
    Table {
        name: name.to_string(),
        original_name: name.to_string(),
        path: PathBuf::from(format!("{}.tbl", name)),
        schema: TableSchema { fields: f.clone() },
        records: records.iter().map(|r| r.iter().map(|s| s.to_string()).collect()).collect(),
        dirty: false,
        deleted: false,
        original_records: vec![],
        original_fields: f,
        saved: false,
        fmt_ver: DEFAULT_TBL_VERSION,
        display_field: None,
        original_display_field: None,
    }
}

pub fn group(records: Vec<Vec<&str>>) -> Group {
    Group {
        name: "hero".into(),
        original_name: "hero".into(),
        dir: PathBuf::from("hero"),
        tables: vec![table("HeroBase", records)],
        constants: vec![],
        enums: vec![],
        is_new: false,
    }
}

pub fn data() -> Vec<Vec<&'static str>> {
    vec![vec!["1", "alice"], vec!["2", "bob"]]
}

/// 临时 xlsx：drop 时自动删除。
pub struct TempXlsx(PathBuf);

impl TempXlsx {
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir()
            .join(format!("tablet_excel_sync_{}_{}.xlsx", std::process::id(), n));
        TempXlsx(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempXlsx {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// 直接取 xlsx（zip）里某个条目的**原始字节**。
///
/// 保真那条红线要用它：没被同步的 sheet 是 zip 层原始压缩字节直通，逐字节不变；
/// 一旦末段变成「序列化 VO」，字节立刻不同。
pub fn zip_entry(path: &Path, name: &str) -> Vec<u8> {
    use std::io::Read;
    let file = std::fs::File::open(path).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    let mut entry = archive.by_name(name)
        .unwrap_or_else(|_| panic!("zip 里没有 {name}"));
    let mut buf = Vec::new();
    entry.read_to_end(&mut buf).unwrap();
    buf
}
