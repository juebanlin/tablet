#!/bin/bash
# tests/01_project/test_info.sh — project info 命令测试
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "project/info"

# ── 测试配置 ──
TEMPLATE="standard"

# ── 正常查询 ──
setup_workspace "$TEMPLATE"
run_cli project info
assert_exit 0
assert_contains "Project:"
assert_contains "Groups:"
assert_contains "Tables:"
cleanup

# ── 指定项目 id ──
setup_empty_workspace
run_cli project new --template standard --id alpha
run_cli project info --id alpha
assert_exit 0
assert_contains "alpha"
cleanup

# ── --id 查一个"盘上有、但没打开"的项目 ──
# load_workspace 只打开 opened_projects / last_project；以前 `project info --id`
# 走 find_project（只看已打开），对存在的项目报"不存在"；`--project` 则被
# set_active_by_id 静默忽略、报告了**另一个**项目。
setup_empty_workspace
run_cli project new --template standard --id first
run_cli project new --template standard --id second   # last_project = second
run_cli project info --id first
assert_exit 0
assert_contains "Project: first"
run_cli --project first project info
assert_exit 0
assert_contains "Project: first"
cleanup

# ── 写坏的 .tbl：整项目打不开（不是少一张表还 exit 0） ──
setup_workspace "$TEMPLATE"
printf '#!tbl v99\n#mode table\n---\n' > "$WORK_DIR/projects/test/config/hero/Bad.tbl"
run_cli project info
assert_exit 1
assert_stderr_contains "Bad.tbl"
cleanup

# ── 写坏的 project.tblmanifest：报错 + 非零退出 ──
# 这条守的是「CLI 给 AI / 脚本调用，没有反馈等于没有发生」：解析失败原来被
# `Err(_) => fallback` 整个吞掉，项目会静默退化成"空身份 + 默认分隔符"，
# 而且要等到下一次保存才炸（那时原始内容已经被覆盖了）。
setup_workspace "$TEMPLATE"
sed -i 's/^# @meta id: test$/# @meta id: Bad-Id/' "$WORK_DIR/projects/test/project.tblmanifest"
run_cli project info --id test
assert_exit 1
assert_stderr_contains "解析失败"
assert_stderr_contains "Bad-Id"
assert_stderr_contains "line 2"
# 关键：断言的是**加载阶段就中止**，而不是"加载成功、后面找不到项目"。
# `project list` 是最好的探针 —— 它不需要那个项目，只有加载闸门会拦它。
run_cli project list
assert_exit 1
assert_stderr_contains "已中止"
cleanup

# ── 但「建一个新的」不能被堵死 ──
# 折腾坏了之后，新建一个正是出路之一。所以 project new 不因为有别的项目打不开就拒绝。
setup_workspace "$TEMPLATE"
sed -i 's/^# @meta id: test$/# @meta id: Bad-Id/' "$WORK_DIR/projects/test/project.tblmanifest"
run_cli project new --template "$TEMPLATE" --id fresh
assert_exit 0
cleanup

end_tests
