#!/bin/bash
# tests/02_schema/test_add_enum.sh
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "schema/add-enum"

# ── 测试配置 ──
TEMPLATE="standard"

setup_workspace "$TEMPLATE"
# 条目必须同一条命令给全：空枚举过不了校验，也就落不了盘（见 test_schema_guards.sh）
run_cli schema add-enum --group hero --name HeroRarity \
    --entry 1:COMMON:普通 --entry 2:RARE:稀有 --entry 3:EPIC:史诗
assert_exit 0
assert_contains "已添加 Enum"
cleanup

# ── 缺 --entry：clap 直接拒绝（不是"添加成功但什么都没写"） ──
setup_workspace "$TEMPLATE"
run_cli schema add-enum --group hero --name HeroRarity
assert_exit 2
cleanup

end_tests
