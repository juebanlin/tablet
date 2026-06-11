#!/bin/bash
# tests/08_util/test_validate_type.sh — util validate-type 测试
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "util/validate-type"

# ── 基本类型通过 ──
run_cli_raw util validate-type int 42
assert_exit 0

run_cli_raw util validate-type str "hello"
assert_exit 0

run_cli_raw util validate-type bool true
assert_exit 0

# ── List 类型通过 ──
run_cli_raw util validate-type "List<int>" "1;2;3"
assert_exit 0

# ── Map 类型通过 ──
run_cli_raw util validate-type "Map<str,int>" "name:10;age:20"
assert_exit 0

# ── 类型不匹配失败 ──
run_cli_raw util validate-type "List<int>" "1;abc;3"
assert_exit 1

run_cli_raw util validate-type int "not_a_number"
assert_exit 1

# ── 自定义分隔符 ──
run_cli_raw util validate-type "List<int>" "1|2|3" --sep List="|"
assert_exit 0

end_tests
