#!/bin/bash
# tests/01_project/test_import.sh — 原生 .tbl / .xlsx 导入 + 模板驱动 Excel 映射
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "project/import"

setup_workspace "standard"
PROJECT_ROOT="$WORK_DIR/projects/test"
MANIFEST="$PROJECT_ROOT/project.tblmanifest"
EXT="$WORK_DIR/ext"
mkdir -p "$EXT/hero"

# ── 1. .tbl 文件导入：文件名 = 节点名，目录名 = 组名 ──
cat > "$EXT/hero/Imported.tbl" <<'TBL'
#!tbl v3
#mode table
---

@field id
  desc:ID
  export:cs
  type:int

@field name
  desc:名称
  export:cs
  type:str

[1]
  name:战士
TBL

run_cli project import-tbl --id test --group imported "$EXT/hero/Imported.tbl"
assert_exit 0
assert_file_exists "$PROJECT_ROOT/config/imported/Imported.tbl"
assert_file_contains "$MANIFEST" "Imported"
run_cli schema show
assert_contains "Imported"

# 同名节点再次导入必须失败，不自动 _copy
run_cli project import-tbl --id test "$EXT/hero/Imported.tbl"
assert_exit 1

# 目录导入：组名取目录名，一次多个 .tbl
mkdir -p "$EXT/pack2"
cp "$EXT/hero/Imported.tbl" "$EXT/pack2/A.tbl"
cp "$EXT/hero/Imported.tbl" "$EXT/pack2/B.tbl"
run_cli project import-tbl --id test "$EXT/pack2/A.tbl" "$EXT/pack2/B.tbl"
assert_exit 0
assert_file_exists "$PROJECT_ROOT/config/pack2/A.tbl"
assert_file_exists "$PROJECT_ROOT/config/pack2/B.tbl"
assert_file_contains "$MANIFEST" "pack2"

# ── 2. xlsx 原生导入：文件 = 组，sheet = 节点 ──
cat > "$EXT/ImportedX.tbl" <<'TBL'
#!tbl v3
#mode table
---

@field id
  desc:ID
  export:cs
  type:int

@field name
  desc:名称
  export:cs
  type:str

[1]
  name:弓手
TBL
run_cli util tbl-to-xlsx "$EXT/ImportedX.tbl" -o "$EXT/pack.xlsx"
assert_exit 0

run_cli project import-xlsx --id test "$EXT/pack.xlsx"
assert_exit 0
assert_file_exists "$PROJECT_ROOT/config/pack.xlsx"
if cmp -s "$EXT/pack.xlsx" "$PROJECT_ROOT/config/pack.xlsx"; then
    _pass "xlsx 原样复制（样式字节不变）"
else
    _fail "xlsx 导入改写了文件"
fi
assert_file_contains "$MANIFEST" "table xlsx"
run_cli schema show --rules
assert_contains "ImportedX"

# ── 3. 模板驱动映射：任意 Excel → .tbl ──
cat > "$EXT/hero/HeroX.tbl" <<'TBL'
#!tbl v3
#mode table
---

@field id
  desc:ID
  export:cs
  type:int

@field name
  desc:名称
  export:cs
  type:str

[7]
  name:法师
TBL
run_cli util tbl-to-xlsx "$EXT/hero/HeroX.tbl" -o "$EXT/foreign.xlsx"
assert_exit 0

cat > "$EXT/map.tblschema" <<'SCHEMA'
#!tblschema v1

[hero/HeroX] table
id | int | cs | ID
name | str | cs | 名称
SCHEMA
run_cli util xlsx-to-tbl "$EXT/foreign.xlsx" --schema "$EXT/map.tblschema" -o "$EXT/mapped"
assert_exit 0
assert_file_exists "$EXT/mapped/HeroX.tbl"
assert_file_contains "$EXT/mapped/HeroX.tbl" "name:法师"

# ── 4. 模板里的 xlsx section：新建项目时直接生成工作簿 ──
cat > "$EXT/xlsx_tpl.tblschema" <<'SCHEMA'
#!tblschema v1
[building/Building] table xlsx
id | int | cs | ID
name | str | cs | 名称
# @preset
1 | 主城
SCHEMA
run_cli project new --from-file "$EXT/xlsx_tpl.tblschema" --id xp --switch-after false
assert_exit 0
assert_file_exists "$WORK_DIR/projects/xp/config/building.xlsx"
assert_file_contains "$WORK_DIR/projects/xp/project.tblmanifest" "table xlsx"

cleanup
end_tests
