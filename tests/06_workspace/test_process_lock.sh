#!/bin/bash
# tests/06_workspace/test_process_lock.sh
#
# GUI 与 CLI 互斥（@03.7.5）：同一个工作区同一时刻只允许一个 tablet 进程在操作。
# 锁文件 `.tablet.lock` 里写 `<pid> <kind>`；持有者崩了留下的残留锁按 PID 存活检测清理。
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "workspace/process-lock"

TEMPLATE="standard"

# 当前测试进程的 Windows PID（tasklist 认这个；MSYS 的 $$ 不是）
winpid() { ps -W | awk -v p="$$" '$1 == p { print $4; exit }'; }

# ── 活进程持有锁：CLI 必须拒绝 ──
setup_workspace "$TEMPLATE"
WP=$(winpid)
if [ -z "$WP" ]; then
    _pass "跳过：拿不到当前进程的 Windows PID"
else
    printf '%s gui\n' "$WP" > "$WORK_DIR/.tablet.lock"
    run_cli project list
    assert_exit 1
    assert_stderr_contains "不能同时操作"
    rm -f "$WORK_DIR/.tablet.lock"
fi
cleanup

# ── 残留锁（PID 不存在）：接管，命令正常，退出后释放 ──
setup_workspace "$TEMPLATE"
printf '4294967290 gui\n' > "$WORK_DIR/.tablet.lock"
run_cli project list
assert_exit 0
if [ -e "$WORK_DIR/.tablet.lock" ]; then
    _fail "CLI 退出后应释放工作区锁"
else
    _pass "CLI 退出后已释放工作区锁"
fi
cleanup

# ── util / list-templates 不碰工作区，不受锁约束 ──
setup_workspace "$TEMPLATE"
WP=$(winpid)
if [ -n "$WP" ]; then
    printf '%s gui\n' "$WP" > "$WORK_DIR/.tablet.lock"
    printf '#!tbl v2\n#mode enum\n---\n1|A|甲\n' > "$WORK_DIR/t.tbl"

    run_cli util parse-tbl "$WORK_DIR/t.tbl"
    assert_exit 0
    run_cli list-templates
    assert_exit 0

    rm -f "$WORK_DIR/.tablet.lock"
fi
cleanup

end_tests
