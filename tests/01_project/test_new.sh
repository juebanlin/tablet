#!/bin/bash
# tests/01_project/test_new.sh — project new 命令测试
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "project/new"

# ── 正常创建 ──
setup_empty_workspace
run_cli project new --template standard --id myproj --name "测试项目"
assert_exit 0
assert_contains "已创建 Project"
assert_dir_exists "$WORK_DIR/projects/myproj"
assert_file_exists "$WORK_DIR/projects/myproj/project.tblschema"
cleanup

# ── 重复创建失败 ──
setup_empty_workspace
run_cli project new --template standard --id dup
assert_exit 0
run_cli project new --template standard --id dup
assert_exit 1
cleanup

# ── 非法 id ──
setup_empty_workspace
run_cli project new --template standard --id "INVALID!"
assert_exit 1
cleanup

end_tests
