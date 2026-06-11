#!/bin/bash
# tests/08_util/test_scaffold.sh
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "util/scaffold"

SCHEMA="$SCRIPT_DIR/../schemas/minimal.tblschema"
setup_empty_workspace
OUT_DIR="$WORK_DIR/scaffold_out"

run_cli_raw util scaffold "$SCHEMA" -o "$OUT_DIR"
assert_exit 0
assert_dir_exists "$OUT_DIR"
assert_file_exists "$OUT_DIR/project.tblschema"
cleanup

end_tests
