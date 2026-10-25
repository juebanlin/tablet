#!/bin/bash
# tests/06_workspace/test_workspace_info.sh
#
# workspace info：只读工作区 + 版本库信息；不加载 .tbl，也不占工作区锁。
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "workspace/info"

setup_workspace standard

# 无版本库
run_cli workspace info
assert_exit 0
assert_contains "Workspace:"
assert_contains "VCS:"
assert_contains "Projects: test"

# 假 .git：分支 main + 提交号
mkdir -p "$WORK_DIR/.git/refs/heads"
printf 'ref: refs/heads/main\n' > "$WORK_DIR/.git/HEAD"
printf '0123456789abcdef0123456789abcdef01234567\n' > "$WORK_DIR/.git/refs/heads/main"

run_cli workspace info
assert_exit 0
assert_contains "Branch: main @ 0123456"

run_cli --fmt json workspace info
assert_exit 0
assert_contains '"vcs": "git"'
assert_contains '"branch": "main"'
assert_contains '"revision": "0123456"'

# GUI / 另一个 CLI 持有锁时，workspace info 仍应可用；普通命令应被拒绝。
# 当前测试进程的 Windows PID（tasklist 认这个；MSYS 的 $$ 不是）。
winpid() { ps -W | awk -v p="$$" '$1 == p { print $4; exit }'; }
WP=$(winpid)
if [ -n "$WP" ]; then
    printf '%s gui\n' "$WP" > "$WORK_DIR/.tablet.lock"
    run_cli workspace info
    assert_exit 0
    run_cli project list
    assert_exit 1
    rm -f "$WORK_DIR/.tablet.lock"
else
    _pass "跳过锁测试：拿不到当前进程的 Windows PID"
fi

cleanup
end_tests
