#!/bin/bash
# tests/03_export/test_export_all.sh — export all 命令测试
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "export/all"

# ── 测试配置 ──
TEMPLATE="standard"

setup_workspace "$TEMPLATE"
run_cli export all
assert_exit 0
assert_contains "JSON"
assert_contains "Java"
cleanup

end_tests
