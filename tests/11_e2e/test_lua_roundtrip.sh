#!/bin/bash
# tests/11_e2e/test_lua_roundtrip.sh — Lua 端到端：export → lua → 验证输出
#
# Lua 产物是数据字面量，没有加载器可测；这条只守住「生成 .lua 能被 dofile 出来
# 且字段名/值对得上」——字段名曾经从源名被改成过 camelCase 又改回来（@01.8.1）。
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "e2e/lua-roundtrip"

# 注意 find_tool 会连 <name>.exe 一起找：Windows 上 LuaRocks 的
# `C:\Program Files (x86)\Lua\5.1\lua` 是个**目录**，会把同目录的 lua.exe 挡住。
# 装了但没进 PATH 时，用 LUA_BIN=/path/to/lua 指定。
LUA_BIN="$(find_tool LUA_BIN "lua5.4 lua5.3 lua54 lua" "/c/Program Files (x86)/Lua/5.1" "/c/Program Files/Lua/5.1")"
if [ -z "$LUA_BIN" ]; then
    echo "SKIP lua not found（可设 LUA_BIN=/path/to/lua）[e2e/lua-roundtrip]"
    _PASS=1
    end_tests
    exit 0
fi

setup_workspace standard

run_cli export all
assert_exit 0

LUA_DIR="$WORK_DIR/projects/test/gen/client"
assert_file_exists "$LUA_DIR/Item.lua"
assert_file_exists "$LUA_DIR/GameConst.lua"

APP="$(mktemp -d)"
trap 'rm -rf "$APP"; cleanup' EXIT

# 只碰客户端可见的字段。Item.price / GameConst.max_level 都是 `export:服务器`，
# 客户端产物里没有——这两个断言曾经写成服务端的字段，而本测试长期处于 SKIP，
# 一直没暴露（price 是 long、max_level 是 int，写成 %d 遇到 nil 会直接让 lua 报错）。
cat > "$APP/smoke.lua" <<'EOF'
local dir = ...
local Item = dofile(dir .. "/Item.lua")
local GameConst = dofile(dir .. "/GameConst.lua")

local it = Item[2001]
print(string.format("item=%s stack_max=%d weight=%s", it.name, it.stack_max, it.weight))
local n = 0
for _ in pairs(Item) do n = n + 1 end
print(string.format("version=%s default_hero=%d item_count=%d",
    GameConst.version, GameConst.default_hero, n))
EOF

set +e
LUA_OUT=$("$LUA_BIN" "$APP/smoke.lua" "$LUA_DIR" 2>&1)
LUA_EXIT=$?
set -e

if [ $LUA_EXIT -ne 0 ]; then
    _fail "lua 运行失败: $LUA_OUT"
    end_tests
    exit 1
fi
_pass "dofile 生成产物 ok"

# stack_max 同时验证「数据文件 / 内联 key 用源名」（@01.8.1）
if echo "$LUA_OUT" | grep -q "stack_max=99"; then
    _pass "Item.stack_max = 99"
else
    _fail "Item.stack_max 不是 99（key 对不上？）: $LUA_OUT"
fi
if echo "$LUA_OUT" | grep -q "item_count=4"; then
    _pass "Item 有 4 行"
else
    _fail "Item 行数不对: $LUA_OUT"
fi
if echo "$LUA_OUT" | grep -q "version=1.0.0"; then
    _pass "GameConst.version = 1.0.0"
else
    _fail "GameConst 不对: $LUA_OUT"
fi
if echo "$LUA_OUT" | grep -q "weight=0.5"; then
    _pass "Item.weight = 0.5（浮点字面量）"
else
    _fail "Item.weight 不对: $LUA_OUT"
fi

end_tests
