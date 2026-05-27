use eframe::egui;

pub const TYPES: &[&str] = &["int", "long", "float", "str", "bool", "IntPair", "StrPair", "IntTriple", "IntArray"];
pub const EXPORTS: &[&str] = &["前后端", "客户端", "服务器", "不导出"];

#[derive(Clone, Debug, PartialEq)]
pub enum CellKind {
    ReadOnly,
    Text,
    TypeEnum,
    ExportEnum,
    TypeEnumCol,
    ExportEnumCol,
    Reference { table: String },
}

impl CellKind {
    pub fn selectable(&self) -> bool {
        !matches!(self, Self::ReadOnly)
    }

    pub fn click_to_edit(&self) -> bool {
        matches!(self, Self::TypeEnum | Self::ExportEnum
            | Self::TypeEnumCol | Self::ExportEnumCol
            | Self::Reference { .. })
    }

    pub fn double_click_to_edit(&self) -> bool {
        matches!(self, Self::Text)
    }

    pub fn show_dropdown_arrow(&self) -> bool {
        self.click_to_edit()
    }

    pub fn copyable(&self) -> bool {
        !matches!(self, Self::ReadOnly)
    }

    pub fn deletable(&self) -> bool {
        matches!(self, Self::Text)
    }

    pub fn enum_options(&self) -> &'static [&'static str] {
        match self {
            Self::TypeEnum | Self::TypeEnumCol => TYPES,
            Self::ExportEnum | Self::ExportEnumCol => EXPORTS,
            _ => &[],
        }
    }
}

#[derive(Clone, Debug)]
pub struct ColDef {
    pub kind: CellKind,
}

#[derive(Clone, Debug)]
pub struct HeaderCell {
    pub text: String,
    pub kind: CellKind,
    pub color: egui::Color32,
}

#[derive(Clone, Debug)]
pub enum GridSource {
    Table,
    Constant,
}

#[derive(Clone, Debug)]
pub struct GridData {
    pub source: GridSource,
    pub header_rows: Vec<Vec<HeaderCell>>,
    pub col_defs: Vec<ColDef>,
    pub data: Vec<Vec<String>>,
    pub data_count: usize,
}
