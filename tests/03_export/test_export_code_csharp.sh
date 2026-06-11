#!/bin/bash
# tests/03_export/test_export_code_csharp.sh
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "export/code-csharp"

# ── 测试配置 ──
TEMPLATE="standard"

setup_workspace "$TEMPLATE"
run_cli export code --csharp
assert_exit 0
assert_contains "C#"
cleanup

end_tests
