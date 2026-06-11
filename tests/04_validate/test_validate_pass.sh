#!/bin/bash
# tests/04_validate/test_validate_pass.sh — validate 通过测试
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "validate/pass"

setup_workspace "standard"
run_cli validate
assert_exit 0
assert_contains "验证通过"
cleanup

end_tests
