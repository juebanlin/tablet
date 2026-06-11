#!/bin/bash
# tests/03_export/test_export_data_xml.sh
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "export/data-xml"

setup_workspace "standard"
run_cli export data --xml
assert_exit 0
assert_contains "XML"
assert_contains "新增"
cleanup

end_tests
