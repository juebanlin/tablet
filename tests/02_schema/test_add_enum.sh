#!/bin/bash
# tests/02_schema/test_add_enum.sh
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "schema/add-enum"

# ── 测试配置 ──
TEMPLATE="standard"

setup_workspace "$TEMPLATE"
run_cli schema add-enum --group hero --name HeroRarity
assert_exit 0
assert_contains "已添加 Enum"
cleanup

end_tests
