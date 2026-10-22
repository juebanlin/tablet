#!/bin/bash
# tests/09_misc/test_skill_example.sh
#
# skills/tablet 里的示例 schema 必须能被真实 CLI 跑通。
# 技能文档是**给 AI 照抄**的样板：示例漂移 = 每个用户都照着一个错例子写。
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "misc/skill-example"

EXAMPLE="$PROJECT_ROOT/skills/tablet/references/example.tblschema"

if [ ! -f "$EXAMPLE" ]; then
    _fail "缺少示例 schema: $EXAMPLE"
    end_tests
    exit 1
fi

setup_empty_workspace
run_cli project new --from-file "$EXAMPLE" --id demo
assert_exit 0

run_cli --project demo validate
assert_exit 0
assert_contains "验证通过"

# 导出一遍，确保示例里的类型 / 引用 / 预设值真的能走完流水线
run_cli --project demo export data --json -o "$WORK_DIR/out"
assert_exit 0
assert_file_exists "$WORK_DIR/out/json/Hero.json"

cleanup
end_tests
