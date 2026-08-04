// 子对话框模块（右键菜单 / 输入&确认 / 类型选择器 / 引用选择器 /
// 数据导出 / Schema 导入导出 / 新建项目（统一） / 克隆项目 / 项目设置 / 全局设置）。
// 每个模块统一暴露 `wire(ui, state)` 注册 callback，`push(ui, state)` 把 state 同步到 UI。

pub mod context_menu;
pub mod pending;
pub mod type_selector;
pub mod ref_picker;
pub mod data_export;
pub mod schema_io;
pub mod create_project;
pub mod clone_project;
pub mod project_settings;
pub mod global_settings;
