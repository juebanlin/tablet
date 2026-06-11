#!/bin/bash
# tests/06_workspace/test_save.sh
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "workspace/save"

setup_workspace "standard"
run_cli workspace save
assert_exit 0
assert_contains "已保存"
cleanup

end_tests
