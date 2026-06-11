#!/bin/bash
# tests/03_export/test_export_code_java.sh
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "export/code-java"

setup_workspace "standard"
run_cli export code --java
assert_exit 0
assert_contains "Java"
assert_contains "新增"
cleanup

end_tests
