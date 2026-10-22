#!/bin/bash
# tests/02_schema/test_delete.sh
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "schema/delete"

# ── 测试配置 ──
TEMPLATE="standard"

# hero 组里只有 HeroBase：删掉最后一个节点，组跟着消失
# （否则下次加载会从残留目录里再冒出一个空组）
setup_workspace "$TEMPLATE"
run_cli schema delete-node --group hero --name HeroBase
assert_exit 0
assert_contains "已删除"
if [ -d "$WORK_DIR/projects/test/config/hero" ]; then
    _fail "删空后组目录应被清掉"
else
    _pass "删空后组目录已清理"
fi
# 组已经不在了，再删就是"目标不存在"，必须吵
run_cli schema delete-group --name hero
assert_exit 1
assert_stderr_contains "Group 不存在"
cleanup

# ── 组里还有别的节点：删节点不动组，删组才清目录 ──
setup_workspace "$TEMPLATE"
run_cli schema add-group --name tmpg
assert_exit 0
run_cli schema add-table --group tmpg --name A
assert_exit 0
run_cli schema add-table --group tmpg --name B
assert_exit 0

run_cli schema delete-node --group tmpg --name A
assert_exit 0
assert_dir_exists "$WORK_DIR/projects/test/config/tmpg"

run_cli schema delete-group --name tmpg
assert_exit 0
assert_contains "已删除 Group"
if [ -d "$WORK_DIR/projects/test/config/tmpg" ]; then
    _fail "删组后目录应被清掉"
else
    _pass "删组后目录已清理"
fi
cleanup

end_tests
