//! 每个 Project 的会话内撤销/重做历史。
//!
//! 设计约束（见 docs/12-保存与连续撤销.md）：
//! - 历史只活在内存里，不写 `.data`，不跨重启；
//! - 每个 Project 一份栈，切换 active 不清栈；
//! - 保存当前 Project 成功后清空该项目的栈，保存失败保留；
//! - 撤销/重做按「事务」为单位，事务里放的是**改动作用域的快照**，
//!   不是为每个操作手写逆函数（@07.S28 的快照式方向）。
//!
//! 快照分三档：
//! - [`CellSnapshot`]：Table 单元格（含行/列形状），单元格编辑是最高频路径；
//! - [`NodeBody`]：Constant / Enum 单元格、Table 行/列/表头等节点级修改；
//! - [`Project`]：节点/组改名、粘贴、Group 增删等跨节点结构操作。
//!
//! **不要把 `original_*` / dirty 的恢复逻辑塞进 undo**：保存成功后栈就清了，
//! 保存失败时也不应该把「上次落盘基线」一起撤回；撤销后由 `update_dirty()` 重算。

use crate::model::{NodeBody, NodeKind, Project};

/// 每个 Project 最多保留多少步（与 Excel 默认 100 级一致）。
pub const HISTORY_LIMIT: usize = 100;

/// Table 单元格快照。`before_rows` / `after_rows` 等形状信息用于撤销「输入时自动补行」。
#[derive(Clone, Debug, PartialEq)]
pub struct CellSnapshot {
    pub project: String,
    pub group: String,
    pub node: String,
    pub row: usize,
    pub col: usize,
    pub before: String,
    pub after: String,
    pub before_rows: usize,
    pub after_rows: usize,
    pub before_row_len: usize,
    pub after_row_len: usize,
}

/// 一条事务里的一个原子改动。
#[derive(Clone, Debug, PartialEq)]
pub enum EditOp {
    Cell(CellSnapshot),
    Node {
        project: String,
        group: String,
        kind: NodeKind,
        /// 事务发生时的节点名；节点级操作不改名（改名走 Project 快照）。
        name: String,
        before: NodeBody,
        after: NodeBody,
    },
    Project {
        project_id: String,
        before: Project,
        after: Project,
    },
}

/// 一次用户动作（一个撤销单元）。
#[derive(Clone, Debug, PartialEq)]
pub struct Transaction {
    pub label: String,
    pub ops: Vec<EditOp>,
}

impl Transaction {
    pub fn new(label: impl Into<String>, ops: Vec<EditOp>) -> Self {
        Self { label: label.into(), ops }
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
}

/// 单个 Project 的 undo / redo 栈。
#[derive(Clone, Debug, Default)]
pub struct EditHistory {
    undo: Vec<Transaction>,
    redo: Vec<Transaction>,
}

impl EditHistory {
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|t| t.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|t| t.label.as_str())
    }

    /// 记录一个新事务；新事务会清空 redo（线性历史）。
    /// 超过上限时丢最旧的一条。
    pub fn push(&mut self, tx: Transaction) {
        if tx.is_empty() {
            return;
        }
        self.undo.push(tx);
        if self.undo.len() > HISTORY_LIMIT {
            let excess = self.undo.len() - HISTORY_LIMIT;
            self.undo.drain(0..excess);
        }
        self.redo.clear();
    }

    /// 弹出倒数第二个事务；调用方应用 before 快照后调用 [`push_redo`]。
    pub fn pop_undo(&mut self) -> Option<Transaction> {
        self.undo.pop()
    }

    pub fn push_redo(&mut self, tx: Transaction) {
        if !tx.is_empty() {
            self.redo.push(tx);
        }
    }

    pub fn pop_redo(&mut self) -> Option<Transaction> {
        self.redo.pop()
    }

    pub fn push_undo_after_redo(&mut self, tx: Transaction) {
        if !tx.is_empty() {
            self.undo.push(tx);
            if self.undo.len() > HISTORY_LIMIT {
                let excess = self.undo.len() - HISTORY_LIMIT;
                self.undo.drain(0..excess);
            }
        }
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    /// 截断到指定 undo 长度，并清空 redo。
    /// 给模态编辑面板的「取消」用：把打开面板之后产生的临时历史一起收掉。
    pub fn truncate_undo(&mut self, undo_len: usize) {
        self.undo.truncate(undo_len);
        self.redo.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tx(label: &str) -> Transaction {
        Transaction::new(label, Vec::new())
    }

    fn nonempty_tx(label: &str) -> Transaction {
        // 测试只关心栈行为；用一个 Cell 变体占位，字段值不影响。
        Transaction::new(
            label,
            vec![EditOp::Cell(CellSnapshot {
                project: "p".into(),
                group: "g".into(),
                node: "t".into(),
                row: 0,
                col: 0,
                before: String::new(),
                after: "x".into(),
                before_rows: 0,
                after_rows: 1,
                before_row_len: 0,
                after_row_len: 1,
            })],
        )
    }

    #[test]
    fn new_transaction_clears_redo() {
        let mut h = EditHistory::default();
        h.push(nonempty_tx("a"));
        let t = h.pop_undo().unwrap();
        h.push_redo(t);
        assert!(h.can_redo());
        h.push(nonempty_tx("b"));
        assert!(!h.can_redo());
        assert_eq!(h.undo_label(), Some("b"));
    }

    #[test]
    fn empty_transaction_is_ignored() {
        let mut h = EditHistory::default();
        h.push(tx("empty"));
        assert!(!h.can_undo());
    }

    #[test]
    fn history_is_bounded() {
        let mut h = EditHistory::default();
        for i in 0..HISTORY_LIMIT + 5 {
            h.push(nonempty_tx(&format!("{i}")));
        }
        assert_eq!(h.undo_len(), HISTORY_LIMIT);
        assert_eq!(h.undo_label(), Some("104"));
    }

    #[test]
    fn truncate_undo_removes_panel_edits_and_redo() {
        let mut h = EditHistory::default();
        h.push(nonempty_tx("before"));
        let mark = h.undo_len();
        h.push(nonempty_tx("inside-panel"));
        let t = h.pop_undo().unwrap();
        h.push_redo(t);
        h.truncate_undo(mark);
        assert_eq!(h.undo_len(), 1);
        assert_eq!(h.undo_label(), Some("before"));
        assert!(!h.can_redo());
    }

    #[test]
    fn clear_drops_both_stacks() {
        let mut h = EditHistory::default();
        h.push(nonempty_tx("a"));
        let t = h.pop_undo().unwrap();
        h.push_redo(t);
        h.clear();
        assert!(!h.can_undo());
        assert!(!h.can_redo());
    }
}
