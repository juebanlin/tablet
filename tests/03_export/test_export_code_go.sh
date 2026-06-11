#!/bin/bash
# tests/03_export/test_export_code_go.sh
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "export/code-go"

# ── 测试配置 ──
TEMPLATE="standard"

setup_workspace "$TEMPLATE"
run_cli export code --go
assert_exit 0
assert_contains "Go"
cleanup

end_tests
