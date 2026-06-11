#!/bin/bash
# tests/08_util/test_stat.sh
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "util/stat"

setup_workspace "standard"
TBL_FILE="$WORK_DIR/projects/test/config/hero/HeroBase.tbl"
CONFIG_DIR="$WORK_DIR/projects/test/config"

# 单文件统计
run_cli_raw util stat "$TBL_FILE"
assert_exit 0
assert_contains "table"
assert_contains "数据行"

# 目录统计
run_cli_raw util stat "$CONFIG_DIR"
assert_exit 0
assert_contains "目录"
assert_contains "文件"
cleanup

end_tests
