#!/bin/bash
# tests/01_project/test_info.sh — project info 命令测试
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "project/info"

# ── 正常查询 ──
setup_workspace "standard"
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

end_tests
