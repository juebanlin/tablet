#!/bin/bash
# tests/10_performance/test_export_json_perf.sh — JSON 导出性能阶梯测试
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "perf/export-json"

SCHEMA="$SCRIPT_DIR/../schemas/standard.tblschema"
DATAGEN="$SCRIPT_DIR/../helpers/datagen.py"
TIERS=(100 1000 10000 100000)

echo ""
echo "=== JSON Export Performance ==="
printf "%-10s %-10s\n" "Rows" "Time(s)"

for ROWS in "${TIERS[@]}"; do
    setup_empty_workspace
    "$CLI" -w "$WORK_DIR" project new --template standard --id perf >/dev/null 2>&1
    CONFIG_DIR="$WORK_DIR/projects/perf/config"
    $PYTHON "$DATAGEN" --schema "$SCHEMA" --output "$CONFIG_DIR" --rows "$ROWS" --seed 42 >/dev/null

    START=$(date +%s%N)
    run_cli export data --json
    END=$(date +%s%N)
    ELAPSED=$(( (END - START) / 1000000 ))
    printf "%-10s %-10s\n" "$ROWS" "${ELAPSED}"
    cleanup
done

_PASS=1
end_tests
