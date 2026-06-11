#!/bin/bash
# tests/03_export/test_export_data_filter.sh — export data --group/--node 粒度
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "export/data-filter"

setup_workspace "standard"

# --group 过滤
run_cli export data --json --group hero
assert_exit 0
assert_contains "JSON"

# --group + --node 过滤
run_cli export data --json --group hero --node HeroBase
assert_exit 0

# 不存在的 group（仍成功但无文件生成）
run_cli export data --json --group nonexist
assert_exit 0

cleanup
end_tests
