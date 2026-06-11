#!/bin/bash
# tests/04_validate/test_validate_group.sh — --group 粒度
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "validate/group"

setup_workspace "standard"
run_cli validate --group hero
assert_exit 0
assert_contains "验证通过"

run_cli validate --group nonexist
assert_exit 0
assert_contains "验证通过"
cleanup

end_tests
