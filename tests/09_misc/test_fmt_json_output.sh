#!/bin/bash
# tests/09_misc/test_fmt_json_output.sh — --fmt json 输出验证
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "misc/fmt-json"

# ── 测试配置 ──
TEMPLATE="standard"

setup_workspace "$TEMPLATE"

# project list --fmt json
run_cli --fmt json project list
assert_exit 0
assert_contains "id"

# validate --fmt json
run_cli --fmt json validate
assert_exit 0
assert_contains "errors"

# sep show --fmt json
run_cli --fmt json sep show --defaults
assert_exit 0
assert_contains "entries"

cleanup
end_tests
