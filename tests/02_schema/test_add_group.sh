#!/bin/bash
# tests/02_schema/test_add_group.sh — schema add-group 测试
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "schema/add-group"

# ── 测试配置 ──
TEMPLATE="standard"

setup_workspace "$TEMPLATE"

# 添加新 group 成功
run_cli schema add-group --name newgroup
assert_exit 0
assert_contains "已添加 Group"

# 添加 table 到新 group
run_cli schema add-table --group newgroup --name TestTable
assert_exit 0
assert_contains "已添加 Table"

cleanup

end_tests
