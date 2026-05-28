#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
CLI="$ROOT_DIR/target/release/tbl-cli"

echo "=== 编译 tbl-cli ==="
cd "$ROOT_DIR"
cargo build --release -p tbl-cli 2>&1 | grep -v "^warning:" | grep -v "^$"

PASS=0
FAIL=0

run_java_test() {
    local SCENE=$1
    local EXTRA_ARGS="${@:2}"
    local DIR="$SCRIPT_DIR/$SCENE"

    echo ""
    echo "--- 测试: $SCENE ---"

    rm -rf "$DIR/gen" "$DIR/tbl-tool.toml" "$DIR/out" "$DIR/actual_output.txt"

    cd "$DIR"
    $CLI -w . $EXTRA_ARGS export

    mkdir -p out
    javac -d out -sourcepath gen/server/code gen/server/code/com/game/config/*.java \
        gen/server/code/com/game/config/types/*.java \
        gen/server/code/com/game/config/hero/*.java \
        $(find gen/server/code -name "*.java" -path "*/global/*" 2>/dev/null) \
        TestMain.java 2>&1 | grep -v "unchecked" | grep -v "^$" || true

    java -cp out -Dfile.encoding=UTF-8 -Dstdout.encoding=UTF-8 TestMain gen/server/data > actual_output.txt 2>&1

    if diff --strip-trailing-cr -u expected_output.txt actual_output.txt > /dev/null 2>&1; then
        echo "PASS: $SCENE"
        PASS=$((PASS + 1))
    else
        echo "FAIL: $SCENE"
        diff --strip-trailing-cr -u expected_output.txt actual_output.txt || true
        FAIL=$((FAIL + 1))
    fi
}

run_java_test "java_json"
run_java_test "java_empty_omit" "-s export.json.empty_as=omit"

echo ""
echo "=== 结果: $PASS 通过, $FAIL 失败 ==="
[ $FAIL -eq 0 ] && exit 0 || exit 1
