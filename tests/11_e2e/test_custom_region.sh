#!/bin/bash
# tests/11_e2e/test_custom_region.sh — 保留区间：注入业务代码 → 重新导出 → 产物仍能编译/解析
#
# 03_export 那条测的是「内容留住了」；这条测的是**留住的东西放进生成代码里仍然合法**。
# 两者的区别很要紧：标记落在错误的语法位置（比如插进表达式中间）时，字符串断言照样全绿，
# 但产物已经编不过了 —— 这个仓库在这类「断言全绿、产物不可用」上栽过两次（@11 §3.3）。
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "e2e/custom-region"

BEGIN_MARK="<TBL-CUSTOM-BEGIN>"
END_MARK="<TBL-CUSTOM-END>"

inject_code() {
    local file="$1" code="$2"
    printf '%s\n' "$code" > "$file.code"
    awk -v codefile="$file.code" '
        { print }
        /TBL-CUSTOM-BEGIN/ { while ((getline line < codefile) > 0) print line }
    ' "$file" > "$file.new"
    mv "$file.new" "$file"
    rm -f "$file.code"
}

# 每个生成的类型文件里，标记必须**恰好一对**：多一个说明落点错了
assert_single_region() {
    local file="$1" tag="$2"
    local b e
    b=$(grep -c "$BEGIN_MARK" "$file" 2>/dev/null || echo 0)
    e=$(grep -c "$END_MARK" "$file" 2>/dev/null || echo 0)
    if [ "$b" = "1" ] && [ "$e" = "1" ]; then
        _pass "[$tag] 标记恰好一对"
    else
        _fail "[$tag] 标记应恰好一对，实际 BEGIN=$b END=$e（$file）"
    fi
}

setup_empty_workspace
"$CLI" -w "$WORK_DIR" project new --template standard --id rt >/dev/null 2>&1
PROJ="$WORK_DIR/projects/rt"
JAVA_PKG_PATH="com/game/config/tpl"

# 四个后端一起开（C# 走 dotnet 段）
CR="-s export.server.java.custom_regions=true"
CR="$CR -s export.server.go.custom_regions=true"
CR="$CR -s export.server.cpp.custom_regions=true"
CR="$CR -s export.server.csharp_dotnet.custom_regions=true"
# lombok 关掉：这条用例只关心保留区间，别把 lombok 的依赖拖进来
run_cli $CR -s export.server.java.lombok=false export code --java --go --cpp --csharp
assert_exit 0

# ── 落点检查：四个后端各挑一个类型文件 ──
assert_single_region "$PROJ/gen/server/java/$JAVA_PKG_PATH/ItemTpl.java" "java/table"
assert_single_region "$PROJ/gen/server/java/$JAVA_PKG_PATH/GameConstTpl.java" "java/constant"
assert_single_region "$PROJ/gen/server/java/$JAVA_PKG_PATH/HeroTypeEnum.java" "java/enum"

GO_ITEM=$(find "$PROJ/gen/server/go" -name "item*.go" | head -1)
CPP_ITEM=$(find "$PROJ/gen/server/cpp" -name "ItemTpl.h" | head -1)
CS_ITEM=$(find "$PROJ/gen/server/csharp" -name "ItemTpl.cs" | head -1)
[ -n "$GO_ITEM" ] && assert_single_region "$GO_ITEM" "go/table"
[ -n "$CPP_ITEM" ] && assert_single_region "$CPP_ITEM" "cpp/table"
[ -n "$CS_ITEM" ] && assert_single_region "$CS_ITEM" "csharp/table"

# ── Java：注入一个真正用到生成字段的方法，重新导出后整体编译 ──
ITEM_TPL="$PROJ/gen/server/java/$JAVA_PKG_PATH/ItemTpl.java"
inject_code "$ITEM_TPL" "    public String bizLabel() { return \"item-\" + id + \"-\" + name; }"

JAVA_ARGS="-s export.server.java.custom_regions=true -s export.server.java.lombok=false"
run_cli $JAVA_ARGS export code --java
assert_exit 0
if grep -q "bizLabel()" "$ITEM_TPL"; then
    _pass "java: 注入的方法留存"
else
    _fail "java: 注入的方法丢了"
fi

if ! command -v javac >/dev/null 2>&1; then
    echo -e "${YELLOW}SKIP${NC} javac not found，跳过 Java 编译 [e2e/custom-region]"
    _PASS=$((_PASS + 1))
else
    JAVA_SRC="$PROJ/gen/server/java"
    set +e
    javac -cp "$JAVA_SRC" $(find "$JAVA_SRC" -name "*.java") -d "$PROJ/out" 2>"$PROJ/javac_err.txt"
    JAVAC_EXIT=$?
    set -e
    if [ $JAVAC_EXIT -eq 0 ]; then
        _pass "java: 保留区间 + 生成代码整体 javac 通过"
    else
        _fail "java: javac 失败 —— 区间落点可能不在类体内: $(head -5 "$PROJ/javac_err.txt")"
    fi
fi

# ── Go：注入一个文件作用域的方法（Go 的方法必须这么写，落点正是为此选的）──
if [ -n "$GO_ITEM" ]; then
    inject_code "$GO_ITEM" "func (t *ItemTpl) BizLabel() string { return \"item\" }"
    run_cli -s export.server.go.custom_regions=true export code --go
    assert_exit 0
    if grep -q "BizLabel" "$GO_ITEM"; then
        _pass "go: 注入的方法留存"
    else
        _fail "go: 注入的方法丢了"
    fi

    if ! command -v gofmt >/dev/null 2>&1; then
        echo -e "${YELLOW}SKIP${NC} gofmt not found，跳过 Go 解析检查 [e2e/custom-region]"
        _PASS=$((_PASS + 1))
    else
        set +e
        gofmt -e "$GO_ITEM" >/dev/null 2>"$PROJ/gofmt_err.txt"
        GOFMT_EXIT=$?
        set -e
        if [ $GOFMT_EXIT -eq 0 ]; then
            _pass "go: 保留区间 + 生成代码 gofmt 解析通过"
        else
            _fail "go: gofmt 解析失败（落点可能不在文件作用域）: $(head -3 "$PROJ/gofmt_err.txt")"
        fi
    fi
fi

# C++ / C# 只做落点检查：它们的完整编译要 nlohmann / dotnet，
# 已有的 cpp / csharp roundtrip 覆盖了基础产物的编译，这里不重复

cleanup
end_tests
