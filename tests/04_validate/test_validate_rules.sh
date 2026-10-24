#!/bin/bash
# tests/04_validate/test_validate_rules.sh
# 节点级规则（@01.10.7）：unique() 的重复值、ref_exists() 的缺失值都要拦住。
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "validate/rules"

# ── unique()：key 列重复 → 建项目被拦 ──
DUP=$(mktemp)
cat > "$DUP" <<'EOF'
#!tblschema v1
# @meta id: uniqbad
# @meta has_preset: true

[lang/Lang] table
id | int | cs | ID
key | str | cs | 英文 key
# @rule: key = unique()
# @preset
1|DUP_KEY
2|DUP_KEY
3|OTHER
EOF

setup_empty_workspace
run_cli project new --from-file "$DUP" --id test
assert_exit 1
assert_stderr_contains "重复"

cleanup
rm -f "$DUP"

# ── 同一份 schema 去掉重复值 → 通过 ──
OK=$(mktemp)
cat > "$OK" <<'EOF'
#!tblschema v1
# @meta id: uniook
# @meta has_preset: true

[lang/Lang] table
id | int | cs | ID
key | str | cs | 英文 key
# @rule: key = unique()
# @preset
1|A_KEY
2|B_KEY
EOF

setup_empty_workspace
run_cli project new --from-file "$OK" --id test
assert_exit 0
run_cli validate
assert_exit 0
cleanup
rm -f "$OK"

# ── ref_exists()：目标存在但缺 key → error 级拦住 ──
MISS=$(mktemp)
cat > "$MISS" <<'EOF'
#!tblschema v1
# @meta id: bindbad
# @meta has_preset: true

[lang/Lang] table
id | int | cs | ID
key | str | cs | 英文 key
# @preset
1|A_KEY

[error/ErrorCode] enum
# @rule: name = ref_exists(Lang.key)
# @preset
10001|A_KEY|存在的
10002|NOT_THERE|缺失的
EOF

setup_empty_workspace
run_cli project new --from-file "$MISS" --id test
assert_exit 1
assert_stderr_contains "NOT_THERE"
cleanup
rm -f "$MISS"

# ── constant 段不接受 # @rule ──
CONST=$(mktemp)
cat > "$CONST" <<'EOF'
#!tblschema v1
# @meta id: construle
# @meta has_preset: true

[global/Const] constant
# @rule: name = unique()
# @preset
max_level|int|100|cs|等级上限
EOF

setup_empty_workspace
run_cli project new --from-file "$CONST" --id test
assert_exit 1
assert_stderr_contains "constant 段不支持"
cleanup
rm -f "$CONST"

end_tests
