#!/bin/bash
# tests/05_excel/test_excel_roundtrip.sh — export → import 数据不变
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "excel/roundtrip"

setup_workspace "standard"

# 导出
run_cli excel export --group hero -o "$WORK_DIR/hero_rt.xlsx"
assert_exit 0

# 回导
run_cli excel import --group hero --file "$WORK_DIR/hero_rt.xlsx"
assert_exit 0
assert_contains "已导入"

# 验证数据仍然合法
run_cli validate --group hero
assert_exit 0
assert_contains "验证通过"
cleanup

end_tests
