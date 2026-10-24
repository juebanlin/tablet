#!/bin/bash
# tests/04_validate/test_validate_warning_level.sh
# 节点的 warn 级规则只提示、不失败；--strict 才把警告算失败。
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "validate/warning-level"

SCHEMA=$(mktemp)
cat > "$SCHEMA" <<'EOF'
#!tblschema v1
# @meta id: warnlv
# @meta name: 警告级绑定
# @meta has_preset: true

[error/ErrorCode] enum
# @rule: name = ref_exists(Lang.key) [warn]
# @preset
10001|PLAYER_NOT_FOUND|玩家不存在
10002|PARAM_INVALID|参数错误
EOF

setup_empty_workspace
# Lang 还不存在 → 只该报警告。建项目不能被警告拦住。
run_cli project new --from-file "$SCHEMA" --id test
assert_exit 0

run_cli validate
assert_exit 0
assert_contains "条警告"
assert_contains "[警告]"
assert_contains "Lang"

# --strict：警告也算失败
run_cli validate --strict
assert_exit 1

# JSON 同时给出 errors / warnings
run_cli validate --fmt json
assert_exit 0
assert_contains "\"warnings\""
assert_contains "\"severity\": \"warning\""

# ── 项目级策略：把 warning 改成 block，同一条警告开始失败 ──
cat > "$WORK_DIR/projects/test/project.toml" <<'EOF'
[validate]
error = "block"
warning = "block"
info = "ignore"
EOF
run_cli validate
assert_exit 1
assert_contains "[警告]"

# 再把 warning 改成 ignore：什么都不报，也照常通过
cat > "$WORK_DIR/projects/test/project.toml" <<'EOF'
[validate]
error = "block"
warning = "ignore"
info = "ignore"
EOF
run_cli validate
assert_exit 0
if [ "${LAST_STDOUT#*条警告}" = "$LAST_STDOUT" ]; then
    _pass "warning=ignore 时不产生警告"
else
    _fail "warning=ignore 时不该再有警告:
$LAST_STDOUT"
fi

cleanup
rm -f "$SCHEMA"
end_tests
