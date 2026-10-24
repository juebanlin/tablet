//! 校验规则面板（@01.10.7）：列优先 + 「填表 → 添加」。
//!
//! 左侧列清单；右侧上半是本列规则（点一行载入下方表单编辑），下半是表单：
//! 选类型 → 参数随之出现 → 点「添加 / 保存」才写进节点。表单里改的是草稿，
//! 不会在每次点击时落到节点上；「取消」用打开时的快照还原。

use std::cell::RefCell;
use std::rc::Rc;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use tablet_core::model::{builtin_rules, CompareOp, NodeMode, Rule, RuleKind, RuleSource, Severity};
use tablet_core::ops::format_validation_log;
use tablet_core::validate::{
    rule_kind_applies, rule_params_valid, validate_enum, validate_table, RefIndex, RuleIndex,
    ValidationError, ValueClass,
};

use crate::state::{AppState, SelectedNode};
use crate::{AppWindow, RuleCard, RuleColumnRow, RuleIssueRow};

const DESC_UNIQUE: &str =
    "本列的非空值在节点内不能重复；整数列按数值比较（01 与 1 视为相同）。";
const DESC_REF_EXISTS: &str =
    "本列每个非空值都必须出现在目标节点的目标列中；目标列必须是标量。同表不同列可以自引用。";
const DESC_NON_EMPTY: &str = "该列必须有值；整行全空的占位行不算缺填。";
const DESC_RANGE: &str = "数值必须落在区间内；留空一侧表示不限。";
const DESC_LEN: &str = "按字符数限制长度；留空一侧表示不限。";
const DESC_ONE_OF: &str = "值必须是候选之一；多个候选用逗号分隔。";
const DESC_MATCHES: &str = "整串匹配请写 ^...$；否则是按「包含」匹配。";
const DESC_REQUIRED_IF: &str = "当条件列等于给定值时，本列必须有值。";
const DESC_COMPARE: &str = "本列与比较列按运算符比较（两边都必须能解析为数字）。";
const DESC_UNSUPPORTED: &str =
    "该列是复合类型（List / Set / Map / Tuple 等），当前版本不参与校验规则。";
const HINT_DUPLICATE: &str =
    "该列已有等价规则（同类型、同目标），不能重复添加。";

/// 内置验证器目录：(key, 显示名)。
const VALIDATORS: &[(&str, &str)] = &[
    ("unique", "unique()"),
    ("ref_exists", "ref_exists(节点.列)"),
    ("non_empty", "non_empty()"),
    ("range", "range(下限, 上限)"),
    ("len", "len(下限, 上限)"),
    ("one_of", "one_of(候选值)"),
    ("matches", "matches(正则)"),
    ("required_if", "required_if(条件列, 值)"),
    ("compare", "compare(比较列, 运算符)"),
];

const OP_LABELS: &[&str] = &["<", "<=", ">", ">=", "==", "!="];

fn kind_key(kind: &RuleKind) -> &'static str {
    match kind {
        RuleKind::Unique => "unique",
        RuleKind::RefExists { .. } => "ref_exists",
        RuleKind::NonEmpty => "non_empty",
        RuleKind::Range { .. } => "range",
        RuleKind::Len { .. } => "len",
        RuleKind::OneOf { .. } => "one_of",
        RuleKind::Matches { .. } => "matches",
        RuleKind::RequiredIf { .. } => "required_if",
        RuleKind::Compare { .. } => "compare",
        // 内置只读种类：不会出现在用户规则里，给个稳定 key 只为穷尽匹配
        RuleKind::PositiveInt => "positive_int",
        RuleKind::UpperSnake => "upper_snake",
    }
}

fn key_of_label(label: &str) -> Option<&'static str> {
    VALIDATORS.iter().find(|(_, l)| *l == label).map(|(k, _)| *k)
}

/// 用来判断「这种类型能不能挂在这列上」的探针。
fn probe_kind(key: &str) -> RuleKind {
    match key {
        "unique" => RuleKind::Unique,
        "ref_exists" => RuleKind::RefExists {
            target_node: "X".into(), target_column: "x".into(),
        },
        "non_empty" => RuleKind::NonEmpty,
        "range" => RuleKind::Range { min: "0".into(), max: "1".into() },
        "len" => RuleKind::Len { min: "0".into(), max: "1".into() },
        "one_of" => RuleKind::OneOf { values: vec!["a".into()] },
        "matches" => RuleKind::Matches { pattern: "^a$".into() },
        "required_if" => RuleKind::RequiredIf { column: "X".into(), value: "1".into() },
        _ => RuleKind::Compare { column: "X".into(), op: CompareOp::Le },
    }
}

pub struct RulesPanelState {
    pub open: bool,
    pub group: String,
    pub name: String,
    /// 打开时的规则快照，「取消」用它还原。
    pub original: Vec<Rule>,
    /// 当前选中的列。
    pub column: Option<String>,
    /// 正在表单里编辑的规则（`node.rules` 的全局下标）；None = 新增模式。
    pub editing: Option<usize>,
    /// 表单草稿。只有「添加 / 保存」时才写回节点。
    pub draft: Rule,
}

impl RulesPanelState {
    pub fn new() -> Self {
        Self {
            open: false,
            group: String::new(),
            name: String::new(),
            original: Vec::new(),
            column: None,
            editing: None,
            draft: Rule { column: String::new(), kind: RuleKind::Unique, level: Severity::Error , source: RuleSource::User },
        }
    }
}

// ── 读取 ────────────────────────────────────────────────────────────────

fn node_mode(st: &AppState, group: &str, name: &str) -> Option<NodeMode> {
    st.engine.node_rules(group, name).map(|(m, _)| m)
}

fn node_rules(st: &AppState, group: &str, name: &str) -> Vec<Rule> {
    st.engine.node_rules(group, name).map(|(_, r)| r).unwrap_or_default()
}

/// 单节点全量校验（含规则产生的错误/警告）。
fn node_issues(st: &AppState, group: &str, name: &str) -> Vec<ValidationError> {
    let project = st.engine.project();
    let sep = &project.schema.separators;
    let refs = RefIndex::build(&project.groups);
    let cidx = RuleIndex::build(&project.groups);
    if let Some(t) = st.engine.find_table(group, name) {
        return validate_table(t, sep, Some(&refs), Some(&cidx));
    }
    if let Some(e) = st.engine.find_enum(group, name) {
        return validate_enum(e, Some(&cidx));
    }
    Vec::new()
}

fn column_rule_indices(rules: &[Rule], column: &str) -> Vec<usize> {
    rules.iter().enumerate()
        .filter(|(_, r)| r.column == column)
        .map(|(i, _)| i)
        .collect()
}

fn class_label(class: Option<ValueClass>) -> &'static str {
    match class {
        Some(ValueClass::Int) => "整数",
        Some(ValueClass::Float) => "浮点",
        Some(ValueClass::Text) => "文本",
        None => "不支持规则",
    }
}

fn level_label(level: Severity) -> &'static str {
    match level {
        Severity::Error => "错误",
        Severity::Warning => "警告",
        Severity::Info => "信息",
    }
}

fn level_index(level: Severity) -> i32 {
    match level {
        Severity::Error => 0,
        Severity::Warning => 1,
        Severity::Info => 2,
    }
}

/// 目标节点候选：项目内 Table / Enum，且至少有一列支持规则。
fn target_nodes(st: &AppState) -> Vec<String> {
    st.engine.rule_target_nodes().into_iter()
        .filter(|n| st.engine.node_column_classes_by_name(n).iter().any(|(_, c)| c.is_some()))
        .collect()
}

/// 目标列候选：只要标量列。
fn target_columns(st: &AppState, node: &str) -> Vec<String> {
    st.engine.node_column_classes_by_name(node).into_iter()
        .filter(|(_, c)| c.is_some())
        .map(|(n, _)| n)
        .collect()
}

/// 列表里第 i 行（含系统内置只读行）对应的用户规则全局下标。
fn user_global_at(st: &AppState, i: usize) -> Option<usize> {
    let group = &st.rules_panel.group;
    let name = &st.rules_panel.name;
    let column = st.rules_panel.column.clone().unwrap_or_default();
    let mode = node_mode(st, group, name);
    let builtin = mode.map(|m| builtin_rules(m).iter().filter(|r| r.column == column).count())
        .unwrap_or(0);
    if i < builtin { return None; }
    let rules = node_rules(st, group, name);
    column_rule_indices(&rules, &column).get(i - builtin).copied()
}

fn index_of(list: &[String], value: &str) -> i32 {
    list.iter().position(|x| x == value).map(|i| i as i32).unwrap_or(0)
}

/// 本节点的其它标量列（required_if / compare 用，排除本列）。
fn other_columns(specs: &[(String, Option<ValueClass>)], current: &str) -> Vec<String> {
    specs.iter()
        .filter(|(n, c)| c.is_some() && n != current)
        .map(|(n, _)| n.clone())
        .collect()
}

/// 按 key 造一条带默认参数的规则。
fn default_kind(
    key: &str,
    st: &AppState,
    column: &str,
) -> RuleKind {
    match key {
        "unique" => RuleKind::Unique,
        "ref_exists" => {
            let targets = target_nodes(st);
            let node = targets.first().cloned().unwrap_or_default();
            let col = target_columns(st, &node).into_iter()
                .find(|c| !(node == st.rules_panel.name && c == column))
                .unwrap_or_default();
            RuleKind::RefExists { target_node: node, target_column: col }
        }
        "non_empty" => RuleKind::NonEmpty,
        "range" => RuleKind::Range { min: "0".into(), max: String::new() },
        "len" => RuleKind::Len { min: "1".into(), max: String::new() },
        "one_of" => RuleKind::OneOf { values: Vec::new() },
        "matches" => RuleKind::Matches { pattern: String::new() },
        "required_if" => {
            let specs = st.engine.node_column_classes(&st.rules_panel.group, &st.rules_panel.name);
            let col = other_columns(&specs, column).into_iter().next().unwrap_or_default();
            RuleKind::RequiredIf { column: col, value: String::new() }
        }
        _ => {
            let specs = st.engine.node_column_classes(&st.rules_panel.group, &st.rules_panel.name);
            let col = other_columns(&specs, column).into_iter().next().unwrap_or_default();
            RuleKind::Compare { column: col, op: CompareOp::Le }
        }
    }
}

/// 新草稿的默认形状：系统已保证唯一的列不用 unique()，直接给 ref_exists。
fn default_rule(state: &AppState, column: &str) -> Rule {
    let mode = node_mode(state, &state.rules_panel.group, &state.rules_panel.name);
    let unique_redundant = mode.map_or(false, |m| Rule {
        column: column.to_string(), kind: RuleKind::Unique, level: Severity::Error,

        source: RuleSource::User,
    }.conflicts_with_builtin(m));
    let kind = if unique_redundant {
        default_kind("ref_exists", state, column)
    } else {
        RuleKind::Unique
    };
    Rule { column: column.to_string(), kind, level: Severity::Error , source: RuleSource::User }
}

/// 草稿是否与已有规则等价（编辑时排除自己）。
fn is_duplicate(rules: &[Rule], editing: Option<usize>, draft: &Rule) -> bool {
    rules.iter().enumerate().any(|(i, r)| {
        Some(i) != editing && r.column == draft.column && r.kind == draft.kind
    })
}

/// 表单能否提交，以及要显示的提示。
fn form_status(st: &AppState, rules: &[Rule]) -> (bool, String) {
    let draft = &st.rules_panel.draft;
    let specs = st.engine.node_column_classes(&st.rules_panel.group, &st.rules_panel.name);
    let class = specs.iter().find(|(n, _)| n == &draft.column).and_then(|(_, c)| *c);
    let Some(class) = class else {
        return (false, DESC_UNSUPPORTED.to_string());
    };
    if !rule_kind_applies(&draft.kind, class) {
        return (false, format!(
            "{} 不适用于该列的类型（{}）", draft.expr_string(), class_label(Some(class))
        ));
    }
    if let RuleKind::RefExists { target_node, target_column } = &draft.kind {
        if target_node == &st.rules_panel.name && target_column == &draft.column {
            return (false, "目标不能是本列自己（恒成立，没有意义）".to_string());
        }
    }
    if let Some(mode) = node_mode(st, &st.rules_panel.group, &st.rules_panel.name) {
        if draft.conflicts_with_builtin(mode) {
            return (false,
                "该列已由系统保证唯一，unique() 不会生效；请改为其它规则或删除。".to_string());
        }
    }
    if !rule_params_valid(&draft.kind) {
        return (false, "参数不完整或不合法（区间两端不能都空、正则不能为空）。".to_string());
    }
    if is_duplicate(rules, st.rules_panel.editing, draft) {
        return (false, HINT_DUPLICATE.to_string());
    }
    (true, String::new())
}

// ── 打开 / 推送 ─────────────────────────────────────────────────────────

pub fn open_for_selected(state: &Rc<RefCell<AppState>>) {
    let target = {
        let s = state.borrow();
        match &s.selected {
            Some(SelectedNode::Table { group, name, .. })
            | Some(SelectedNode::Enum { group, name, .. }) => Some((group.clone(), name.clone())),
            _ => None,
        }
    };
    if let Some((group, name)) = target {
        open_for_node(state, &group, &name);
    }
}

pub fn open_for_node(state: &Rc<RefCell<AppState>>, group: &str, name: &str) {
    let mut st = state.borrow_mut();
    let Some((_, rules)) = st.engine.node_rules(group, name) else { return };
    let specs = st.engine.node_column_classes(group, name);
    let column = specs.iter().find(|(n, _)| rules.iter().any(|r| &r.column == n))
        .or_else(|| specs.iter().find(|(_, c)| c.is_some()))
        .or_else(|| specs.first())
        .map(|(n, _)| n.clone());
    st.rules_panel.open = true;
    st.rules_panel.group = group.to_string();
    st.rules_panel.name = name.to_string();
    st.rules_panel.original = rules;
    st.rules_panel.column = column.clone();
    st.rules_panel.editing = None;
    st.rules_panel.draft = match &column {
        Some(c) => default_rule(&st, c),
        None => Rule { column: String::new(), kind: RuleKind::Unique, level: Severity::Error , source: RuleSource::User },
    };
}

pub fn push(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let st = state.borrow();
    ui_h.set_dlg_rules_open(st.rules_panel.open);
    if !st.rules_panel.open { return; }

    let group = st.rules_panel.group.clone();
    let name = st.rules_panel.name.clone();
    let mode = node_mode(&st, &group, &name);
    let rules = node_rules(&st, &group, &name);
    let issues = node_issues(&st, &group, &name);
    let specs = st.engine.node_column_classes(&group, &name);
    let selected_column = st.rules_panel.column.clone().unwrap_or_default();
    let draft = st.rules_panel.draft.clone();
    let draft_key = kind_key(&draft.kind);

    let mode_label = match mode {
        Some(NodeMode::Table) => "Table",
        Some(NodeMode::Enum) => "Enum",
        _ => "",
    };
    ui_h.set_ru_node_label(SharedString::from(format!("{}/{}  ({})", group, name, mode_label)));

    // ── 左：列清单 ──
    let columns: Vec<RuleColumnRow> = specs.iter().map(|(col, class)| {
        let count = rules.iter().filter(|r| &r.column == col).count();
        let system_unique = mode.map_or(false, |m| Rule {
            column: col.clone(), kind: RuleKind::Unique, level: Severity::Error,

            source: RuleSource::User,
        }.conflicts_with_builtin(m));
        let badge = if class.is_none() {
            "不支持规则".to_string()
        } else if count > 0 {
            format!("{} 条", count)
        } else if system_unique {
            "系统唯一".to_string()
        } else {
            String::new()
        };
        RuleColumnRow {
            name: SharedString::from(col.clone()),
            badge: SharedString::from(badge),
            unsupported: class.is_none(),
            selected: &selected_column == col,
        }
    }).collect();
    ui_h.set_ru_columns(ModelRc::new(VecModel::from(columns)));

    let class = specs.iter().find(|(c, _)| c == &selected_column).and_then(|(_, c)| *c);
    ui_h.set_ru_column_title(SharedString::from(format!(
        "列 \"{}\"  ·  {}", selected_column, class_label(class)
    )));

    // ── 右：本列规则（系统内置只读行在前，用户规则在后）──
    let indices = column_rule_indices(&rules, &selected_column);
    let mode_rules = mode.map(builtin_rules).unwrap_or_default();
    let mut cards: Vec<RuleCard> = Vec::new();
    for b in mode_rules.iter().filter(|r| r.column == selected_column) {
        cards.push(RuleCard {
            num: SharedString::from("·"),
            title: SharedString::from(b.expr_string()),
            level: SharedString::from("系统"),
            is_warning: false,
            is_info: false,
            selected: false,
            is_builtin: true,
            can_up: false,
            can_down: false,
        });
    }
    for (pos, gi) in indices.iter().enumerate() {
        let r = &rules[*gi];
        cards.push(RuleCard {
            num: SharedString::from((pos + 1).to_string()),
            title: SharedString::from(r.expr_string()),
            level: SharedString::from(level_label(r.level)),
            is_warning: r.level == Severity::Warning,
            is_info: r.level == Severity::Info,
            selected: st.rules_panel.editing == Some(*gi),
            is_builtin: false,
            can_up: pos > 0,
            can_down: pos + 1 < indices.len(),
        });
    }
    ui_h.set_ru_rules(ModelRc::new(VecModel::from(cards)));

    // ── 结果区 ──
    let issue_rows: Vec<RuleIssueRow> = issues.iter().map(|e| RuleIssueRow {
        severity: SharedString::from(match e.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        }),
        text: SharedString::from(format_validation_log(&group, &name, e)),
    }).collect();
    ui_h.set_ru_issues(ModelRc::new(VecModel::from(issue_rows)));
    ui_h.set_ru_error_count(issues.iter().filter(|e| e.severity == Severity::Error).count() as i32);
    ui_h.set_ru_warning_count(issues.iter().filter(|e| e.severity == Severity::Warning).count() as i32);
    ui_h.set_ru_info_count(issues.iter().filter(|e| e.severity == Severity::Info).count() as i32);

    // ── 表单：验证器目录 ──
    let mut options: Vec<(&'static str, &'static str)> = VALIDATORS.iter()
        .filter(|(k, _)| {
            let applies = class.map_or(false, |c| rule_kind_applies(&probe_kind(k), c));
            // 系统已保证的约束不再出现在下拉里（例如 id 列不提供 unique / non_empty）
            let redundant = mode.map_or(false, |m| Rule {
                column: selected_column.clone(),
                kind: probe_kind(k),
                level: Severity::Error,
                source: RuleSource::User,
            }.conflicts_with_builtin(m));
            (applies && !redundant) || *k == draft_key
        })
        .cloned()
        .collect();
    if options.is_empty() {
        options = VALIDATORS.to_vec();
    }
    let kind_idx = options.iter().position(|(k, _)| *k == draft_key).unwrap_or(0) as i32;

    let mut has_target = false;
    let mut target_node_idx = 0i32;
    let mut target_col_idx = 0i32;
    let mut target_cols: Vec<String> = Vec::new();
    let mut param_a_label = String::new();
    let mut param_a = String::new();
    let mut param_b_label = String::new();
    let mut param_b = String::new();
    let mut param_column_label = String::new();
    let mut param_columns: Vec<String> = Vec::new();
    let mut param_column_idx = 0i32;
    let mut param_op_options: Vec<String> = Vec::new();
    let mut param_op_idx = 0i32;
    let mut desc = if class.is_none() { DESC_UNSUPPORTED } else { "" }.to_string();

    match &draft.kind {
        RuleKind::Unique => { if class.is_some() { desc = DESC_UNIQUE.to_string(); } }
        RuleKind::RefExists { target_node, target_column } => {
            has_target = true;
            let targets = target_nodes(&st);
            target_node_idx = index_of(&targets, target_node);
            target_cols = target_columns(&st, target_node);
            // 自己引用自己（同节点同列）没有意义，从候选里剔掉；
            // 但当前值若是它，保留在列表里以免下拉错位（下面会禁用保存）。
            let self_pair = target_node == &name && target_column == &draft.column;
            target_cols.retain(|c| !(target_node == &name && c == &draft.column));
            if self_pair { target_cols.insert(0, draft.column.clone()); }
            target_col_idx = index_of(&target_cols, target_column);
            if class.is_some() { desc = DESC_REF_EXISTS.to_string(); }
        }
        RuleKind::NonEmpty => { if class.is_some() { desc = DESC_NON_EMPTY.to_string(); } }
        RuleKind::Range { min, max } => {
            param_a_label = "下限".into();
            param_a = min.clone();
            param_b_label = "上限".into();
            param_b = max.clone();
            if class.is_some() { desc = DESC_RANGE.to_string(); }
        }
        RuleKind::Len { min, max } => {
            param_a_label = "下限".into();
            param_a = min.clone();
            param_b_label = "上限".into();
            param_b = max.clone();
            if class.is_some() { desc = DESC_LEN.to_string(); }
        }
        RuleKind::OneOf { values } => {
            param_a_label = "候选值（逗号分隔）".into();
            param_a = values.join(", ");
            if class.is_some() { desc = DESC_ONE_OF.to_string(); }
        }
        RuleKind::Matches { pattern } => {
            param_a_label = "正则".into();
            param_a = pattern.clone();
            if class.is_some() { desc = DESC_MATCHES.to_string(); }
        }
        RuleKind::RequiredIf { column, value } => {
            param_column_label = "条件列".into();
            param_columns = other_columns(&specs, &draft.column);
            param_column_idx = index_of(&param_columns, column);
            param_b_label = "等于".into();
            param_b = value.clone();
            if class.is_some() { desc = DESC_REQUIRED_IF.to_string(); }
        }
        RuleKind::Compare { column, op } => {
            param_column_label = "比较列".into();
            param_columns = other_columns(&specs, &draft.column);
            param_column_idx = index_of(&param_columns, column);
            param_op_options = OP_LABELS.iter().map(|s| s.to_string()).collect();
            param_op_idx = OP_LABELS.iter().position(|s| *s == op.as_str()).unwrap_or(0) as i32;
            if class.is_some() { desc = DESC_COMPARE.to_string(); }
        }
        // 内置只读种类不会成为草稿；给个说明即可。
        RuleKind::PositiveInt | RuleKind::UpperSnake => {
            desc = "系统内置规则，不可编辑。".to_string();
        }
    }

    let (can_apply, hint) = form_status(&st, &rules);

    let editing_pos = st.rules_panel.editing
        .and_then(|gi| indices.iter().position(|x| *x == gi));
    let form_title = match editing_pos {
        Some(p) => format!("编辑规则 #{}", p + 1),
        None => format!("新增规则（列 \"{}\"）", selected_column),
    };

    ui_h.set_ru_form_title(SharedString::from(form_title));
    ui_h.set_ru_form_kind_options(ModelRc::new(VecModel::from(
        options.iter().map(|(_, l)| SharedString::from((*l).to_string())).collect::<Vec<_>>()
    )));
    ui_h.set_ru_form_kind_index(kind_idx);
    ui_h.set_ru_form_has_target(has_target);
    let targets = target_nodes(&st);
    ui_h.set_ru_form_target_node_options(ModelRc::new(VecModel::from(
        targets.iter().map(|s| SharedString::from(s.clone())).collect::<Vec<_>>()
    )));
    ui_h.set_ru_form_target_node_index(target_node_idx);
    ui_h.set_ru_form_target_column_options(ModelRc::new(VecModel::from(
        target_cols.iter().map(|s| SharedString::from(s.clone())).collect::<Vec<_>>()
    )));
    ui_h.set_ru_form_target_column_index(target_col_idx);
    ui_h.set_ru_form_level_index(level_index(draft.level));
    ui_h.set_ru_form_desc(SharedString::from(desc));
    ui_h.set_ru_form_hint(SharedString::from(hint));
    ui_h.set_ru_form_can_apply(can_apply);
    ui_h.set_ru_form_editing(editing_pos.is_some());
    ui_h.set_ru_form_apply_label(SharedString::from(
        if editing_pos.is_some() { "保存" } else { "添加" }
    ));
    ui_h.set_ru_can_add(class.is_some());

    ui_h.set_ru_form_param_a_label(SharedString::from(param_a_label));
    ui_h.set_ru_form_param_a(SharedString::from(param_a));
    ui_h.set_ru_form_param_b_label(SharedString::from(param_b_label));
    ui_h.set_ru_form_param_b(SharedString::from(param_b));
    ui_h.set_ru_form_param_column_label(SharedString::from(param_column_label));
    ui_h.set_ru_form_param_column_options(ModelRc::new(VecModel::from(
        param_columns.iter().map(|s| SharedString::from(s.clone())).collect::<Vec<_>>()
    )));
    ui_h.set_ru_form_param_column_index(param_column_idx);
    ui_h.set_ru_form_param_op_options(ModelRc::new(VecModel::from(
        param_op_options.iter().map(|s| SharedString::from(s.clone())).collect::<Vec<_>>()
    )));
    ui_h.set_ru_form_param_op_index(param_op_idx);
    ui_h.set_ru_can_confirm(true);
}

/// 只刷新「能否提交 + 提示」，不重建模型（文本框输入时用，避免光标跳动）。
fn refresh_form_flags(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let st = state.borrow();
    let group = st.rules_panel.group.clone();
    let name = st.rules_panel.name.clone();
    let rules = node_rules(&st, &group, &name);
    let (can_apply, hint) = form_status(&st, &rules);
    ui_h.set_ru_form_can_apply(can_apply);
    ui_h.set_ru_form_hint(SharedString::from(hint));
}

// ── 写回 ────────────────────────────────────────────────────────────────

fn commit_rules(state: &Rc<RefCell<AppState>>, rules: Vec<Rule>) {
    let mut st = state.borrow_mut();
    let group = st.rules_panel.group.clone();
    let name = st.rules_panel.name.clone();
    if st.engine.set_node_rules(&group, &name, rules) && st.realtime_validate {
        st.engine.revalidate(&group, &name);
    }
}

fn edit_rules<F: FnOnce(&mut Vec<Rule>)>(state: &Rc<RefCell<AppState>>, f: F) {
    let (group, name) = {
        let st = state.borrow();
        (st.rules_panel.group.clone(), st.rules_panel.name.clone())
    };
    let mut rules = {
        let st = state.borrow();
        node_rules(&st, &group, &name)
    };
    f(&mut rules);
    commit_rules(state, rules);
}

fn reset_form_to_add(state: &Rc<RefCell<AppState>>) {
    let mut st = state.borrow_mut();
    st.rules_panel.editing = None;
    st.rules_panel.draft = match st.rules_panel.column.clone() {
        Some(c) => default_rule(&st, &c),
        None => Rule { column: String::new(), kind: RuleKind::Unique, level: Severity::Error , source: RuleSource::User },
    };
}

/// 把逗号分隔的候选值拆开（保持用户输入里的引号内容原样）。
fn split_values(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| {
            let t = s.trim();
            let t = t.strip_prefix('"').and_then(|x| x.strip_suffix('"')).unwrap_or(t);
            t.to_string()
        })
        .filter(|s| !s.is_empty())
        .collect()
}

// ── 回调注册 ────────────────────────────────────────────────────────────

pub fn wire(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    // 选列
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_select_column(move |i| {
            let specs = {
                let st = s.borrow();
                st.engine.node_column_classes(&st.rules_panel.group, &st.rules_panel.name)
            };
            let Some((col, _)) = specs.get(i as usize) else { return };
            s.borrow_mut().rules_panel.column = Some(col.clone());
            reset_form_to_add(&s);
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }

    // 点一行规则 → 载入表单
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_select_rule(move |i| {
            let (Some(gi), rules) = ({
                let st = s.borrow();
                let gi = if i < 0 { None } else { user_global_at(&st, i as usize) };
                (gi, st.engine.node_rules(&st.rules_panel.group, &st.rules_panel.name)
                    .map(|(_, r)| r).unwrap_or_default())
            }) else { return };
            let rule = match rules.get(gi) { Some(r) => r.clone(), None => return };
            {
                let mut st = s.borrow_mut();
                st.rules_panel.editing = Some(gi);
                st.rules_panel.draft = rule;
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }

    // 删除（i = 列内序号）
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_delete_rule(move |i| {
            let Some(gi) = (if i < 0 { None } else { user_global_at(&s.borrow(), i as usize) })
                else { return };
            edit_rules(&s, |rules| { if gi < rules.len() { rules.remove(gi); } });
            if s.borrow().rules_panel.editing == Some(gi) {
                reset_form_to_add(&s);
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }

    // 上移 / 下移
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_move_up(move |i| move_rule(&s, &weak, i, true));
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_move_down(move |i| move_rule(&s, &weak, i, false));
    }

    // 表单：改类型
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_set_kind(move |label| {
            let Some(key) = key_of_label(label.as_str()) else { return };
            let column = match s.borrow().rules_panel.column.clone() { Some(c) => c, None => return };
            let kind = {
                let st = s.borrow();
                default_kind(key, &st, &column)
            };
            {
                let mut st = s.borrow_mut();
                st.rules_panel.draft.kind = kind;
                st.rules_panel.draft.column = column;
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }

    // 表单：目标节点（目标列自动落到新节点第一张标量列）
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_set_target_node(move |picked| {
            let (current_name, current_col) = {
                let st = s.borrow();
                (st.rules_panel.name.clone(), st.rules_panel.draft.column.clone())
            };
            let first_col = target_columns(&s.borrow(), picked.as_str())
                .into_iter()
                .find(|c| !(picked.as_str() == current_name && c == &current_col))
                .unwrap_or_default();
            {
                let mut st = s.borrow_mut();
                if let RuleKind::RefExists { target_node, target_column } = &mut st.rules_panel.draft.kind {
                    *target_node = picked.to_string();
                    *target_column = first_col;
                }
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }

    // 表单：目标列
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_set_target_column(move |picked| {
            {
                let mut st = s.borrow_mut();
                if let RuleKind::RefExists { target_column, .. } = &mut st.rules_panel.draft.kind {
                    *target_column = picked.to_string();
                }
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }

    // 表单：级别
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_set_level(move |picked| {
            let level = match picked.as_str() {
                "警告" => Severity::Warning,
                "信息" => Severity::Info,
                _ => Severity::Error,
            };
            s.borrow_mut().rules_panel.draft.level = level;
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }

    // 表单：通用参数（文本框）
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_set_param_a(move |v| {
            {
                let mut st = s.borrow_mut();
                match &mut st.rules_panel.draft.kind {
                    RuleKind::Range { min, .. } | RuleKind::Len { min, .. } => *min = v.to_string(),
                    RuleKind::OneOf { values } => *values = split_values(&v),
                    RuleKind::Matches { pattern } => *pattern = v.to_string(),
                    _ => {}
                }
            }
            if let Some(ui) = weak.upgrade() { refresh_form_flags(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_set_param_b(move |v| {
            {
                let mut st = s.borrow_mut();
                match &mut st.rules_panel.draft.kind {
                    RuleKind::Range { max, .. } | RuleKind::Len { max, .. } => *max = v.to_string(),
                    RuleKind::RequiredIf { value, .. } => *value = v.to_string(),
                    _ => {}
                }
            }
            if let Some(ui) = weak.upgrade() { refresh_form_flags(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_set_param_column(move |picked| {
            {
                let mut st = s.borrow_mut();
                match &mut st.rules_panel.draft.kind {
                    RuleKind::RequiredIf { column, .. } | RuleKind::Compare { column, .. } =>
                        *column = picked.to_string(),
                    _ => {}
                }
            }
            if let Some(ui) = weak.upgrade() { refresh_form_flags(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_set_param_op(move |picked| {
            let Some(op) = CompareOp::parse(&picked) else { return };
            {
                let mut st = s.borrow_mut();
                if let RuleKind::Compare { op: slot, .. } = &mut st.rules_panel.draft.kind {
                    *slot = op;
                }
            }
            if let Some(ui) = weak.upgrade() { refresh_form_flags(&ui, &s); }
        });
    }

    // 添加 / 保存
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_apply(move || {
            let (column, editing, draft) = {
                let st = s.borrow();
                (st.rules_panel.column.clone().unwrap_or_default(),
                 st.rules_panel.editing,
                 st.rules_panel.draft.clone())
            };
            let mut draft = draft;
            draft.column = column;
            match editing {
                Some(gi) => edit_rules(&s, |rules| {
                    if let Some(r) = rules.get_mut(gi) { *r = draft; }
                }),
                None => edit_rules(&s, |rules| { rules.push(draft); }),
            }
            reset_form_to_add(&s);
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }

    // 取消编辑
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_cancel_edit(move || {
            reset_form_to_add(&s);
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }

    // 确定
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_confirm(move || {
            let (group, name) = {
                let st = s.borrow();
                (st.rules_panel.group.clone(), st.rules_panel.name.clone())
            };
            {
                let mut st = s.borrow_mut();
                st.engine.revalidate(&group, &name);
                st.rules_panel.open = false;
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }

    // 取消：还原快照
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_cancel(move || {
            let (group, name, original) = {
                let st = s.borrow();
                (st.rules_panel.group.clone(), st.rules_panel.name.clone(), st.rules_panel.original.clone())
            };
            {
                let mut st = s.borrow_mut();
                st.engine.set_node_rules(&group, &name, original);
                st.engine.revalidate(&group, &name);
                st.rules_panel.open = false;
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }
}

/// 与同列相邻规则交换位置（i = 列内序号）。
fn move_rule(
    state: &Rc<RefCell<AppState>>,
    weak: &slint::Weak<AppWindow>,
    i: i32,
    up: bool,
) {
    let (column, rules, editing) = {
        let st = state.borrow();
        (st.rules_panel.column.clone().unwrap_or_default(),
         node_rules(&st, &st.rules_panel.group, &st.rules_panel.name),
         st.rules_panel.editing)
    };
    let order = column_rule_indices(&rules, &column);
    if i < 0 { return; }
    let mode = {
        let st = state.borrow();
        st.engine.node_rules(&st.rules_panel.group, &st.rules_panel.name).map(|(m, _)| m)
    };
    let builtin = mode.map(|m| builtin_rules(m).iter().filter(|r| r.column == column).count())
        .unwrap_or(0);
    let display = i as usize;
    if display < builtin { return; }
    let pos = display - builtin;
    if pos >= order.len() { return; }
    let other_pos = if up {
        if pos == 0 { return; }
        pos - 1
    } else {
        if pos + 1 >= order.len() { return; }
        pos + 1
    };
    let gi = order[pos];
    let other = order[other_pos];
    edit_rules(state, |rules| {
        if gi < rules.len() && other < rules.len() { rules.swap(gi, other); }
    });
    if editing == Some(gi) {
        state.borrow_mut().rules_panel.editing = Some(other);
    } else if editing == Some(other) {
        state.borrow_mut().rules_panel.editing = Some(gi);
    }
    if let Some(ui) = weak.upgrade() { push(&ui, state); }
}
