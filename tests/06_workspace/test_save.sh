#!/bin/bash
# tests/06_workspace/test_save.sh
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "workspace/save"

# ── 测试配置 ──
TEMPLATE="standard"

setup_workspace "$TEMPLATE"
run_cli workspace save
assert_exit 0
assert_contains "已保存"
cleanup

# ── 有验证错误时 save_all 什么都不写，命令不能说"已保存" ──
# 把 int 字段的格子改坏（v3 块式：`  hp:1200`）
setup_workspace "$TEMPLATE"
"$PYTHON" - "$WORK_DIR/projects/test/config/hero/HeroBase.tbl" <<'PY'
import io, re, sys
p = sys.argv[1]
s = io.open(p, encoding='utf-8').read()
io.open(p, 'w', encoding='utf-8').write(re.sub(r'(\n\s*hp:)\d+', r'\1abc', s, count=1))
PY
run_cli workspace save
assert_exit 1
assert_stderr_contains "验证错误"
assert_not_contains "已保存"
cleanup

end_tests
