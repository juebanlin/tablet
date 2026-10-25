#!/bin/bash
# tests/07_sep/test_sep_project.sh — sep show 带项目上下文：展示的是哪个项目
#
# 背景：`sep show` 曾经完全不读项目（三个来源参数都是 None，只打内置默认值），
# 于是「带项目上下文」这半边是坏的，而文档写着它展示「当前项目的生效分隔符」。
# 这条测试守住两件事：
#   1. 裸 `sep show` 反映**当前项目** schema 里的 `# @sep`
#   2. 多项目工作区里各看各的，表头说清是哪个项目
#
# 分隔符的运行期来源只有一个：项目自己的 project.tblmanifest（@01.7.4）。
# tablet.toml [separators] 早已降级为「新建空项目时拷贝一次的模板」，不参与运行期。

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "sep/project"

# 在第 1 行（#!tblschema 头）之后插一行 directive
insert_directive() {
    local f="$1" line="$2"
    { head -1 "$f"; printf '%s\n' "$line"; tail -n +2 "$f"; } > "$f.tmp"
    mv "$f.tmp" "$f"
}

setup_empty_workspace
"$CLI" -w "$WORK_DIR" project new --template standard --id pa >/dev/null 2>&1
"$CLI" -w "$WORK_DIR" project new --template standard --id pb >/dev/null 2>&1

# 只给 pa 设一个偏离默认值的列表分隔符（默认是 ;）
insert_directive "$WORK_DIR/projects/pa/project.tblmanifest" '# @sep List = ~'

# ── pa：应反映自己的 schema，并在表头标明项目 ──
run_cli --project pa sep show
assert_exit 0
assert_contains "项目 pa"
assert_contains 'List .*~'
assert_contains "项目"

# ── pb：没改过 → 全默认，不该被 pa 的设置串味 ──
run_cli --project pb sep show
assert_exit 0
assert_contains "项目 pb"
assert_not_contains "~"

# ── 不给 --project 时走 active（同一套解析规则），至少要标明项目 ──
run_cli sep show
assert_exit 0
assert_contains "分隔符配置（项目 "

# ── 显式来源仍与项目无关，且不标项目 ──
run_cli sep show --schema "$WORK_DIR/projects/pa/project.tblmanifest"
assert_exit 0
assert_contains 'List .*~'
assert_not_contains "分隔符配置（项目"

cleanup
end_tests
