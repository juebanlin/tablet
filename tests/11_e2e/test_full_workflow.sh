#!/bin/bash
# tests/11_e2e/test_full_workflow.sh — 完整工作流：new → schema → export → validate
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "e2e/full-workflow"

setup_empty_workspace

# 1. 创建项目
run_cli project new --template standard --id e2e-test --name "E2E"
assert_exit 0

# 2. 查看结构
run_cli schema show
assert_exit 0
assert_contains "table"

# 3. 验证
run_cli validate
assert_exit 0
assert_contains "验证通过"

# 4. 导出全部
run_cli export all
assert_exit 0
assert_contains "JSON"
assert_contains "Java"

# 5. 导出数据子集
run_cli export data --json --group hero
assert_exit 0

# 6. Excel 导出
run_cli excel export --group hero -o "$WORK_DIR/hero.xlsx"
assert_exit 0
assert_file_exists "$WORK_DIR/hero.xlsx"

# 7. 项目信息
run_cli project info
assert_exit 0
assert_contains "e2e-test"

cleanup
end_tests
