#!/bin/bash
# tests/06_workspace/test_clear.sh
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "workspace/clear"

# ── 测试配置 ──
TEMPLATE="standard"

# 无 --confirm 拒绝
setup_workspace "$TEMPLATE"
run_cli workspace clear
assert_exit 1
assert_stderr_contains "confirm"
cleanup

# 有 --confirm 执行
setup_workspace "$TEMPLATE"
run_cli workspace clear --confirm
assert_exit 0
assert_contains "已清空"
cleanup

end_tests
