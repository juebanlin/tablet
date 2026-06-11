#!/bin/bash
# tests/02_schema/test_rename.sh
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "schema/rename"

# ── 测试配置 ──
TEMPLATE="standard"

setup_workspace "$TEMPLATE"

# 重命名 group
run_cli schema rename-group --old hero --new character
assert_exit 0
assert_contains "已重命名 Group"

# 重命名 node
setup_workspace "$TEMPLATE"
run_cli schema rename-node --group hero --old HeroBase --new HeroMain
assert_exit 0
assert_contains "已重命名"
cleanup

end_tests
