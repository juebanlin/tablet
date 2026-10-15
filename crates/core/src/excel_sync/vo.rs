//! 两侧共用的行列表模型（VO）。
//!
//! 这是**边界 DTO**：格子里就是字符串，类型化信息在 schema 里，不塞进来。
//! 一套 VO 服务两侧——tbl 与 xlsx 的形状是同一个东西（列 + 行），不一样的是来源
//! 与可信度，不是结构。做成两个类型只会让决策阶段到处挂适配器。
//!
//! **读取边界无损**：VO 是「文件长什么样」的完整投影。有损只发生在两个显式的地方
//! ——`GroupPlan`（决策产物）与 `SheetSummary`（面板显示用的统计）。
//!
//! **VO 不承载样式**（填充/字体/边框/列宽行高）。样式不是同步的交换物，而是必须
//! 原样留存的东西，留存的机制是 apply 的定点写入、不是 VO。把样式塞进来会诱导出
//! 「VO → 文件」的序列化写法，那条路会毁掉保真（见 `apply` 模块注释）。

// ── 表头布局 ──

/// Table sheet 的 4 行表头。顺序是**约定**：列名行必须在最后一行，列匹配靠它。
pub const HDR_DESC: u32 = 1;
pub const HDR_EXPORT: u32 = 2;
pub const HDR_TYPE: u32 = 3;
pub const HDR_NAME: u32 = 4;
/// 数据行起点
pub const DATA_START: u32 = 5;

/// 旧布局（本特性之前生成的文件）：只有 1 行列名，数据从第 2 行起
pub const LEGACY_HEADER_ROW: u32 = 1;
pub const LEGACY_DATA_START: u32 = 2;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layout {
    /// desc / export / type / 列名 四行
    Modern,
    /// 只有列名一行
    Legacy,
}

impl Layout {
    pub fn name_row(self) -> u32 {
        match self {
            Layout::Modern => HDR_NAME,
            Layout::Legacy => LEGACY_HEADER_ROW,
        }
    }

    pub fn data_start(self) -> u32 {
        match self {
            Layout::Modern => DATA_START,
            Layout::Legacy => LEGACY_DATA_START,
        }
    }
}

// ── VO ──

/// 工作表实际用到的范围（1-based）。
///
/// 用命名字段而不是 `(u32, u32)`：umya 的 `highest_column_and_row()` 返回的是
/// `(col, row)`——列在前，反直觉，这个仓库已经因此踩过一次。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SheetRange {
    pub max_col: u32,
    pub max_row: u32,
}

impl SheetRange {
    pub fn is_empty(self) -> bool {
        self.max_col == 0 || self.max_row == 0
    }
}

/// 一份 sheet 的完整投影。两侧同型。
pub struct SheetVo {
    pub name: String,
    /// 列轴：区域内的列，按物理顺序。`index` 是真实列号。
    pub columns: Vec<ColumnVo>,
    /// 行轴：只含**有效行**（尾部空行不进）。`index` 是真实行号——中间的空行
    /// 不进 `rows`，但体现为 index 跳号，所以位置不会丢。
    pub rows: Vec<RowVo>,
    /// 全表的真实范围。区域之外的内容不进 VO，但也不消失——记在这里。
    /// `range.max_col > columns.len()` 就是「表头之外还有东西」的明确信号。
    pub range: SheetRange,
}

pub struct ColumnVo {
    /// 1-based 物理列号。**位置不等于身份**：列名是身份，列号是落点，两个都要留，
    /// 否则「按列名匹配」与「按位置对齐」这两种机制就没法互换。
    pub index: u32,
    /// 列名。区域由列名行界定，但列名行内部可能有空格，所以是 Option。
    pub name: Option<String>,
    /// desc 格。`None` = 这一格不存在（旧布局整行都不存在）。
    pub desc: Option<String>,
    pub ty: Option<String>,
    pub export: Option<String>,
}

pub struct RowVo {
    /// 1-based 物理行号
    pub index: u32,
    /// 与 `columns` 等长，不足补空串
    pub cells: Vec<String>,
}

impl SheetVo {
    /// 有没有 desc 行。
    ///
    /// 旧布局整行都不存在，所以每列的 `desc` 都是 `None`；新布局即使内容全空，
    /// 格也是存在的（`Some("")`）。**不要**为此再给 `SheetVo` 加一个 layout 字段
    /// ——那是同一个事实的第二处表达。
    pub fn has_desc_row(&self) -> bool {
        self.columns.iter().any(|c| c.desc.is_some())
    }

    /// 表头之外还有内容吗（区域界定没法表达的那部分）。
    pub fn has_content_outside_region(&self) -> bool {
        self.range.max_col > self.columns.len() as u32
    }
}
