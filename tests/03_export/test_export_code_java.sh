#!/bin/bash
# tests/03_export/test_export_code_java.sh
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "export/code-java"

# ── 测试配置 ──
TEMPLATE="standard"

setup_workspace "$TEMPLATE"
run_cli export code --java
assert_exit 0
assert_contains "Java"
assert_contains "新增"

TPL_DIR="$WORK_DIR/projects/test/gen/server/java/com/game/config/tpl"
ITEM_TPL="$TPL_DIR/ItemTpl.java"
CONST_TPL="$TPL_DIR/GameConstTpl.java"

if [ ! -f "$ITEM_TPL" ]; then
    _fail "未生成 $ITEM_TPL"
    end_tests
    exit 1
fi

# ── 默认开 lombok：注解在，手写 getter 不在 ──
if grep -q "^@Getter$" "$ITEM_TPL"; then
    _pass "默认产物带 @Getter"
else
    _fail "默认应带 @Getter（export.server.java.lombok 默认 true）"
fi
if grep -q "public int getStackMax() { return stackMax; }" "$ITEM_TPL"; then
    _fail "开了 lombok 就不该再手写 getter"
else
    _pass "开了 lombok 不手写 getter"
fi
# getId() 不能跟着一起没了：ITpl 接口声明了它，TplHolder 注册表也在调
if grep -q "public int getId() { return id; }" "$ITEM_TPL"; then
    _pass "保留 getId()（ITpl 要求）"
else
    _fail "getId() 丢了 —— ITpl 声明了它，TplHolder 也在调"
fi
# 常量类同理
if grep -q "^@Getter$" "$CONST_TPL"; then
    _pass "常量类也带 @Getter"
else
    _fail "常量类应带 @Getter"
fi

# ── 关掉之后回到显式 getter，且产物里不该出现 lombok 字样 ──
run_cli -s export.server.java.lombok=false export code --java
assert_exit 0
if grep -q "lombok" "$ITEM_TPL"; then
    _fail "lombok=false 时产物里不该有 lombok"
else
    _pass "lombok=false 无 lombok 字样"
fi
if grep -q "public int getStackMax() { return stackMax; }" "$ITEM_TPL"; then
    _pass "lombok=false 手写 getter 回来了"
else
    _fail "lombok=false 应手写 getter"
fi

# ── 值不合法要告警，不能静默退回默认（那等于悄悄改了产物） ──
run_cli -s export.server.java.lombok=yes export code --java
assert_stderr_contains "只接受 true / false"

cleanup

end_tests
