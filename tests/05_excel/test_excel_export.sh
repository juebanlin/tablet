#!/bin/bash
# tests/05_excel/test_excel_export.sh
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "excel/export"

# ── 测试配置 ──
TEMPLATE="standard"

setup_workspace "$TEMPLATE"
run_cli excel export --group hero -o "$WORK_DIR/hero.xlsx"
assert_exit 0
assert_file_exists "$WORK_DIR/hero.xlsx"
assert_contains "Excel"
cleanup

end_tests
