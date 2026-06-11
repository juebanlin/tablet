#!/bin/bash
# tests/10_performance/test_parse_perf.sh — .tbl 解析性能阶梯测试
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "perf/parse"

SCHEMA="$SCRIPT_DIR/../schemas/standard.tblschema"
DATAGEN="$SCRIPT_DIR/../helpers/datagen.py"
TIERS=(100 1000 10000 100000)

echo ""
echo "=== TBL Parse Performance (project load) ==="
printf "%-10s %-10s\n" "Rows" "Time(s)"

for ROWS in "${TIERS[@]}"; do
    setup_empty_workspace
    "$CLI" -w "$WORK_DIR" project new --template standard --id perf >/dev/null 2>&1
    CONFIG_DIR="$WORK_DIR/projects/perf/config"
    $PYTHON "$DATAGEN" --schema "$SCHEMA" --output "$CONFIG_DIR" --rows "$ROWS" --seed 42 >/dev/null

    # project info 触发完整的项目加载（解析所有 .tbl）
    START=$(date +%s%N)
    run_cli project info
    END=$(date +%s%N)
    ELAPSED=$(( (END - START) / 1000000 ))
    printf "%-10s %-10s\n" "$ROWS" "${ELAPSED}"
    cleanup
done

_PASS=1
end_tests
