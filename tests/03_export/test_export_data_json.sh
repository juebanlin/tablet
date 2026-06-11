#!/bin/bash
# tests/03_export/test_export_data_json.sh — export data --json 测试
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "export/data-json"

# ── 测试配置 ──
TEMPLATE="standard"

setup_workspace "$TEMPLATE"
run_cli export data --json
assert_exit 0
assert_contains "JSON"
assert_contains "新增"
cleanup

end_tests
