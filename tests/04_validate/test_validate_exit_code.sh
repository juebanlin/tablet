#!/bin/bash
# tests/04_validate/test_validate_exit_code.sh
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "validate/exit-code"

# ── 测试配置 ──
TEMPLATE="standard"

# 合法数据 → exit 0
setup_workspace "$TEMPLATE"
run_cli validate
assert_exit 0
cleanup

end_tests
