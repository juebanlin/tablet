#!/bin/bash
# tests/run_all.sh — 测试总入口
#
# 用法:
#   ./tests/run_all.sh              运行全部测试
#   ./tests/run_all.sh 01_project   运行指定目录
#   ./tests/run_all.sh --skip-perf  跳过性能测试
#
# 前置: cargo build -p tablet-cli --release

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
export CLI="$ROOT_DIR/target/release/tablet-cli"

# 解析参数
SKIP_PERF=false
TARGET=""
for arg in "$@"; do
    case "$arg" in
        --skip-perf) SKIP_PERF=true ;;
        *) TARGET="$arg" ;;
    esac
done

# 检查 CLI 二进制
if [ ! -f "$CLI" ]; then
    echo "编译 tablet-cli (release)..."
    cargo build -p tablet-cli --release --manifest-path "$ROOT_DIR/Cargo.toml"
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

    if [ "$SKIP_PERF" = true ] && [ "$dir_name" = "10_performance" ]; then
        SKIP=$((SKIP + 1))
        return
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
