#!/bin/bash
# tests/09_misc/test_help.sh — --help 不报错测试
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "misc/help"

# 顶层
run_cli_raw --help
assert_exit 0
assert_contains "Commands"

# 子命令 help
run_cli_raw project --help
assert_exit 0

run_cli_raw schema --help
assert_exit 0

run_cli_raw export --help
assert_exit 0
assert_contains "data"
assert_contains "code"
assert_contains "all"

run_cli_raw util --help
assert_exit 0

run_cli_raw sep --help
assert_exit 0

end_tests
