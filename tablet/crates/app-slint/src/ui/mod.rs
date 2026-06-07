// 主窗口区域模块（树 / 表格 / 工具栏 / 日志 / 焦点）。
// 每个模块统一暴露 `wire(ui, state)` 注册 callback，`push(ui, state)` 把 state 同步到 UI。

pub mod tree;
pub mod grid;
pub mod grid_actions;
pub mod toolbar;
pub mod log_panel;
pub mod focus;
