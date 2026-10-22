#!/bin/bash
# tests/03_export/test_export_failure_exit.sh
#
# 逐格式失败不打断其它格式（和以前一致），但**退出码必须是失败**：
# `export all || exit 1` 这种 CI 写法全靠它。以前这里恒为 Ok(0)。
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "export/failure-exit"

TEMPLATE="standard"
setup_workspace "$TEMPLATE"

# 把 data 输出目录下的 `json` 先占成一个**文件**：JSON 写不进去，XML 照样能写
mkdir -p "$WORK_DIR/blocked"
: > "$WORK_DIR/blocked/json"

run_cli export data -o "$WORK_DIR/blocked"
assert_exit 1
assert_stderr_contains "错误"
# 另一种格式仍然跑完了
assert_file_exists "$WORK_DIR/blocked/xml/HeroBase.xml"

cleanup
end_tests
