#!/bin/bash
# tests/08_util/test_fmt.sh
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "util/fmt"

# ── 测试配置 ──
TEMPLATE="standard"

setup_workspace "$TEMPLATE"
TBL_FILE="$WORK_DIR/projects/test/config/hero/HeroBase.tbl"

# stdout 模式
run_cli_raw util fmt "$TBL_FILE"
assert_exit 0
assert_contains "#!tbl"

# in-place 模式
run_cli_raw util fmt "$TBL_FILE" -i
assert_exit 0
assert_contains "已格式化"
cleanup

end_tests
