#!/bin/bash
# tests/02_schema/test_xlsx_group.sh — Xlsx 组（一组一个工作簿，节点 = sheet）
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "schema/xlsx-group"

TEMPLATE="standard"

# ── 建组：目录里应当出现 <group>.xlsx，而不是一堆 .tbl ──
setup_workspace "$TEMPLATE"
run_cli schema add-group --name xg --storage xlsx
assert_exit 0
assert_contains "xg"
assert_file_exists "$WORK_DIR/projects/test/config/xg/xg.xlsx"
if ls "$WORK_DIR/projects/test/config/xg/"*.tbl >/dev/null 2>&1; then
    _fail "xlsx 组不该有 .tbl 文件"
else
    _pass "xlsx 组没有 .tbl 文件"
fi

# ── 加 Table / Constant / Enum 都落成 sheet（不落 .tbl） ──
run_cli schema add-table --group xg --name Hot
assert_exit 0
run_cli schema add-constant --group xg --name Cfg
assert_exit 0
run_cli schema add-enum --group xg --name Kind --entry 1:A:甲 --entry 2:B:乙
assert_exit 0

run_cli schema show --fmt json
assert_exit 0
assert_contains '"name": "Hot"'
assert_contains '"name": "Cfg"'
assert_contains '"name": "Kind"'
if ls "$WORK_DIR/projects/test/config/xg/"*.tbl >/dev/null 2>&1; then
    _fail "xlsx 组不该写出 .tbl"
else
    _pass "节点都落在工作簿里"
fi

# ── 校验 / 导出与 tbl 组同一条流水线 ──
run_cli validate
assert_exit 0
assert_contains "验证通过"

run_cli export data --json -o "$WORK_DIR/out"
assert_exit 0
assert_file_exists "$WORK_DIR/out/json/Hot.json"

# ── 干净的组再保存：一个字节都不许碰 ──
before=$(sha256sum "$WORK_DIR/projects/test/config/xg/xg.xlsx" | cut -d' ' -f1)
run_cli workspace save
assert_exit 0
after=$(sha256sum "$WORK_DIR/projects/test/config/xg/xg.xlsx" | cut -d' ' -f1)
if [ "$before" = "$after" ]; then
    _pass "干净保存没有重写工作簿"
else
    _fail "干净保存把 workbook 重写了"
fi

# ── 加载：重新读回来，节点还在 ──
run_cli project info
assert_exit 0
assert_contains "Tables:    4"  # standard 自带 3 张 + Hot

cleanup

# ── 非法的 --storage 必须报错 ──
setup_workspace "$TEMPLATE"
run_cli schema add-group --name bad --storage parquet
assert_exit 1
assert_stderr_contains "tbl / xlsx"
cleanup

end_tests
