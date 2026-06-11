#!/bin/bash
# tests/02_schema/test_add_constant.sh
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "schema/add-constant"

# ── 测试配置 ──
TEMPLATE="standard"

setup_workspace "$TEMPLATE"
run_cli schema add-constant --group global --name ServerConfig
assert_exit 0
assert_contains "已添加 Constant"
cleanup

end_tests
