#!/bin/bash
# tests/helpers/common.sh — 测试公共函数
# 每个 test_*.sh 通过 source 引入

set -euo pipefail

# CLI 二进制路径：
# - run_all.sh 设置 CLI 环境变量（已编译好的路径）
# - 单独执行时，传 --build 参数触发编译
CLI="${CLI:-}"
TESTS_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROJECT_ROOT="$(cd "$TESTS_ROOT/.." && pwd)"

# Python 路径
if command -v python3 &>/dev/null && python3 --version &>/dev/null; then
    PYTHON="${PYTHON:-python3}"
else
    PYTHON="${PYTHON:-python}"
fi

# 单独执行时的自编译支持
_ensure_cli() {
    if [ -n "$CLI" ] && [ -f "$CLI" ]; then
        return
    fi
    # 检查是否有 --build 参数（单独执行模式）
    if [[ "${1:-}" == "--build" ]] || [ -z "$CLI" ]; then
        local bin="$PROJECT_ROOT/target/release/tablet-cli"
        if [ ! -f "$bin" ]; then
            echo "编译 tablet-cli (release)..."
            cargo build -p tablet-cli --release --manifest-path "$PROJECT_ROOT/Cargo.toml" >/dev/null 2>&1
        fi
        CLI="$bin"
    fi
}

# 颜色
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
NC='\033[0m'

# 测试计数
_PASS=0
_FAIL=0
_TEST_NAME=""

# ─── workspace 管理 ───────────────────────────────────────────────────────

WORK_DIR=""

# trap 确保异常退出时也清理临时目录
trap 'cleanup' EXIT

setup_workspace() {
    local template="${1:-standard}"
    cleanup
    WORK_DIR=$(mktemp -d)
    "$CLI" -w "$WORK_DIR" project new --template "$template" --id test >/dev/null 2>&1
}

setup_empty_workspace() {
    cleanup
    WORK_DIR=$(mktemp -d)
}

cleanup() {
    if [ -n "$WORK_DIR" ] && [ -d "$WORK_DIR" ]; then
        rm -rf "$WORK_DIR"
        WORK_DIR=""
    fi
}

# ─── CLI 调用 ─────────────────────────────────────────────────────────────

LAST_EXIT=0
LAST_STDOUT=""
LAST_STDERR=""

run_cli() {
    local tmpout=$(mktemp)
    local tmperr=$(mktemp)
    set +e
    "$CLI" -w "$WORK_DIR" "$@" >"$tmpout" 2>"$tmperr"
    LAST_EXIT=$?
    set -e
    LAST_STDOUT=$(cat "$tmpout")
    LAST_STDERR=$(cat "$tmperr")
    rm -f "$tmpout" "$tmperr"
}

# 不带 -w 的 CLI 调用（util 命令等）
run_cli_raw() {
    local tmpout=$(mktemp)
    local tmperr=$(mktemp)
    set +e
    "$CLI" "$@" >"$tmpout" 2>"$tmperr"
    LAST_EXIT=$?
    set -e
    LAST_STDOUT=$(cat "$tmpout")
    LAST_STDERR=$(cat "$tmperr")
    rm -f "$tmpout" "$tmperr"
}

# ─── 断言 ─────────────────────────────────────────────────────────────────

assert_exit() {
    local expected=$1
    if [ "$LAST_EXIT" -ne "$expected" ]; then
        _fail "exit code: got $LAST_EXIT, expected $expected\nstdout: $LAST_STDOUT\nstderr: $LAST_STDERR"
    else
        _pass "exit=$expected"
    fi
}

assert_contains() {
    local pattern="$1"
    if echo "$LAST_STDOUT" | grep -q "$pattern"; then
        _pass "contains '$pattern'"
    else
        _fail "stdout does not contain '$pattern'\nstdout: $LAST_STDOUT"
    fi
}

assert_not_contains() {
    local pattern="$1"
    if echo "$LAST_STDOUT" | grep -q "$pattern"; then
        _fail "stdout should not contain '$pattern'"
    else
        _pass "not contains '$pattern'"
    fi
}

assert_stderr_contains() {
    local pattern="$1"
    if echo "$LAST_STDERR" | grep -q "$pattern"; then
        _pass "stderr contains '$pattern'"
    else
        _fail "stderr does not contain '$pattern'\nstderr: $LAST_STDERR"
    fi
}

assert_file_exists() {
    local path="$1"
    if [ -f "$path" ]; then
        _pass "file exists: $path"
    else
        _fail "file not found: $path"
    fi
}

assert_dir_exists() {
    local path="$1"
    if [ -d "$path" ]; then
        _pass "dir exists: $path"
    else
        _fail "dir not found: $path"
    fi
}

# ─── 内部 ─────────────────────────────────────────────────────────────────

_pass() {
    _PASS=$((_PASS + 1))
}

_fail() {
    _FAIL=$((_FAIL + 1))
    echo -e "${RED}FAIL${NC} [${_TEST_NAME}] $1" >&2
}

begin_test() {
    _TEST_NAME="$1"
    cd "$(dirname "${BASH_SOURCE[1]}")"
    _ensure_cli "${2:-}"
}

# 测试文件结尾调用，输出结果
end_tests() {
    local total=$((_PASS + _FAIL))
    if [ $_FAIL -eq 0 ]; then
        echo -e "${GREEN}PASS${NC} $_PASS/$total tests passed [$_TEST_NAME]"
    else
        echo -e "${RED}FAIL${NC} $_PASS passed, $_FAIL failed [$_TEST_NAME]"
    fi
    cleanup
    [ $_FAIL -eq 0 ]
}
