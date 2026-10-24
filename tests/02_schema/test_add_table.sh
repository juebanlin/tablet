#!/bin/bash
# tests/02_schema/test_add_table.sh
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "schema/add-table"

# ── 测试配置 ──
TEMPLATE="standard"

setup_workspace "$TEMPLATE"

# 添加到已有 group
run_cli schema add-table --group hero --name HeroSkill
assert_exit 0
assert_contains "已添加 Table"

# group 不存在 = 用错命令，必须非零退出并说清楚。
# 以前这里 exit 0 且 println"已添加 Table" —— core 对不存在的 group 是静默 no-op，
# 而 CLI 照样对外宣布成功（@02.26.7「读不懂就报错」同一个道理）。
run_cli schema add-table --group nonexist --name Foo
assert_exit 1
assert_stderr_contains "Group 不存在"

# ── 新建 Table 可选 v2（v3 默认） ──
run_cli schema add-table --group hero --name HeroV2 --tbl-fmt v2
assert_exit 0
HERO_V2="$WORK_DIR/projects/test/config/hero/HeroV2.tbl"
assert_file_exists "$HERO_V2"
if head -n 1 "$HERO_V2" | grep -q '#!tbl v2'; then
    _pass "v2 表头正确"
else
    _fail "HeroV2.tbl 不是 v2 格式:
$(head -n 2 "$HERO_V2")"
fi

# 默认仍是 v3
run_cli schema add-table --group hero --name HeroV3
assert_exit 0
if head -n 1 "$WORK_DIR/projects/test/config/hero/HeroV3.tbl" | grep -q '#!tbl v3'; then
    _pass "默认 v3"
else
    _fail "默认格式不是 v3"
fi

# 非法版本号
run_cli schema add-table --group hero --name BadFmt --tbl-fmt v9
assert_exit 1
assert_stderr_contains "v2 / v3"

cleanup
end_tests
