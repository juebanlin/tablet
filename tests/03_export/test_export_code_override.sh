#!/bin/bash
# tests/03_export/test_export_code_override.sh — --package/--namespace/-o 覆盖
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "export/code-override"

# ── 测试配置 ──
TEMPLATE="standard"

setup_workspace "$TEMPLATE"

# --package 覆盖
run_cli export code --java --package com.test.override
assert_exit 0
assert_contains "Java"

# --namespace 覆盖
run_cli export code --cpp --namespace test::ns
assert_exit 0
assert_contains "C++"

# -o 输出路径覆盖
run_cli export code --java -o "$WORK_DIR/custom_out"
assert_exit 0
assert_dir_exists "$WORK_DIR/custom_out"

cleanup
end_tests
