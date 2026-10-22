#!/bin/bash
# tests/01_project/test_rename.sh — project rename 命令测试
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "project/rename"

# ── 测试配置 ──
TEMPLATE="standard"

# ── 改名称 ──
setup_workspace "$TEMPLATE"
run_cli project rename --id test --new-name "新名字"
assert_exit 0
cleanup

# ── 改 id ──
setup_workspace "$TEMPLATE"
run_cli project rename --id test --new-id renamed
assert_exit 0
assert_dir_exists "$WORK_DIR/projects/renamed"
cleanup

# ── 非法 id：一律拒绝，且原项目不动 ──
# 「只判 new_root.exists()」会放过这些 —— 新建/克隆都拦字符集，只有改名这条放行，
# 于是能造出一个别的地方都不允许存在的 id。
for BAD in "Bad-Id" "foo bar" "中文"; do
    setup_workspace "$TEMPLATE"
    run_cli project rename --id test --new-id "$BAD"
    assert_exit 1
    assert_dir_exists "$WORK_DIR/projects/test"
    cleanup
done

# ── 路径穿越：`../escaped` 曾经把项目从 projects/ 里挪到工作区根（实测过） ──
setup_workspace "$TEMPLATE"
run_cli project rename --id test --new-id "../escaped"
assert_exit 1
assert_dir_exists "$WORK_DIR/projects/test"
if [ -e "$WORK_DIR/escaped" ]; then _fail "项目被挪出了 projects/ 目录"; else _pass; fi
cleanup

# ── id 撞车 ──
setup_workspace "$TEMPLATE"
run_cli project new --template "$TEMPLATE" --id other >/dev/null 2>&1
run_cli project rename --id test --new-id other
assert_exit 1
assert_dir_exists "$WORK_DIR/projects/test"
cleanup

end_tests
