#!/bin/bash
# tests/06_workspace/test_reload.sh
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "workspace/reload"

setup_workspace "standard"
run_cli workspace reload
assert_exit 0
assert_contains "已重新加载"
cleanup

end_tests
