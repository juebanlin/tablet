#!/bin/bash
# tests/01_project/test_list.sh — project list 命令测试
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "project/list"

# ── 空 workspace ──
setup_empty_workspace
run_cli project list
assert_exit 0
assert_contains "无 Project"
cleanup

# ── 有项目时列出 ──
setup_workspace "standard"
run_cli project list
assert_exit 0
assert_contains "test"
cleanup

# ── 多项目 ──
setup_empty_workspace
run_cli project new --template standard --id proj-a
run_cli project new --template standard --id proj-b
run_cli project list
assert_exit 0
assert_contains "proj-a"
assert_contains "proj-b"
cleanup

end_tests
