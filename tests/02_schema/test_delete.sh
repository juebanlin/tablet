#!/bin/bash
# tests/02_schema/test_delete.sh
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "schema/delete"

# ── 测试配置 ──
TEMPLATE="standard"

setup_workspace "$TEMPLATE"

# 删除 node
run_cli schema delete-node --group hero --name HeroBase
assert_exit 0
assert_contains "已删除"

# 删除 group
run_cli schema delete-group --name hero
assert_exit 0
assert_contains "已删除 Group"
cleanup

end_tests
