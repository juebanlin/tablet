#!/bin/bash
# tests/01_project/test_rename.sh — project rename 命令测试
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "project/rename"

# ── 测试配置 ──
TEMPLATE="standard"

# ── 改名称 ──
setup_workspace "$TEMPLATE"
run_cli project rename --id test --new-name "新名字"
assert_exit 0
cleanup

# ── 改 id ──
setup_workspace "$TEMPLATE"
run_cli project rename --id test --new-id renamed
assert_exit 0
assert_dir_exists "$WORK_DIR/projects/renamed"
cleanup

end_tests
