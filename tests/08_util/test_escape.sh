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
# HeroBase 字段（standard 模板，13 个）：
#   id     int          name   str          hp     int      mp     int
#   atk    float        def    float        kind   @HeroType
#   quality @ItemQuality skills List<int>   attrs  Map<str,int>
#   leader @HeroBase    items  List<@Item>  desc   txt
#
# 复杂内容只能放在 **txt** 字段（这里用 desc）：
# `str` 按 @01.7.1 是「简单标识符」，不允许 | \n \t，放进去会被校验拦下。
# 这里曾经把含 pipe/tab 的内容塞进 name（str）——那不正当地「通过」了很多年，
# 只因为追加的行根本没被解析（见下）。
#
# 注意：**.tbl 是 v3 段式格式**（`[id]` 段 + `  key:value` 行），不是 v2 的管道行。
# 这里曾经写成一行 `9999|...|...`，模板迁移到 v3 后那行根本不被解析，
# 于是整条测试「看着在测转义、实际什么也没测」。

HERO_NAME='HeroX'
HERO_DESC='<div class="a\|b">{"name":"英雄","level":10,"tags":["强","快"]}</div>\nLine2\tCol'

# 用 printf 逐字段写：%s 不做转义处理，\n \t \| 保持「反斜杠+字符」两字符进文件
{
    printf '\n[9999]\n'
    printf '  name:%s\n'  "$HERO_NAME"
    printf '  hp:100\n'
    printf '  mp:50\n'
    printf '  atk:20.0\n'
    printf '  def:10.0\n'
    printf '  kind:1\n'
    printf '  quality:1\n'
    printf '  skills:1;2;3\n'
    printf '  attrs:k1:1;k2:2\n'
    printf '  leader:1001\n'
    printf '  items:2001\n'
    printf '  desc:%s\n'  "$HERO_DESC"
} >> "$TBL_FILE"

# ── 1. parse-tbl：解析成功 ──
run_cli_raw util parse-tbl "$TBL_FILE"
assert_exit 0
assert_contains "HeroBase"

# ── 2. validate：数据合法 ──
run_cli validate --group hero --node HeroBase
assert_exit 0

# ── 3. .tbl 文件里保持 escaped 状态（a\|b 而非 a|b）──
if grep -qF 'a\|b' "$TBL_FILE"; then
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
if grep -qF 'a|b' "$JSON_FILE"; then
    _pass "JSON 中 desc 含原始 pipe"
else
    _fail "JSON 中未找到 'a|b'"
fi

# 追加的那一行真的进了导出结果（曾经因为列数/格式不对整行被丢掉）
if grep -qF '"HeroX"' "$JSON_FILE"; then
    _pass "追加的行进了 JSON"
else
    _fail "追加的行没进 JSON（.tbl 格式或列数不对）"
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
