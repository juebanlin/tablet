#!/bin/bash
# tests/07_sep/test_sep_defaults.sh — sep show --defaults 测试
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "sep/defaults"

setup_workspace "standard"
run_cli sep show --defaults
assert_exit 0
assert_contains "List"
assert_contains "Map.kv"
assert_contains "默认"
cleanup

end_tests
