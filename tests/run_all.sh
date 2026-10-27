#!/bin/bash
# tests/run_all.sh — 测试总入口
#
# 用法:
#   ./tests/run_all.sh              运行全部测试
#   ./tests/run_all.sh 01_project   运行指定目录
#   ./tests/run_all.sh --skip-perf  跳过性能测试
#
# 前置: 默认走 **dev profile**（CLAUDE.md「构建」节：日常不用 release）。
# 10_performance 的耗时数字只有 release 才有意义，所以 CLI 不是 release 构建时
# 该目录自动跳过；要跑性能：
#   CLI=$PWD/target/release/tablet-cli ./tests/run_all.sh
#
# 外部已设 CLI 就用外部的，别再无条件覆盖——否则想跑 dev profile 也跑不了。

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
export CLI="${CLI:-$ROOT_DIR/target/debug/tablet-cli}"

# 解析参数
SKIP_PERF=false
TARGET=""
for arg in "$@"; do
    case "$arg" in
        --skip-perf) SKIP_PERF=true ;;
        *) TARGET="$arg" ;;
    esac
done

# 检查 CLI 二进制（只在默认路径下自动编；外部指定的路径不存在就直接报错）
if [ ! -f "$CLI" ]; then
    if [ "$CLI" = "$ROOT_DIR/target/debug/tablet-cli" ]; then
        echo "编译 tablet-cli (debug)..."
        cargo build -p tablet-cli --manifest-path "$ROOT_DIR/Cargo.toml"
    else
        echo "指定的 CLI 不存在: $CLI" >&2
        exit 1
    fi
fi

# 性能数字只有 release 有意义：CLI 不是 release 构建时不静默给出误导性的耗时
if [[ "$CLI" != *"/target/release/"* ]]; then
    SKIP_PERF_FORCED=true
fi

echo "CLI: $CLI"
"$CLI" --version
echo ""

# 收集测试文件
PASS=0
FAIL=0
SKIP=0

run_test() {
    local test_file="$1"
    local dir_name=$(basename "$(dirname "$test_file")")

    if [ "$dir_name" = "10_performance" ]; then
        if [ "$SKIP_PERF" = true ]; then
            SKIP=$((SKIP + 1))
            return
        fi
        if [ "${SKIP_PERF_FORCED:-false}" = true ]; then
            echo -e "${YELLOW:-}SKIP${NC:-} 非 release 构建，跳过性能测试（要跑：CLI=target/release/tablet-cli）[$dir_name]"
            SKIP=$((SKIP + 1))
            return
        fi
    fi

    if bash "$test_file"; then
        PASS=$((PASS + 1))
    else
        FAIL=$((FAIL + 1))
    fi
}

if [ -n "$TARGET" ]; then
    # 运行指定目录或文件
    if [ -d "$SCRIPT_DIR/$TARGET" ]; then
        for f in "$SCRIPT_DIR/$TARGET"/test_*.sh; do
            [ -f "$f" ] && run_test "$f"
        done
    elif [ -f "$SCRIPT_DIR/$TARGET" ]; then
        run_test "$SCRIPT_DIR/$TARGET"
    elif [ -f "$TARGET" ]; then
        run_test "$TARGET"
    else
        echo "未找到: $TARGET"
        exit 1
    fi
else
    # 运行全部
    for dir in "$SCRIPT_DIR"/*/; do
        for f in "$dir"test_*.sh; do
            [ -f "$f" ] && run_test "$f"
        done
    done
fi

# 汇总
echo ""
echo "════════════════════════════════════"
TOTAL=$((PASS + FAIL))
echo "总计: $TOTAL 个测试文件"
echo "通过: $PASS"
[ $FAIL -gt 0 ] && echo "失败: $FAIL"
[ $SKIP -gt 0 ] && echo "跳过: $SKIP"
echo "════════════════════════════════════"

exit $FAIL
