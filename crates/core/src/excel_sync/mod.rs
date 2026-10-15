//! Excel 同步引擎（`.excel/*.xlsx` ↔ 内存 group）。
//!
//! # 定位
//!
//! 这条流是**让策划自由操作 Excel**：约定列名不改、desc 可改、颜色与行高列宽随便
//! 调，而这份 xlsx 与 tbl 之间**只交换 desc + 数据**。表头是 4 行（desc / export /
//! type / 列名），与 `crate::excel::write_table` 逐格一致。
//!
//! 与 `crate::excel`（严格导入导出桥）是两条独立的流、两套库，互不影响。
//!
//! # 形状：两侧投影 → 决策 → 定点落盘
//!
//! ```text
//! Group ──投影──► SheetVo ─┐
//!                          ├── plan_group()（纯函数）──► GroupPlan ──┬──► 定点写入 Group
//! Workbook ─投影─► SheetVo ─┘                                        └──► 定点写入 Workbook ──► write
//!                                                                         （NOT: VO → 文件）
//! ```
//!
//! | 模块 | 职责 |
//! |---|---|
//! | [`vo`] | 两侧共用的行列表模型，**读取边界无损**；不承载样式 |
//! | [`read`] | `Group` / `Workbook` → `SheetVo`。只读，不写回 |
//! | [`plan`] | 决策：两侧 VO + schema → [`GroupPlan`]。**纯函数**，列对齐的黑盒就在这一个文件里 |
//! | [`apply`] | 把一次 [`SyncOp`] 作用到两个真相对象上。**补丁应用器，不是序列化器** |
//!
//! 决策之所以要物化成数据：面板「显示什么」和「点下去做什么」原本是两条独立推导的
//! 路径（Slint 谓词算一个、Rust 的 action 分支算另一个），靠手工对齐。现在两边读
//! 同一份 [`PairPlan`]。
//!
//! # 数据安全
//!
//! - [`apply::execute`] 每次都**重算计划**并校验操作仍然合法；面板过期就拒绝，
//!   不拿旧快照去写新文件。
//! - 回读（[`SyncOp::Pull`]）会正确产生未保存标记（`update_dirty()` 比对落盘快照，
//!   不伪造快照），所以同步完要**保存**才真正写进 `.tbl`。
//! - 推送走 `lazy_read`，没被同步的 sheet 逐字节原样保留；被同步的那几张会丢掉
//!   umya 没有建模的元素，清单见 @06 §7.4。

pub mod apply;
pub mod plan;
pub mod read;
pub mod vo;

#[cfg(test)]
mod testutil;

pub use apply::execute;
pub use plan::{
    compute_column_match, plan_group, ColumnMatch, GroupPlan, PairPlan, PushScope, SheetSummary,
    SyncOp,
};
pub use read::{read_xlsx_sheets, tbl_sheet_vo};
pub use vo::{
    ColumnVo, Layout, RowVo, SheetRange, SheetVo, DATA_START, HDR_DESC, HDR_EXPORT, HDR_NAME,
    HDR_TYPE,
};
