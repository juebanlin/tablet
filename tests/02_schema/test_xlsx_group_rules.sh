#!/bin/bash
# tests/02_schema/test_xlsx_group_rules.sh
# 原生 xlsx 组与 tbl 组共用同一套规则：规则只住 project.tblmanifest，
# 加载后对 xlsx 组同样生效（@01.8.11 / @02.27）。
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "schema/xlsx-group-rules"

setup_workspace "standard"

run_cli schema add-group --name xg --storage xlsx
assert_exit 0
run_cli schema add-table --group xg --name XT
assert_exit 0

SCHEMA_FILE="$WORK_DIR/projects/test/project.tblmanifest"
assert_file_exists "$SCHEMA_FILE"
assert_file_contains "$SCHEMA_FILE" "table xlsx"

# 规则写在段头之后（解析与位置无关）
sed -i '/^\[xg\/XT\] table xlsx$/a # @rule: id = range(1, 10)' "$SCHEMA_FILE"

# 重新加载项目时把规则应用到 xlsx 组的节点上
run_cli schema show --rules
assert_exit 0
assert_contains "id = range(1, 10)"

# 内置规则对 xlsx 组同样存在
assert_contains "unique()"
assert_contains "系统"

cleanup
end_tests
