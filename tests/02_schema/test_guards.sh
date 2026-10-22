#!/bin/bash
# tests/02_schema/test_guards.sh — schema 写命令的「先校验、再看结果」
#
# 守的是一类洞：命令**看起来成功**，实际什么都没做、或者做坏了。
# CLI 主要由 AI / 脚本驱动，没有反馈等于没有发生（@02.26.7）。
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "schema/guards"

TEMPLATE="standard"

# ── 撞名会把同名 .tbl 整个覆盖掉（以前就是这样，且退出码 0） ──
setup_workspace "$TEMPLATE"
before=$(wc -l < "$WORK_DIR/projects/test/config/hero/HeroBase.tbl")
run_cli schema add-table --group hero --name HeroBase
assert_exit 1
assert_stderr_contains "重复"
after=$(wc -l < "$WORK_DIR/projects/test/config/hero/HeroBase.tbl")
if [ "$before" -eq "$after" ]; then _pass "原表未被覆盖"; else _fail "原表被覆盖: $before 行 → $after 行"; fi
cleanup

# ── 非法节点名：生成不出合法类名，以前照样"已添加" ──
setup_workspace "$TEMPLATE"
run_cli schema add-table --group hero --name bad_name
assert_exit 1
assert_stderr_contains "Java类名规则"
cleanup

# ── 非法 / 重复组名 ──
setup_workspace "$TEMPLATE"
run_cli schema add-group --name "bad group!"
assert_exit 1
assert_stderr_contains "组名"
run_cli schema add-group --name hero
assert_exit 1
assert_stderr_contains "重复"
cleanup

# ── 改名 / 删除不存在的目标：以前是静默 no-op + "已重命名"/"已删除" ──
setup_workspace "$TEMPLATE"
run_cli schema rename-node --group hero --old Nope --new Other
assert_exit 1
assert_stderr_contains "节点不存在"
run_cli schema delete-node --group hero --name Nope
assert_exit 1
assert_stderr_contains "节点不存在"
run_cli schema delete-group --name nope
assert_exit 1
assert_stderr_contains "Group 不存在"
run_cli schema rename-group --old nope --new other
assert_exit 1
assert_stderr_contains "Group 不存在"
cleanup

# ── add-enum 必须带条目：空枚举过不了校验，也就落不了盘 ──
setup_workspace "$TEMPLATE"
run_cli schema add-enum --group hero --name Rarity
assert_exit 2                     # clap：缺少必填参数
run_cli schema add-enum --group hero --name Rarity --entry 1:COMMON:普通
assert_exit 0
assert_contains "已添加 Enum"
cleanup

# ── add-group 建的空组要能跨进程活下来（CLI 每条命令一个进程） ──
setup_workspace "$TEMPLATE"
run_cli schema add-group --name newgroup
assert_exit 0
assert_dir_exists "$WORK_DIR/projects/test/config/newgroup"
run_cli schema add-table --group newgroup --name TestTable
assert_exit 0
assert_file_exists "$WORK_DIR/projects/test/config/newgroup/TestTable.tbl"
cleanup

end_tests
