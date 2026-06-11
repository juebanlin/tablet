#!/bin/bash
# tests/08_util/test_gen_test.sh
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "util/gen-test"

SCHEMA="$SCRIPT_DIR/../schemas/standard.tblschema"
setup_empty_workspace
OUT_DIR="$WORK_DIR/gen_out"
mkdir -p "$OUT_DIR"

# Java
run_cli_raw util gen-test --lang java --format json --schema "$SCHEMA" -o "$OUT_DIR"
assert_exit 0
assert_file_exists "$OUT_DIR/TestMain.java"

# Go
run_cli_raw util gen-test --lang go --format json --schema "$SCHEMA" -o "$OUT_DIR" --code-output gen/server/go
assert_exit 0
assert_file_exists "$OUT_DIR/test_main_go/main.go"

cleanup
end_tests
