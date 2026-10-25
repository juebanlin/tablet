#!/bin/bash
# tests/02_schema/test_rules_roundtrip.sh
# # @rule 指令写进 project.tblmanifest，保存后仍在（规则只住 schema，@01.10.7）。
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "schema/rules-roundtrip"

SCHEMA=$(mktemp)
cat > "$SCHEMA" <<'EOF'
#!tblschema v1
# @meta id: ruleround
# @meta has_preset: true

[lang/Lang] table
id | int | cs | ID
key | str | cs | 英文 key
# @rule: key = unique()
# @preset
1|A_KEY
2|B_KEY

[error/ErrorCode] enum
# @rule: name = ref_exists(Lang.key) [warn]
# @preset
10001|A_KEY|存在的
EOF

setup_empty_workspace
run_cli project new --from-file "$SCHEMA" --id test
assert_exit 0

SCHEMA_FILE="$WORK_DIR/projects/test/project.tblmanifest"
assert_file_exists "$SCHEMA_FILE"

if ! grep -q '^# @rule: key = unique()$' "$SCHEMA_FILE"; then
    _fail "# @rule: unique() 没写进 project.tblmanifest:\n$(cat "$SCHEMA_FILE")"
else
    _pass "unique 规则已写入"
fi

if ! grep -q '^# @rule: name = ref_exists(Lang.key) \[warn\]$' "$SCHEMA_FILE"; then
    _fail "# @rule: ref_exists + [warn] 没写进 project.tblmanifest:\n$(cat "$SCHEMA_FILE")"
else
    _pass "ref_exists 规则已写入（含级别）"
fi

# 再保存一次，规则不能被投影丢掉
run_cli workspace save
assert_exit 0
if ! grep -q '^# @rule: key = unique()$' "$SCHEMA_FILE"; then
    _fail "保存后规则丢了:\n$(cat "$SCHEMA_FILE")"
else
    _pass "保存后规则仍在"
fi

# ── schema show --rules：系统内置规则也列出来 ──
run_cli schema show --rules
assert_exit 0
assert_contains "unique()"
assert_contains "系统"

cleanup
rm -f "$SCHEMA"
end_tests
