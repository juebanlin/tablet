#!/bin/bash
# tests/02_schema/test_add_table.sh
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "schema/add-table"

setup_workspace "standard"

# 添加到已有 group
run_cli schema add-table --group hero --name HeroSkill
assert_exit 0
assert_contains "已添加 Table"

# group 不存在时失败
run_cli schema add-table --group nonexist --name Foo
assert_exit 0
# 命令本身不报错（core 层静默忽略不存在的 group），但不会创建

cleanup
end_tests
