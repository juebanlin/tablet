#!/bin/bash
# tests/04_validate/test_validate_group.sh — --group 粒度
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "validate/group"

# ── 测试配置 ──
TEMPLATE="standard"

setup_workspace "$TEMPLATE"
run_cli validate --group hero
assert_exit 0
assert_contains "验证通过"

# 空集恒真：`--group ` 拼错时曾经返回"验证通过" + exit 0，脚本无从分辨
# 「这个组没问题」和「根本没有这个组」。
run_cli validate --group nonexist
assert_exit 1
assert_stderr_contains "Group 不存在"
cleanup

end_tests
