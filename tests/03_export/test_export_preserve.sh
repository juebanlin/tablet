#!/bin/bash
# tests/03_export/test_export_preserve.sh
#
# 业务扩展保留区间（@11 §3.4 / @02）。这套契约写错的后果是**静默吃掉用户手写的代码**，
# 所以三条底线都要断言：默认不动产物、开了能留住、留不住时必须吭声。
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "export/preserve-region"

BEGIN_MARK="<TBL-CUSTOM-BEGIN>"
END_MARK="<TBL-CUSTOM-END>"

# 往区间的 BEGIN 标记之后插入一段代码（不依赖 python / cygpath，纯 awk）
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

setup_workspace standard
PROJ="$WORK_DIR/projects/test"
ITEM_TPL="$PROJ/gen/server/java/com/game/config/tpl/ItemTpl.java"

# ── 默认不配 → 产物里不该有标记（「默认关」的验收标准）──
run_cli export code --java
assert_exit 0
if grep -q "$BEGIN_MARK" "$ITEM_TPL"; then
    _fail "默认（未配 custom_regions）不该出现保留区间标记"
else
    _pass "默认产物不含保留区间"
fi

# ── 打开 → 标记出现 ──
run_cli -s export.server.java.custom_regions=true export code --java
assert_exit 0
if grep -q "$BEGIN_MARK" "$ITEM_TPL" && grep -q "$END_MARK" "$ITEM_TPL"; then
    _pass "开启后产物带保留区间"
else
    _fail "开启后应带保留区间"
fi

# ── 往区间里写业务代码 → 重新导出必须原样留住 ──
inject_code "$ITEM_TPL" "    public static int biz() { return 42; }"
if grep -q "biz()" "$ITEM_TPL"; then
    _pass "业务代码已注入"
else
    _fail "注入失败"
fi

run_cli -s export.server.java.custom_regions=true export code --java
assert_exit 0
if grep -q "biz()" "$ITEM_TPL"; then
    _pass "重新导出后业务代码仍在"
else
    _fail "业务代码被覆盖了 —— 这正是本特性要防的事"
fi
# 拼接结果与磁盘上的文件逐字节相同时不该重写（否则每次导出都在动文件）
assert_contains "0 修改"

# ── 关掉开关 → 手写内容会丢，必须警告 ──
run_cli export code --java
assert_exit 0
assert_stderr_contains "未被保留"
if grep -q "biz()" "$ITEM_TPL"; then
    _fail "关掉开关后业务代码不该还在（契约是覆盖）"
else
    _pass "关掉开关后按契约覆盖"
fi

# ── 标记被写坏 → 整文件重新生成，但必须警告 ──
run_cli -s export.server.java.custom_regions=true export code --java
inject_code "$ITEM_TPL" "    public static int biz() { return 42; }"
sed -i "/TBL-CUSTOM-END/d" "$ITEM_TPL"
run_cli -s export.server.java.custom_regions=true export code --java
assert_exit 0
assert_stderr_contains "标记不完整"
if grep -q "$END_MARK" "$ITEM_TPL"; then
    _pass "标记已随整文件重新生成回来"
else
    _fail "重新生成后 END 标记应在"
fi

# ── 区间里只有默认提示时不该刷警告（关掉开关是常事，别每次都吵）──
run_cli -s export.server.java.custom_regions=true export code --java
run_cli export code --java
assert_exit 0
assert_stderr_not_contains "未被保留"

cleanup

end_tests
