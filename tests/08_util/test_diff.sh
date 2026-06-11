#!/bin/bash
# tests/08_util/test_diff.sh
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "util/diff"

setup_workspace "standard"
TBL_FILE="$WORK_DIR/projects/test/config/hero/HeroBase.tbl"

# 自己和自己对比 → 无差异
run_cli_raw util diff "$TBL_FILE" "$TBL_FILE"
assert_exit 0
assert_contains "无差异"
cleanup

end_tests
