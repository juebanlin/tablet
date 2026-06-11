#!/bin/bash
# tests/03_export/test_export_code_typescript.sh
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "export/code-typescript"

setup_workspace "standard"
run_cli export code --typescript
assert_exit 0
assert_contains "TypeScript"
cleanup

end_tests
