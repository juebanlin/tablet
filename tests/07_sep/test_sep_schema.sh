#!/bin/bash
# tests/07_sep/test_sep_schema.sh — sep show --schema
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "sep/schema"

SCHEMA="$SCRIPT_DIR/../schemas/standard.tblschema"
setup_empty_workspace
run_cli sep show --schema "$SCHEMA"
assert_exit 0
assert_contains "List"
assert_contains "Map.kv"
cleanup

end_tests
