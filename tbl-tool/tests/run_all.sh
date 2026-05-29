#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "=== 编译 tbl-cli ==="
cd "$ROOT_DIR"
cargo build --release -p tbl-cli 2>&1 | grep -v "^warning:" | grep -v "^$"

export CLI="$ROOT_DIR/target/release/tbl-cli"

PASS=0
FAIL=0

for DIR in "$SCRIPT_DIR"/*/; do
    [ -f "$DIR/run.sh" ] || continue
    SCENE=$(basename "$DIR")

    echo ""
    echo "--- 测试: $SCENE ---"

    pushd "$DIR" > /dev/null
    rm -rf gen tbl-tool.toml out actual_output.txt config TestMain.java test_main_go go.mod go.sum

    if bash run.sh > actual_output.txt 2>&1; then
        if diff --strip-trailing-cr -u expected_output.txt actual_output.txt > /dev/null 2>&1; then
            echo "PASS: $SCENE"
            PASS=$((PASS + 1))
        else
            echo "FAIL: $SCENE (output mismatch)"
            diff --strip-trailing-cr -u expected_output.txt actual_output.txt || true
            FAIL=$((FAIL + 1))
        fi
    else
        echo "FAIL: $SCENE (execution error)"
        cat actual_output.txt 2>/dev/null || true
        FAIL=$((FAIL + 1))
    fi
    popd > /dev/null
done

echo ""
echo "=== 结果: $PASS 通过, $FAIL 失败 ==="
[ $FAIL -eq 0 ] && exit 0 || exit 1
