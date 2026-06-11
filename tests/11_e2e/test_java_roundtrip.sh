#!/bin/bash
# tests/11_e2e/test_java_roundtrip.sh — Java 端到端：export → javac → run → 验证输出
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "e2e/java-roundtrip"

# 检查 Java 环境
if ! command -v javac &>/dev/null || ! command -v java &>/dev/null; then
    echo -e "${YELLOW}SKIP${NC} javac/java not found [e2e/java-roundtrip]"
    _PASS=1
    end_tests
    exit 0
fi

# ── 配置变量（export code 和 gen-test 共享）──
JAVA_PACKAGE="com.game.config"

setup_empty_workspace
"$CLI" -w "$WORK_DIR" project new --template standard --id rt >/dev/null 2>&1
PROJECT_ROOT="$WORK_DIR/projects/rt"

# 导出 JSON 数据
run_cli export data --json
assert_exit 0

# 导出 Java 代码（使用统一 package）
run_cli export code --java --package "$JAVA_PACKAGE"
assert_exit 0

# 生成 TestMain.java（使用同一个 package）
run_cli_raw util gen-test --lang java --format json \
    --schema "$PROJECT_ROOT/project.tblschema" \
    --package "$JAVA_PACKAGE" \
    -o "$PROJECT_ROOT"
assert_exit 0
assert_file_exists "$PROJECT_ROOT/TestMain.java"

# 找到 Java 源文件目录
JAVA_SRC=$(find "$PROJECT_ROOT" -path "*/gen/server/java" -type d | head -1)
if [ -z "$JAVA_SRC" ]; then
    _fail "Java source dir not found"
    end_tests
    exit 1
fi

# 编译（TestMain.java + 所有生成的 Java 类）
JAVA_FILES=$(find "$JAVA_SRC" -name "*.java")
set +e
javac -cp "$JAVA_SRC" $JAVA_FILES "$PROJECT_ROOT/TestMain.java" -d "$PROJECT_ROOT/out" 2>"$PROJECT_ROOT/javac_err.txt"
JAVAC_EXIT=$?
set -e
if [ $JAVAC_EXIT -ne 0 ]; then
    _fail "javac failed: $(cat "$PROJECT_ROOT/javac_err.txt")"
    end_tests
    exit 1
fi
_pass "javac ok"

# 运行
DATA_DIR="$PROJECT_ROOT/gen/server/data/json"
set +e
JAVA_OUT=$(java -cp "$PROJECT_ROOT/out" TestMain "$DATA_DIR" 2>&1)
JAVA_EXIT=$?
set -e
if [ $JAVA_EXIT -ne 0 ]; then
    _fail "java run failed: $JAVA_OUT"
    end_tests
    exit 1
fi
_pass "java run ok"

# 验证输出包含预期数据
if echo "$JAVA_OUT" | grep -q "HeroBase"; then
    _pass "output contains HeroBase"
else
    _fail "output missing HeroBase data"
fi

cleanup
end_tests
