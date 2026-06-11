#!/bin/bash
# tests/08_util/test_tbl_to_xlsx.sh
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "util/tbl-to-xlsx"

setup_workspace "standard"
TBL_FILE="$WORK_DIR/projects/test/config/hero/HeroBase.tbl"
OUT_XLSX="$WORK_DIR/out.xlsx"

run_cli_raw util tbl-to-xlsx "$TBL_FILE" -o "$OUT_XLSX"
assert_exit 0
assert_file_exists "$OUT_XLSX"
assert_contains "已转换"
cleanup

end_tests
