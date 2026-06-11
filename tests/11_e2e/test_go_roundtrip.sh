#!/bin/bash
# tests/11_e2e/test_go_roundtrip.sh — Go 端到端：export → go run → 验证输出
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "e2e/go-roundtrip"

# 检查 Go 环境
if ! command -v go &>/dev/null; then
    echo -e "${YELLOW}SKIP${NC} go not found [e2e/go-roundtrip]"
    _PASS=1
    end_tests
    exit 0
fi

# ── 配置变量（所有 CLI 调用统一引用）──
GO_PACKAGE="config"
GO_CODE_OUTPUT="gen/server/go"

setup_empty_workspace
"$CLI" -w "$WORK_DIR" project new --template standard --id rt >/dev/null 2>&1
PROJECT_ROOT="$WORK_DIR/projects/rt"

# 导出 JSON 数据
run_cli export data --json
assert_exit 0

# 导出 Go 代码（使用统一 package）
run_cli export code --go --package "$GO_PACKAGE"
assert_exit 0

# 生成 Go test main（使用同一套 package + code-output）
run_cli_raw util gen-test --lang go --format json \
    --schema "$PROJECT_ROOT/project.tblschema" \
    --package "$GO_PACKAGE" \
    --code-output "$GO_CODE_OUTPUT" \
    -o "$PROJECT_ROOT"
assert_exit 0
assert_file_exists "$PROJECT_ROOT/test_main_go/main.go"
assert_file_exists "$PROJECT_ROOT/go.mod"

# 运行 Go 测试（从 project root 执行，go.mod 在此）
DATA_DIR="$PROJECT_ROOT/gen/server/data/json"
cd "$PROJECT_ROOT"
set +e
GO_OUT=$(go run ./test_main_go "$DATA_DIR" 2>&1)
GO_EXIT=$?
set -e
cd - >/dev/null

if [ $GO_EXIT -ne 0 ]; then
    _fail "go run failed: $GO_OUT"
    end_tests
    exit 1
fi
_pass "go run ok"

# 验证输出包含预期数据
if echo "$GO_OUT" | grep -q "HeroBase"; then
    _pass "output contains HeroBase"
else
    _fail "output missing HeroBase data"
fi

cleanup
end_tests
