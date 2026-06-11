#!/bin/bash
# tests/08_util/test_parse_tbl.sh
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "util/parse-tbl"

# 创建一个带数据的项目，取其 .tbl 文件测试
setup_workspace "standard"
TBL_FILE="$WORK_DIR/projects/test/config/hero/HeroBase.tbl"

run_cli_raw util parse-tbl "$TBL_FILE"
assert_exit 0
assert_contains "table"
assert_contains "HeroBase"
cleanup

end_tests
