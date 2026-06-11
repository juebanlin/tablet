#!/bin/bash
# tests/04_validate/test_validate_node.sh — --group --node 粒度
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "validate/node"

# ── 测试配置 ──
TEMPLATE="standard"

setup_workspace "$TEMPLATE"
run_cli validate --group hero --node HeroBase
assert_exit 0
assert_contains "验证通过"
cleanup

end_tests
