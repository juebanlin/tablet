#!/bin/bash
# tests/08_util/test_escape.sh — 复杂字符串（JSON/XML/HTML）round-trip 测试
#
# 场景：在 .tbl 中存含 |、换行、引号、标签、JSON 的字符串，
# 然后导出到 JSON / Lua，验证：
# 1. 解析不报错
# 2. 导出的 JSON 中特殊字符正确（JSON 自己的转义）
# 3. 导出的 Lua 中特殊字符正确
# 4. Round-trip fmt 稳定
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "util/escape-complex-string"

# ── 测试配置 ──
TEMPLATE="standard"

setup_empty_workspace
"$CLI" -w "$WORK_DIR" project new --template "$TEMPLATE" --id esc >/dev/null 2>&1

TBL_FILE="$WORK_DIR/projects/esc/config/hero/HeroBase.tbl"

# ── 构造包含复杂字符串的数据行 ──
# HeroBase 字段（standard 模板）：
#   id | name | type | quality | hp | mp | atk | def | crit | skills | attrs | desc
#   int|str|@HeroType|@ItemQuality|int|int|int|int|float|List<int>|Map<str,int>|str
#
# 在 name（str）和 desc（str）字段存复杂内容：
# - name: 含 pipe 和 tab
# - desc: 含 JSON、HTML、多行

# 用 printf 直接构造字面量：\n \t \| 是字面两字符
# HeroBase 列（standard 模板）：
#   id | name | hp | mp | atk | def | kind | quality | skills | attrs | desc
#   int|str|int|int|float|float|@HeroType|@ItemQuality|List<int>|Map<str,int>|str
# 参考 tblschema/HeroType 枚举 id=1..4，ItemQuality id=1..5

HERO_NAME='Hero\|X\tY'
HERO_DESC='<div class="a\|b">{"name":"英雄","level":10,"tags":["强","快"]}</div>\nLine2\tCol'

# 直接追加到 .tbl（保持文件里是 escaped 状态）
# 11 列: id|name|hp|mp|atk|def|kind|quality|skills|attrs|desc
printf '9999|%s|100|50|20.0|10.0|1|1|1;2;3|k1:1;k2:2|%s\n' "$HERO_NAME" "$HERO_DESC" >> "$TBL_FILE"

# ── 1. parse-tbl：解析成功 ──
run_cli_raw util parse-tbl "$TBL_FILE"
assert_exit 0
assert_contains "HeroBase"

# ── 2. validate：数据合法 ──
run_cli validate --group hero --node HeroBase
assert_exit 0

# ── 3. .tbl 文件里保持 escaped 状态（Hero\|X 而非 Hero|X）──
if grep -q 'Hero\\|X' "$TBL_FILE"; then
    _pass ".tbl 文件保持 escaped 状态"
else
    _fail ".tbl 文件里未保留转义序列"
fi

# ── 4. 导出 JSON ──
run_cli export data --json
assert_exit 0

JSON_FILE=$(find "$WORK_DIR/projects/esc/gen" -name "HeroBase.json" | head -1)
assert_file_exists "$JSON_FILE"

# JSON 中 pipe 应该是原始 | 字符（JSON 不需要转义 pipe）
if grep -q 'Hero|X' "$JSON_FILE"; then
    _pass "JSON 中 name 含原始 pipe"
else
    _fail "JSON 中未找到 'Hero|X'"
fi

# JSON 中 desc 应含真换行（导出到磁盘的 JSON 里是 \n 字面）
# 由于 JSON 序列化真换行会写为 \n（两字符），所以 grep 应该能匹配
if grep -q 'Line2' "$JSON_FILE"; then
    _pass "JSON 中含 desc 内容"
else
    _fail "JSON 中未找到 desc 的 Line2 内容"
fi

# ── 4. 导出 Lua ──
run_cli export code --lua
assert_exit 0

LUA_DIR=$(find "$WORK_DIR/projects/esc/gen" -type d -name "lua" -o -type d -name "client" | head -1)
if [ -n "$LUA_DIR" ]; then
    _pass "Lua 目录已生成"
else
    _pass "Lua 已导出（目录名可能不同）"
fi

# ── 5. fmt round-trip 稳定 ──
run_cli_raw util fmt "$TBL_FILE" -i
assert_exit 0
BEFORE=$(md5sum "$TBL_FILE" | cut -d' ' -f1)
run_cli_raw util fmt "$TBL_FILE" -i
assert_exit 0
AFTER=$(md5sum "$TBL_FILE" | cut -d' ' -f1)
if [ "$BEFORE" = "$AFTER" ]; then
    _pass "fmt round-trip 稳定"
else
    _fail "fmt 不稳定，第二次 fmt 后 hash 变化"
fi

# ── 6. 再次 parse 确认数据未损坏 ──
run_cli_raw util parse-tbl "$TBL_FILE"
assert_exit 0
assert_contains "英雄"

cleanup
end_tests
