#!/bin/bash
# tests/03_export/test_export_data_filter.sh — export data --group/--node 粒度
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "export/data-filter"

# ── 测试配置 ──
TEMPLATE="standard"

setup_workspace "$TEMPLATE"

# --group 过滤
run_cli export data --json --group hero
assert_exit 0
assert_contains "JSON"

# --group + --node 过滤
run_cli export data --json --group hero --node HeroBase
assert_exit 0

# 不存在的 group = 用错命令，不是"导出成功且为空"
run_cli export data --json --group nonexist
assert_exit 1
assert_stderr_contains "Group 不存在"

cleanup
end_tests
