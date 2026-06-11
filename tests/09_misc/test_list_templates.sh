#!/bin/bash
# tests/09_misc/test_list_templates.sh
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "misc/list-templates"

setup_empty_workspace
run_cli list-templates
assert_exit 0
assert_contains "内置模板"
assert_contains "standard"
cleanup

end_tests
