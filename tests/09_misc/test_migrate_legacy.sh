#!/bin/bash
# tests/09_misc/test_migrate_legacy.sh
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "misc/migrate-legacy"

# ── 测试配置 ──
TEMPLATE="standard"

# 已有 projects/ 目录时无需迁移
setup_workspace "$TEMPLATE"
run_cli migrate-legacy
assert_exit 0
assert_contains "无需迁移"
cleanup

end_tests
