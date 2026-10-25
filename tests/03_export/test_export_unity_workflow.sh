#!/bin/bash
# tests/03_export/test_export_unity_workflow.sh
#
# Unity 客户端场景：路径只配在 project.toml，CLI 不感知 Unity。
#   export data --json        → <data_output>/json/
#   export code --csharp-unity → <csharp_unity.code_output>/
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "export/unity-workflow"

setup_workspace standard
PROJECT_ROOT="$WORK_DIR/projects/test"
CLIENT="$WORK_DIR/client"
GEN="$PROJECT_ROOT/gen"

mkdir -p "$CLIENT/Assets/Resources/Config" "$CLIENT/Assets/Scripts/Config"
cat > "$PROJECT_ROOT/project.toml" <<'TOML'
[export.server]
data_output = "../../client/Assets/Resources/Config"

[export.client.csharp_unity]
code_output = "../../client/Assets/Scripts/Config"
TOML

run_cli --project test validate
assert_exit 0

run_cli --project test export data --json
assert_exit 0
assert_file_exists "$CLIENT/Assets/Resources/Config/json/HeroBase.json"

run_cli --project test export code --csharp-unity
assert_exit 0
assert_file_exists "$CLIENT/Assets/Scripts/Config/Loader.cs"
assert_file_contains "$CLIENT/Assets/Scripts/Config/Loader.cs" "Newtonsoft"
assert_dir_not_exists "$GEN/client/csharp_unity"
assert_dir_not_exists "$GEN/server/data"

cleanup
end_tests
