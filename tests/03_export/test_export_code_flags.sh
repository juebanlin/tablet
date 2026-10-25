#!/bin/bash
# tests/03_export/test_export_code_flags.sh
#
# export code 的格式粒度与 -o 语义：
# - --csharp-unity / --typescript-server 只出指定那一套；
# - 多格式 -o DIR = 输出根（标准子目录），单格式 -o = 精确目录；
# - GDScript 的 -o 覆盖（以前静默失效）必须生效。
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "export/code-flags"

setup_workspace standard
GEN="$WORK_DIR/projects/test/gen"

# ── 单个 C# runtime ──
run_cli export code --csharp-unity
assert_exit 0
assert_file_exists "$GEN/client/csharp_unity/Loader.cs"
assert_file_contains "$GEN/client/csharp_unity/Loader.cs" "Newtonsoft"
assert_dir_not_exists "$GEN/server/csharp"
assert_dir_not_exists "$GEN/client/csharp_godot"

# ── 单个 TS 端 ──
run_cli export code --typescript-server
assert_exit 0
assert_dir_exists "$GEN/server/typescript"
assert_dir_not_exists "$GEN/client/typescript"

run_cli export code --typescript-client
assert_exit 0
assert_dir_exists "$GEN/client/typescript"

# ── 多格式 -o：按标准布局展开，不互相覆盖 ──
run_cli export code --java --go -o "$WORK_DIR/out"
assert_exit 0
assert_dir_exists "$WORK_DIR/out/server/java"
assert_dir_exists "$WORK_DIR/out/server/go"

run_cli export code --csharp -o "$WORK_DIR/cs"
assert_exit 0
assert_file_exists "$WORK_DIR/cs/server/csharp/Loader.cs"
assert_file_exists "$WORK_DIR/cs/client/csharp_unity/Loader.cs"
assert_file_exists "$WORK_DIR/cs/client/csharp_godot/Loader.cs"
assert_file_contains "$WORK_DIR/cs/client/csharp_unity/Loader.cs" "Newtonsoft"

# ── 单格式 -o：精确目录（含 GDScript 回归）──
run_cli export code --gdscript -o "$WORK_DIR/gd"
assert_exit 0
assert_file_exists "$WORK_DIR/gd/HeroBase.gd"

cleanup
end_tests
