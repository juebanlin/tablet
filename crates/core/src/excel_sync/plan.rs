//! 决策：两侧 VO + schema → [`GroupPlan`]。**纯函数，不碰文件系统。**
//!
//! 这里是整个同步唯一的「怎么对齐列、什么能做什么不能做」的地方——黑盒就在这一个
//! 模块里。`apply` 只消费这里算好的 [`ColumnMatch`]，**不自己算**（否则又会出现两份
//! 规则，改一份忘一份）。换匹配机制 = 只改这个文件。
//!
//! 决策之所以要物化成数据，是因为面板渲染和按钮行为原本是两条独立推导的路径——
//! 显示什么由 Slint 谓词算、做什么由 Rust 的 action 分支算，靠手工对齐。现在两边
//! 读同一份 [`PairPlan`]。

use std::collections::BTreeMap;

use crate::model::Group;

use super::read::tbl_sheet_vo;
use super::vo::{ColumnVo, RowVo, SheetVo};

// ── 对齐 ──

/// 两侧的列对齐结果。**位置不等于身份**：对齐按列名，落点由它推导。
#[derive(Clone, PartialEq, Debug)]
pub struct ColumnMatch {
    /// 按顺序同名匹配的前缀列数
    pub matched_prefix: usize,
    /// tablet 有、xlsx 没有的列数（在前缀之后）
    pub tablet_only: usize,
    /// xlsx 有、tablet 没有的列数（在前缀之后）
    pub xlsx_only: usize,
}

impl ColumnMatch {
    /// tablet 第 `i` 列（0-based）落到 xlsx 第几列（1-based）。
    ///
    /// 这是**消费**已经算好的映射，不是重新计算对齐——`apply` 调它不算破例。
    /// 匹配前缀内按位置；前缀之外的（tablet 独有列）只在补列 / 强制模式下落笔，
    /// 落在匹配前缀正后面（追加，不插入：这两种模式下 xlsx 侧已经确定没有多余列）。
    pub fn xlsx_col(&self, i: usize, scope: PushScope) -> Option<u32> {
        if i < self.matched_prefix {
            return Some(i as u32 + 1);
        }
        match scope {
            PushScope::MatchedOnly => None,
            PushScope::WithTabletColumns | PushScope::WholeTable => {
                Some(self.matched_prefix as u32 + 1 + (i - self.matched_prefix) as u32)
            }
        }
    }
}

/// 按列名逐位求匹配前缀。
pub fn compute_column_match(tablet: &[ColumnVo], xlsx: &[ColumnVo]) -> ColumnMatch {
    let n = tablet.len();
    let m = xlsx.len();
    let mut matched = 0;
    while matched < n && matched < m && tablet[matched].name == xlsx[matched].name {
        matched += 1;
    }
    ColumnMatch {
        matched_prefix: matched,
        tablet_only: n.saturating_sub(matched),
        xlsx_only: m.saturating_sub(matched),
    }
}

// ── 操作 ──

/// 推送的列范围。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PushScope {
    /// 只覆盖匹配前缀内（数据→）
    MatchedOnly,
    /// 匹配前缀 + 在尾部追加 tablet 独有列（补列→）
    WithTabletColumns,
    /// 整表就地重写，并清掉多余的列与数据行（强制→）
    WholeTable,
}

/// 一次同步操作。**它的内容就是它的身份**——UI 回传的是它，不是它在列表里的下标，
/// 否则文件一变，下标就指向另一个操作（那正是要拆掉的「按行号回查」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyncOp {
    /// tablet → xlsx
    Push { scope: PushScope },
    /// xlsx → tablet（数据 + desc）
    Pull,
    /// xlsx 文件还不存在，新建一份
    CreateFile,
    /// xlsx 独有的 sheet。**今天仍未实现**，见 [`crate::excel_sync::apply`]
    ImportSheet,
}

impl SyncOp {
    /// 面板上的按钮文字。
    pub fn label(&self) -> &'static str {
        match self {
            SyncOp::Push { scope: PushScope::MatchedOnly } => "数据→",
            SyncOp::Push { scope: PushScope::WithTabletColumns } => "补列→",
            SyncOp::Push { scope: PushScope::WholeTable } => "强制→",
            SyncOp::Pull => "←数据",
            SyncOp::CreateFile => "创建→",
            SyncOp::ImportSheet => "←导入",
        }
    }
}

// ── 计划 ──

/// 一侧的形态摘要。**有损**：只装面板要显示的东西。
///
/// 这里**不带差异统计**——「有没有差异」已经编进 `ops` 本身了（没差异就不产出
/// Push/Pull），UI 那边也就没有 `has_diff` 这个字段了。
#[derive(Clone, PartialEq, Debug)]
pub struct SheetSummary {
    pub rows: usize,
    pub cols: usize,
}

impl SheetSummary {
    fn of(vo: &SheetVo) -> Self {
        SheetSummary { rows: vo.rows.len(), cols: vo.columns.len() }
    }
}

/// 一行配对：一个 (group, 节点/sheet)。
#[derive(Clone, Debug)]
pub struct PairPlan {
    pub node: String,
    /// `None` = tablet 没有这个节点
    pub left: Option<SheetSummary>,
    /// `None` = xlsx 没有这张 sheet
    pub right: Option<SheetSummary>,
    /// 列对齐结果。`apply` 靠它把 tablet 列号翻成 xlsx 列号，**不自己算**
    pub col_match: Option<ColumnMatch>,
    /// 这一行允许做哪些操作。**渲染与执行读的是同一个列表**
    pub ops: Vec<SyncOp>,
    /// 有值 = 这一行做不了任何操作，原因是它
    pub blocked_reason: Option<String>,
    /// desc 行与 tablet 有差异（只算匹配前缀内，那正是回读时会落地的那几列）
    pub desc_diff: bool,
    /// type / export 行被改过。这两行按约定策划**不该**动，改了要吭声而不是静默忽略
    pub header_edited: bool,
}

#[derive(Clone, Debug)]
pub struct GroupPlan {
    pub group: String,
    pub pairs: Vec<PairPlan>,
}

/// 算出一整个 group 的同步计划。
///
/// `sheets` 是该 group 对应 xlsx 的投影；文件不存在时传 `None`。
pub fn plan_group(group: &Group, sheets: Option<&[SheetVo]>) -> GroupPlan {
    let none: Vec<SheetVo> = Vec::new();
    let sheets = sheets.unwrap_or(&none);

    let mut pairs: Vec<PairPlan> = Vec::new();

    for table in &group.tables {
        if table.deleted { continue; }
        let left = tbl_sheet_vo(table);
        let right = sheets.iter().find(|s| s.name == left.name);
        pairs.push(pair_of(&left, right));
    }

    // xlsx 独有、group 里没有的 sheet
    for sheet in sheets {
        let known = group.tables.iter().any(|t| !t.deleted && t.name == sheet.name);
        if known { continue; }
        pairs.push(PairPlan {
            node: sheet.name.clone(),
            left: None,
            right: Some(SheetSummary::of(sheet)),
            col_match: None,
            ops: vec![SyncOp::ImportSheet],
            blocked_reason: None,
            desc_diff: false,
            header_edited: false,
        });
    }

    GroupPlan { group: group.name.clone(), pairs }
}

fn pair_of(left: &SheetVo, right: Option<&SheetVo>) -> PairPlan {
    let Some(right) = right else {
        return PairPlan {
            node: left.name.clone(),
            left: Some(SheetSummary::of(left)),
            right: None,
            col_match: None,
            ops: vec![SyncOp::CreateFile],
            blocked_reason: None,
            desc_diff: false,
            header_edited: false,
        };
    };

    let cm = compute_column_match(&left.columns, &right.columns);
    let n = cm.matched_prefix;

    let desc_diff = (0..n).any(|i| {
        let want = left.columns[i].desc.as_deref().unwrap_or("");
        right.columns[i].desc.as_deref().is_some_and(|got| got != want)
    });

    // 越权检测：type / export 是策划不该改的两行。改了要说一声，不能静默忽略——
    // 那和这个仓库已经栽过的几类「默默吃掉改动」同源。
    let header_edited = (0..n).any(|i| {
        let l = &left.columns[i];
        let r = &right.columns[i];
        r.ty.as_deref().is_some_and(|v| Some(v) != l.ty.as_deref())
            || r.export.as_deref().is_some_and(|v| Some(v) != l.export.as_deref())
    });

    let has_diff = rows_differ(&left.rows, &right.rows, n) || desc_diff;

    let (ops, blocked_reason) = match classify(&cm) {
        Some(ops) => (ops, None),
        None => (Vec::new(), Some("表头不符合规范".to_string())),
    };

    PairPlan {
        node: left.name.clone(),
        left: Some(SheetSummary::of(left)),
        right: Some(SheetSummary::of(right)),
        col_match: Some(cm),
        ops: gate_on_diff(ops, has_diff),
        blocked_reason,
        desc_diff,
        header_edited,
    }
}

/// 由列对齐结果决定「本来能做哪些操作」，`None` = 这一行不可操作。
///
/// 注意这里**没有**「首列必须叫 id」那条判断：改名会让 `matched_prefix == 0`，
/// 已经在下面被挡掉了，那条判断只会让首列不叫 id 的表被无端封死。
fn classify(cm: &ColumnMatch) -> Option<Vec<SyncOp>> {
    if cm.matched_prefix == 0 {
        return None;
    }
    Some(if cm.tablet_only == 0 && cm.xlsx_only == 0 {
        vec![
            SyncOp::Push { scope: PushScope::MatchedOnly },
            SyncOp::Pull,
        ]
    } else if cm.tablet_only > 0 && cm.xlsx_only == 0 {
        vec![
            SyncOp::Push { scope: PushScope::MatchedOnly },
            SyncOp::Pull,
            SyncOp::Push { scope: PushScope::WithTabletColumns },
        ]
    } else {
        // 两侧各有独有列，或只有 xlsx 多列（策划的公式列 / 删列残留）：
        // 结构对不上，只能整表重写
        vec![SyncOp::Push { scope: PushScope::WholeTable }]
    })
}

/// 数据与 desc 都没差异时，`Push{MatchedOnly}` / `Pull` 没有意义，撤掉。
///
/// 补列 / 强制 / 创建 / 导入**不**受它约束——那些改的是结构，没有数据差异也照样
/// 是该做的事。
fn gate_on_diff(ops: Vec<SyncOp>, has_diff: bool) -> Vec<SyncOp> {
    if has_diff {
        return ops;
    }
    ops.into_iter()
        .filter(|op| !matches!(op, SyncOp::Pull | SyncOp::Push { scope: PushScope::MatchedOnly }))
        .collect()
}

/// 两侧的数据行有没有差异。只比匹配前缀内的列——那是真正会对齐的部分。
///
/// 按**行号**配对而不是按下标：读取时整行空白会被跳过，所以两侧的行号本来就可能
/// 对不齐，按下标比会冤枉出一堆假差异。
fn rows_differ(left: &[RowVo], right: &[RowVo], cols: usize) -> bool {
    let l: BTreeMap<u32, &Vec<String>> = left.iter().map(|r| (r.index, &r.cells)).collect();
    let r: BTreeMap<u32, &Vec<String>> = right.iter().map(|x| (x.index, &x.cells)).collect();

    if l.len() != r.len() { return true; }
    l.iter().zip(r.iter()).any(|((li, lc), (ri, rc))| {
        li != ri || (0..cols).any(|c| lc.get(c) != rc.get(c))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::excel_sync::testutil::*;
    use crate::excel_sync::vo::{SheetRange, DATA_START};

    /// 「完全匹配」时 xlsx 侧的样子，测试再按需改。
    fn xlsx(cols: &[(&str, &str)], records: &[Vec<&str>]) -> SheetVo {
        let columns = cols.iter().enumerate().map(|(i, (n, d))| ColumnVo {
            index: i as u32 + 1,
            name: Some(n.to_string()),
            desc: Some(d.to_string()),
            // 与 tablet 侧一致才不会被判成「越权改了 type 行」
            ty: Some(FIELD_SPEC.iter().find(|(fn_, _, _)| fn_ == n)
                .map_or_else(|| "int".to_string(), |(_, _, t)| t.to_string())),
            export: Some("前后端".to_string()),
        }).collect();
        let rows = records.iter().enumerate().map(|(i, r)| RowVo {
            index: DATA_START + i as u32,
            cells: r.iter().map(|s| s.to_string()).collect(),
        }).collect();
        SheetVo { name: "HeroBase".into(), columns, rows, range: SheetRange::default() }
    }

    fn matched(records: Vec<Vec<&str>>) -> SheetVo {
        xlsx(&[("id", "编号"), ("name", "名字")], &records)
    }

    /// 算这一行的 ops。`right` 为 None 表示 xlsx 里没有这张 sheet。
    fn ops_of(right: Option<SheetVo>, records: Vec<Vec<&str>>) -> PairPlan {
        let g = group(records);
        let sheets: Vec<SheetVo> = right.into_iter().collect();
        let plan = plan_group(&g, if sheets.is_empty() { None } else { Some(&sheets) });
        plan.pairs.into_iter().next().unwrap()
    }

    #[test]
    fn identical_columns_offer_data_both_ways() {
        let p = ops_of(Some(matched(vec![vec!["1", "carol"], vec!["2", "bob"]])),
                       data());
        assert_eq!(p.ops, vec![
            SyncOp::Push { scope: PushScope::MatchedOnly },
            SyncOp::Pull,
        ]);
        assert!(p.blocked_reason.is_none());
        assert_eq!(p.col_match.as_ref().unwrap().matched_prefix, 2);
    }

    /// 数据与 desc 都没差异、结构也没差异 → 没有任何操作可做。
    /// 「有没有差异」编进 ops 本身，UI 那边就不需要 `has_diff` 这个字段了。
    #[test]
    fn no_difference_offers_nothing() {
        let p = ops_of(Some(matched(data())), data());
        assert!(p.ops.is_empty());
    }

    #[test]
    fn tablet_extra_columns_also_offer_add_columns() {
        let p = ops_of(Some(matched(vec![vec!["1", "carol"], vec!["2", "bob"]])),
                       data());
        assert!(!p.ops.contains(&SyncOp::Push { scope: PushScope::WithTabletColumns }));

        // tablet 三个字段、xlsx 只有两个 → 多出「补列→」
        let g = {
            let mut g = group(data());
            g.tables[0].schema.fields.push(crate::model::FieldDef {
                name: "atk".into(), desc: "攻击".into(),
                tbl_type: "int".into(), export: crate::model::Export::ClientServer,
            });
            g
        };
        let sheets = vec![matched(vec![vec!["1", "carol"], vec!["2", "bob"]])];
        let plan = plan_group(&g, Some(&sheets));
        let p = &plan.pairs[0];
        assert_eq!(p.col_match.as_ref().unwrap().tablet_only, 1);
        assert!(p.ops.contains(&SyncOp::Push { scope: PushScope::WithTabletColumns }));
    }

    #[test]
    fn both_sides_extra_columns_offer_force_only() {
        // tablet: id,name ／ xlsx: id,bonus（各有一个对方没有的）
        let p = ops_of(Some(xlsx(&[("id", "编号"), ("bonus", "额外")],
                                 &[vec!["1", "x"]])), data());
        assert_eq!(p.ops, vec![SyncOp::Push { scope: PushScope::WholeTable }]);
    }

    #[test]
    fn xlsx_extra_columns_offer_force_only() {
        let p = ops_of(Some(xlsx(&[("id", "编号"), ("name", "名字"), ("note", "备注")],
                                 &[vec!["1", "alice", "x"]])), data());
        assert_eq!(p.ops, vec![SyncOp::Push { scope: PushScope::WholeTable }]);
    }

    #[test]
    fn tablet_only_node_offers_create() {
        let p = ops_of(None, data());
        assert_eq!(p.ops, vec![SyncOp::CreateFile]);
        assert!(p.right.is_none());
        assert!(p.left.is_some());
    }

    #[test]
    fn xlsx_only_sheet_offers_import() {
        let g = group(data());
        let sheets = vec![SheetVo {
            name: "策划备注".into(),
            columns: vec![ColumnVo {
                index: 1, name: Some("a".into()), desc: None, ty: None, export: None,
            }],
            rows: vec![],
            range: SheetRange::default(),
        }];
        let plan = plan_group(&g, Some(&sheets));
        let p = plan.pairs.iter().find(|p| p.node == "策划备注").unwrap();
        assert_eq!(p.ops, vec![SyncOp::ImportSheet]);
        assert!(p.left.is_none());
    }

    /// 首列名对不上（被改坏）→ 一个操作都不给，只能看到原因。
    #[test]
    fn mismatched_header_is_blocked() {
        let p = ops_of(Some(xlsx(&[("编号", "编号"), ("name", "名字")], &[vec!["1", "alice"]])),
                       data());
        assert!(p.ops.is_empty());
        assert_eq!(p.blocked_reason.as_deref(), Some("表头不符合规范"));
    }

    /// 只改了 desc：数据一行没动，也要给得出同步按钮——否则这个特性等于不存在。
    #[test]
    fn desc_only_change_still_offers_sync() {
        let mut right = matched(data());
        right.columns[1].desc = Some("角色名".into());
        let p = ops_of(Some(right), data());
        assert!(p.desc_diff);
        assert!(p.ops.contains(&SyncOp::Pull));
        assert!(p.ops.contains(&SyncOp::Push { scope: PushScope::MatchedOnly }));
    }

    /// 旧布局没有 desc 行 → 不算 desc 差异（回读时也不会动 tablet 的 desc）。
    #[test]
    fn legacy_sheet_has_no_desc_diff() {
        let mut right = matched(data());
        for c in &mut right.columns { c.desc = None; }
        let p = ops_of(Some(right), data());
        assert!(!p.desc_diff);
        assert!(p.ops.is_empty(), "数据也一样，没什么可同步的");
    }

    /// type / export 是策划**不该动**的两行。他改了要吭声，不能静默忽略。
    #[test]
    fn edited_type_or_export_row_is_reported() {
        let mut right = matched(data());
        right.columns[0].ty = Some("float".into());
        let p = ops_of(Some(right), data());
        assert!(p.header_edited);

        let mut right = matched(data());
        right.columns[1].export = Some("客户端".into());
        let p = ops_of(Some(right), data());
        assert!(p.header_edited);

        let p = ops_of(Some(matched(data())), data());
        assert!(!p.header_edited, "没改过就不能乱报");
    }

    /// 列映射是算一次、放在 plan 里给 apply 消费的。这里把它的落点钉住：
    /// 匹配前缀内按位置，前缀之外追加在正后面。
    #[test]
    fn xlsx_col_maps_prefix_and_appended_columns() {
        let cm = ColumnMatch { matched_prefix: 2, tablet_only: 1, xlsx_only: 0 };
        assert_eq!(cm.xlsx_col(0, PushScope::MatchedOnly), Some(1));
        assert_eq!(cm.xlsx_col(1, PushScope::MatchedOnly), Some(2));
        assert_eq!(cm.xlsx_col(2, PushScope::MatchedOnly), None, "MatchedOnly 不碰前缀之外");
        assert_eq!(cm.xlsx_col(2, PushScope::WithTabletColumns), Some(3));
    }
}
