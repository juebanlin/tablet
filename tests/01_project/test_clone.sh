#!/bin/bash
# tests/01_project/test_clone.sh — project clone 命令测试
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "project/clone"

# ── 正常克隆 ──
setup_workspace "standard"
run_cli project clone --source test --id test-copy
assert_exit 0
assert_contains "已克隆"
assert_dir_exists "$WORK_DIR/projects/test-copy"
cleanup

# ── 源不存在 ──
setup_workspace "standard"
run_cli project clone --source nonexist --id copy2
assert_exit 1
cleanup

end_tests
