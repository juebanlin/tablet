#!/bin/bash
# tests/02_schema/test_show.sh — schema show 命令测试
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "schema/show"

setup_workspace "standard"
run_cli schema show
assert_exit 0
assert_contains "table"
assert_contains "enum"
assert_contains "constant"
cleanup

end_tests
