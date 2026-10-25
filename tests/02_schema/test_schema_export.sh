#!/bin/bash
# tests/02_schema/test_schema_export.sh
#
# schema export：项目结构 → .tblschema；支持分组 / 节点选择、带 preset、另存为本地模板。
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "schema/export"

setup_workspace standard

# 全部节点
run_cli --project test schema export --out "$WORK_DIR/all.tblschema"
assert_exit 0
assert_file_exists "$WORK_DIR/all.tblschema"
assert_file_contains "$WORK_DIR/all.tblschema" "\[hero/HeroBase\]"
assert_file_contains "$WORK_DIR/all.tblschema" "\[item/Item\]"
assert_file_not_contains "$WORK_DIR/all.tblschema" "# @preset"

# 按分组
run_cli --project test schema export --out "$WORK_DIR/hero.tblschema" --group hero
assert_exit 0
assert_file_contains "$WORK_DIR/hero.tblschema" "\[hero/HeroBase\]"
assert_file_not_contains "$WORK_DIR/hero.tblschema" "\[item/Item\]"

# 按节点
run_cli --project test schema export --out "$WORK_DIR/one.tblschema" --node hero/HeroBase
assert_exit 0
assert_file_contains "$WORK_DIR/one.tblschema" "\[hero/HeroBase\]"
assert_file_not_contains "$WORK_DIR/one.tblschema" "\[hero/HeroType\]"

# 带 preset
run_cli --project test schema export --out "$WORK_DIR/preset.tblschema" --group hero --with-preset
assert_exit 0
assert_file_contains "$WORK_DIR/preset.tblschema" "# @preset"

# 另存为本地模板：从 JSON 拿到实际路径并清理
run_cli --project test --fmt json schema export --as-template --meta-id tablet_test_tpl
assert_exit 0
assert_contains '"template": true'
TPL_PATH=$(echo "$LAST_STDOUT" | sed -n 's/.*"path": "\([^"]*\)".*/\1/p')
assert_file_exists "$TPL_PATH"
rm -f "$TPL_PATH"

cleanup
end_tests
