// state→UI 派生：把 AppState 投影成 slint Model。
//
// 拆分：
// - tree.rs   ：树面板（TreeNode 列表 + state.tree_targets 同步）
// - grid.rs   ：表格区（GridSnapshot + 三种节点类型的 build_*_grid）
// - util.rs   ：col_letter / raw_cell_for / 单格校验消息（坐标 + 单格读值）
// - style.rs  ：颜色 + ICON 常量
//
// 对外 API：build_tree_nodes / build_grid / EXTRA_ROWS / col_letter / raw_cell_for。
// GridSnapshot / sorted_available 仅 convert 内部使用，不向外重导。

mod grid;
mod tree;
mod util;

pub use grid::build_grid;
pub use tree::build_tree_nodes;
pub use util::{col_letter, raw_cell_for, EXTRA_ROWS};
