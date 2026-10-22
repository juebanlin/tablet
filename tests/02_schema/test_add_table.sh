#!/bin/bash
# tests/02_schema/test_add_table.sh
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "schema/add-table"

# ── 测试配置 ──
TEMPLATE="standard"

setup_workspace "$TEMPLATE"

# 添加到已有 group
run_cli schema add-table --group hero --name HeroSkill
assert_exit 0
assert_contains "已添加 Table"

# group 不存在 = 用错命令，必须非零退出并说清楚。
# 以前这里 exit 0 且 println"已添加 Table" —— core 对不存在的 group 是静默 no-op，
# 而 CLI 照样对外宣布成功（@02.26.7「读不懂就报错」同一个道理）。
run_cli schema add-table --group nonexist --name Foo
assert_exit 1
assert_stderr_contains "Group 不存在"

cleanup
end_tests
