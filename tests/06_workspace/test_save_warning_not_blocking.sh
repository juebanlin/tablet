#!/bin/bash
# tests/06_workspace/test_save_warning_not_blocking.sh
# 警告不阻断保存；错误拦保存（@01.10.7）。
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "workspace/save-warning"

WARN=$(mktemp)
cat > "$WARN" <<'EOF'
#!tblschema v1
# @meta id: saverule
# @meta has_preset: true

[error/ErrorCode] enum
# @rule: name = ref_exists(Lang.key) [warn]
# @preset
10001|PLAYER_NOT_FOUND|玩家不存在
EOF

setup_empty_workspace
run_cli project new --from-file "$WARN" --id test
assert_exit 0

# 只有警告 → 保存必须成功
run_cli workspace save
assert_exit 0

# 该项目确实有警告
run_cli validate
assert_contains "[警告]"

cleanup
rm -f "$WARN"
end_tests
