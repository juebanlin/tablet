#!/bin/bash
# tests/08_util/test_parse_schema.sh
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "util/parse-schema"

SCHEMA="$SCRIPT_DIR/../schemas/minimal.tblschema"
run_cli_raw util parse-schema "$SCHEMA"
assert_exit 0
assert_contains "Hero"
assert_contains "hero"
cleanup

end_tests
