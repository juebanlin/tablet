#!/bin/bash
# tests/01_project/test_delete.sh — project delete 命令测试
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "project/delete"

# ── 无 --confirm 拒绝 ──
setup_workspace "standard"
run_cli project delete --id test
assert_exit 1
assert_stderr_contains "confirm"
assert_dir_exists "$WORK_DIR/projects/test"
cleanup

# ── 有 --confirm 删除 ──
setup_empty_workspace
run_cli project new --template standard --id to-delete
run_cli project new --template standard --id keep
run_cli project delete --id to-delete --confirm
assert_exit 0
cleanup

end_tests
