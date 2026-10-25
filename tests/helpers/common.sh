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
        # 走 dev profile——日常跑测试用 debug 构建快得多（CLAUDE.md「构建」节）
        local bin="$PROJECT_ROOT/target/debug/tablet-cli"
        if [ ! -f "$bin" ]; then
            echo "编译 tablet-cli (debug)..."
            cargo build -p tablet-cli --manifest-path "$PROJECT_ROOT/Cargo.toml" >/dev/null 2>&1
        fi
        CLI="$bin"
    fi
}

# ─── 外部工具发现 ─────────────────────────────────────────────────────────

# find_tool <覆盖用环境变量名> <候选名(空格分隔)> [额外搜索目录...]
#
# 比裸 `command -v` 多两件事，都是 Windows + MSYS 上真会踩的坑：
# 1. 同名**目录**会挡住同名 .exe —— MSYS bash 沿 PATH 撞到一个叫 `lua` 的目录就不再找
#    `lua.exe` 了。LuaRocks 的 `C:\Program Files (x86)\Lua\5.1\lua` 正是这种。
#    所以候选名一律连 `<name>.exe` 一起试。
# 2. 装了但没进 PATH —— 允许传额外目录兜底。
#
# 找到返回绝对路径，找不到返回空串（调用方据此 SKIP）。
find_tool() {
    local env_name="$1" names="$2"
    shift 2

    local override="${!env_name:-}"
    if [ -n "$override" ]; then
        if [ -x "$override" ]; then printf '%s\n' "$override"; return 0; fi
        local resolved
        resolved="$(command -v "$override" 2>/dev/null || true)"
        if [ -n "$resolved" ]; then printf '%s\n' "$resolved"; return 0; fi
    fi

    local name cand dir
    for name in $names; do
        for cand in "$name" "$name.exe"; do
            local found
            found="$(command -v "$cand" 2>/dev/null || true)"
            if [ -n "$found" ] && [ -f "$found" ]; then printf '%s\n' "$found"; return 0; fi
        done
    done

    for dir in "$@"; do
        for name in $names; do
            for cand in "$name" "$name.exe"; do
                if [ -x "$dir/$cand" ]; then printf '%s\n' "$dir/$cand"; return 0; fi
            done
        done
    done

    return 0
}

# find_mvn — 找一个能执行的 maven 启动器
#
# 只认 maven 自己声明的入口，不扫盘找安装目录：
#   1. $MVN（显式指定）
#   2. PATH 上的 mvn
#   3. $MAVEN_HOME/bin/mvn、$M2_HOME/bin/mvn
#
# 注意取的是**不带扩展名的那个**：Maven 同时装 `mvn`（sh 脚本）和 `mvn.cmd`，
# 而 Git Bash 不能直接执行 .cmd（这正是 cmd 下 mvn 正常、Bash 下 `command -v mvn`
# 找不到的同一类问题）。所以优先 sh 版，只有它不存在时才退回 .cmd。
find_mvn() {
    if [ -n "${MVN:-}" ] && [ -x "$MVN" ]; then printf '%s\n' "$MVN"; return 0; fi

    local found
    found="$(command -v mvn 2>/dev/null || true)"
    if [ -n "$found" ]; then printf '%s\n' "$found"; return 0; fi

    # MAVEN_HOME 在 Windows 上是 `D:\...` 形式，直接拼 `$home/bin/mvn` 会得到
    # 混合分隔符的怪路径。MSYS 恰好吃得下，但别依赖这个——统一转成 /d/...
    local home
    for home in "${MAVEN_HOME:-}" "${M2_HOME:-}"; do
        [ -n "$home" ] || continue
        home="$(to_msys_path "$home")"
        if [ -x "$home/bin/mvn" ]; then printf '%s\n' "$home/bin/mvn"; return 0; fi
        if [ -x "$home/bin/mvn.cmd" ]; then printf '%s\n' "$home/bin/mvn.cmd"; return 0; fi
    done
    return 0
}

# maven_local_repository — 问 maven 本地仓库在哪
#
# 本地仓库位置是**可配置**的（settings.xml 的 <localRepository>、全局 settings、
# -Dmaven.repo.local），`~/.m2/repository` 只是默认值。与其手搓 sed 去猜配置，
# 不如直接问 maven 本人——它会把所有配置层级算完再给出权威答案。
#
# 代价：trampoline 一个 JVM，冷启 ~7s、暖缓存 ~1.5s（本机实测）。
# 所以只在 maven 够得着时才走这条路，够不着由调用方回落到解析 settings.xml。
maven_local_repository() {
    local mvn
    mvn="$(find_mvn)"
    [ -n "$mvn" ] || return 0
    "$mvn" -q help:evaluate -Dexpression=settings.localRepository -DforceStdout 2>/dev/null \
        | tr -d '\r' | grep -v '^[[:space:]]*$' | tail -1
}

# find_lombok_jar — 找一个可用的 lombok.jar（Java 产物开了 @Getter 时编译要用）
#
# 顺序：LOMBOK_JAR 覆盖 → maven 问出来的本地仓库 → 解析 settings.xml 兜底
#      → 默认 ~/.m2/repository → gradle 缓存。取版本号最高的那个。
# 找不到返回空串，调用方据此 SKIP。
#
# gradle 缓存的坐标写法不同（点号分隔），单独列。
#
# ⚠️ 版本与 JDK 强相关：Lombok 直接摸 javac 内部 API，1.18.30 在 JDK 25 上会
# ExceptionInInitializerError。JDK 25 需要 >= 1.18.40。判定用 lombok_usable() 真编译探测。
find_lombok_jar() {
    # 覆盖值存在就用它；指向不存在的文件时**继续自动查找**，而不是直接返回空——
    # 否则报出来的 SKIP 会说「请设 LOMBOK_JAR」，而调用方明明已经设了（同 find_tool）
    if [ -n "${LOMBOK_JAR:-}" ] && [ -f "$LOMBOK_JAR" ]; then
        printf '%s\n' "$LOMBOK_JAR"
        return 0
    fi

    local repos=() roots=() repo root jar custom
    custom="$(maven_local_repository)"
    [ -n "$custom" ] && repos+=("$(to_msys_path "$custom")")

    # maven 够不着（不在 PATH、也没设 MVN/MAVEN_HOME/M2_HOME）时退而解析 settings.xml
    if [ ${#repos[@]} -eq 0 ] && [ -f "$HOME/.m2/settings.xml" ]; then
        custom="$(sed -n 's:.*<localRepository>\([^<]*\)</localRepository>.*:\1:p' \
                  "$HOME/.m2/settings.xml" | head -1)"
        [ -n "$custom" ] && repos+=("$(to_msys_path "$custom")")
    fi

    repos+=("$HOME/.m2/repository")
    for repo in "${repos[@]}"; do roots+=("$repo/org/projectlombok/lombok"); done
    roots+=("$HOME/.gradle/caches/modules-2/files-2.1/org.projectlombok/lombok")

    for root in "${roots[@]}"; do
        [ -d "$root" ] || continue
        jar="$(find "$root" -name 'lombok-*.jar' \
                ! -name '*-sources.jar' ! -name '*-javadoc.jar' 2>/dev/null \
               | sort -V | tail -1)"
        if [ -n "$jar" ]; then printf '%s\n' "$jar"; return 0; fi
    done

    return 0
}

# to_msys_path — Windows 路径（D:\a\b 或 D:/a/b）转成 MSYS 形式（/d/a/b）
#
# 必须转：`D:/...` 塞进 `-cp "src:D:/jar"` 这种**冒号分隔**的列表里会被 MSYS 拆错，
# jar 悄悄没进 classpath，症状是「程序包 lombok 不存在」。
to_msys_path() {
    local p="$1"
    p="$(printf '%s' "$p" | tr '\\' '/')"
    if command -v cygpath >/dev/null 2>&1; then
        printf '%s\n' "$(cygpath -u "$p" 2>/dev/null || printf '%s' "$p")"
    else
        printf '%s\n' "$p"
    fi
}

# lombok_usable <jar> — 这个 jar 在当前 JDK 上真的能用吗？
#
# 用真编译探测，不去匹配编译错误文本。匹配文本太脆：Lombok 的 deprecation 警告里
# 也含 `lombok.permit.Permit`，会把真正的编译错误误判成「jar 与 JDK 不匹配」，
# 于是真 bug 被 SKIP 吞掉。
#
# 顺带把「javac 静默忽略注解处理器」也探掉了：只给 -cp 时 javac 在 JDK 23+ 不会跑
# 处理器，编译 exit 0 但产物里没有任何 getter —— 所以这里额外用 javap 确认
# Lombok 真的生成了 getValue()，而不只是「编译没报错」。
lombok_usable() {
    local jar="$1" probe
    [ -f "$jar" ] || return 1
    probe="$(mktemp -d)"
    mkdir -p "$probe/src/p"
    cat > "$probe/src/p/Probe.java" <<'PROBE_EOF'
package p;
import lombok.Getter;
@Getter
public class Probe { private int value; }
PROBE_EOF
    local rc=1
    if javac -processorpath "$jar" -cp "$jar" -d "$probe/out" \
             "$probe/src/p/Probe.java" >/dev/null 2>&1; then
        if javap -p -cp "$probe/out" p.Probe 2>/dev/null | grep -q "getValue()"; then
            rc=0
        fi
    fi
    rm -rf "$probe"
    return $rc
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

assert_stderr_not_contains() {
    local pattern="$1"
    if echo "$LAST_STDERR" | grep -q "$pattern"; then
        _fail "stderr should not contain '$pattern'\nstderr: $LAST_STDERR"
    else
        _pass "stderr not contains '$pattern'"
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

assert_file_not_exists() {
    local path="$1"
    if [ -e "$path" ]; then
        _fail "path should not exist: $path"
    else
        _pass "path absent: $path"
    fi
}

assert_dir_not_exists() {
    local path="$1"
    if [ -d "$path" ]; then
        _fail "dir should not exist: $path"
    else
        _pass "dir absent: $path"
    fi
}

assert_file_contains() {
    local path="$1"
    local pattern="$2"
    if [ -f "$path" ] && grep -q "$pattern" "$path"; then
        _pass "file contains '$pattern': $path"
    else
        _fail "file does not contain '$pattern': $path"
    fi
}

assert_file_not_contains() {
    local path="$1"
    local pattern="$2"
    if [ -f "$path" ] && grep -q "$pattern" "$path"; then
        _fail "file should not contain '$pattern': $path"
    else
        _pass "file does not contain '$pattern': $path"
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
