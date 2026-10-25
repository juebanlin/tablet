#!/bin/bash
# tests/11_e2e/test_java_roundtrip.sh — Java 端到端：export → javac → run → 验证输出
#
# 跑两遍，覆盖 export.server.java.lombok 的两态：
#   分支 A（lombok=false）：纯 javac，不需要任何额外依赖，产物是手写 getter。
#   分支 B（lombok=true，默认）：需要 lombok.jar，产物靠 @Getter 生成 getter。
# 两分支走同一套断言 —— 这本身就守住了「开关不改变加载行为」。
#
# ⚠️ javac 必须显式给 -processorpath。JDK 23 起 javac 不再隐式运行 classpath 上的
#    注解处理器，只给 -cp 的话 Lombok 会被**静默忽略**：exit 0、零警告，
#    编出来的 class 里一个 getter 都没有（本机 JDK 25 实测）。用 -cp 而不是
#    -proc:full 是因为 -proc:full 到 JDK 21 才存在，CI 上的 JDK 17 认不出来。
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "e2e/java-roundtrip"

# 检查 Java 环境
if ! command -v javac &>/dev/null || ! command -v java &>/dev/null; then
    echo -e "${YELLOW}SKIP${NC} javac/java not found [e2e/java-roundtrip]"
    _PASS=1
    end_tests
    exit 0
fi

JAVA_PACKAGE="com.game.config"

# run_roundtrip <lombok: true|false>
run_roundtrip() {
    local lombok="$1"

    setup_empty_workspace
    "$CLI" -w "$WORK_DIR" project new --template standard --id rt >/dev/null 2>&1
    local proj="$WORK_DIR/projects/rt"

    # 导出 JSON 数据
    run_cli export data --json
    assert_exit 0

    # 导出 Java 代码（使用统一 package）
    run_cli -s "export.server.java.lombok=$lombok" export code --java --package "$JAVA_PACKAGE"
    assert_exit 0

    # 生成 TestMain.java（同一个 package；布尔字段的取值方式要跟导出侧一致）
    run_cli --project rt schema export --out "$WORK_DIR/gen_test.tblschema"
    assert_exit 0

    run_cli_raw util gen-test --lang java --format json \
        --schema "$WORK_DIR/gen_test.tblschema" \
        --package "$JAVA_PACKAGE" \
        --lombok "$lombok" \
        -o "$proj"
    assert_exit 0
    assert_file_exists "$proj/TestMain.java"

    local java_src
    java_src=$(find "$proj" -path "*/gen/server/java" -type d | head -1)
    if [ -z "$java_src" ]; then
        _fail "Java source dir not found"
        return
    fi

    # 编译（TestMain.java + 所有生成的 Java 类）
    local javac_args=()
    if [ "$lombok" = "true" ]; then
        javac_args=(-processorpath "$LOMBOK_JAR_PATH")
    fi
    local java_files
    java_files=$(find "$java_src" -name "*.java")
    set +e
    javac ${javac_args[@]+"${javac_args[@]}"} -cp "$java_src:$LOMBOK_JAR_PATH" \
        $java_files "$proj/TestMain.java" -d "$proj/out" 2>"$proj/javac_err.txt"
    local javac_exit=$?
    set -e
    if [ $javac_exit -ne 0 ]; then
        _fail "[lombok=$lombok] javac failed: $(cat "$proj/javac_err.txt")"
        return
    fi
    _pass "[lombok=$lombok] javac ok"

    # 运行
    local data_dir="$proj/gen/server/data/json"
    set +e
    local java_out
    java_out=$(java -cp "$proj/out" TestMain "$data_dir" 2>&1)
    local java_exit=$?
    set -e
    if [ $java_exit -ne 0 ]; then
        _fail "[lombok=$lombok] java run failed: $java_out"
        return
    fi
    _pass "[lombok=$lombok] java run ok"

    # 验证输出包含预期数据
    if echo "$java_out" | grep -q "HeroBase"; then
        _pass "[lombok=$lombok] output contains HeroBase"
    else
        _fail "[lombok=$lombok] output missing HeroBase data"
    fi

    # 嵌套 ref（@S24）：List<@Item> 加载为 List<Integer>，值往返正确；顶层 table ref leader=@HeroBase → int
    if echo "$java_out" | grep -q "items=\[2001, 2002\]"; then
        _pass "[lombok=$lombok] output contains items=[2001, 2002] (List<@Item>)"
    else
        _fail "[lombok=$lombok] output missing items=[2001, 2002] (List<@Item>)"
    fi
    if echo "$java_out" | grep -q "leader=1002"; then
        _pass "[lombok=$lombok] output contains leader=1002 (@HeroBase)"
    else
        _fail "[lombok=$lombok] output missing leader=1002 (@HeroBase)"
    fi
}

LOMBOK_JAR_PATH="$(find_lombok_jar)"
LOMBOK_AVAILABLE=false
[ -n "$LOMBOK_JAR_PATH" ] && LOMBOK_AVAILABLE=true

# ── 分支 A：lombok=false，纯 javac ──
echo "--- lombok=false ---"
run_roundtrip false

# ── 分支 B：lombok=true（默认），需要 lombok.jar ──
echo "--- lombok=true ---"
if [ "$LOMBOK_AVAILABLE" = false ]; then
    echo -e "${YELLOW}SKIP${NC} lombok.jar 未找到，跳过 lombok=true 分支" \
         "（mvn dependency:get -Dartifact=org.projectlombok:lombok:1.18.48，或设 LOMBOK_JAR=...）[e2e/java-roundtrip]"
    _PASS=$((_PASS + 1))
elif ! lombok_usable "$LOMBOK_JAR_PATH"; then
    echo -e "${YELLOW}SKIP${NC} lombok.jar 在当前 JDK 上不可用：$LOMBOK_JAR_PATH" \
         "（Lombok 与 JDK 严格绑定，JDK 25 需要 >= 1.18.40；" \
         "升级：mvn dependency:get -Dartifact=org.projectlombok:lombok:1.18.48）[e2e/java-roundtrip]"
    _PASS=$((_PASS + 1))
else
    run_roundtrip true

    # ── 布尔字段的 is 前缀 ──
    # standard 里唯一的 bool（debug_mode）是 export=-，不进 Java 产物；只有 sanguo 有
    # server 导出的 bool（building/Building.unique、map/ResourceNode.occupiable）。
    # Lombok 对原始 boolean 生成的是 isXxx() 而不是 getXxx()（@11.3.4），这里把它钉死。
    setup_empty_workspace
    "$CLI" -w "$WORK_DIR" project new --template sanguo --id sg >/dev/null 2>&1
    local_proj="$WORK_DIR/projects/sg"
    run_cli export code --java
    assert_exit 0

    sg_src=$(find "$local_proj" -path "*/gen/server/java" -type d | head -1)
    sg_out="$local_proj/out"
    set +e
    javac -processorpath "$LOMBOK_JAR_PATH" -cp "$sg_src:$LOMBOK_JAR_PATH" \
        $(find "$sg_src" -name "*.java") -d "$sg_out" 2>"$local_proj/javac_err.txt"
    sg_javac=$?
    set -e
    if [ $sg_javac -ne 0 ]; then
        _fail "sanguo javac failed: $(cat "$local_proj/javac_err.txt")"
    else
        building_api="$(javap -p -cp "$sg_out" com.game.config.tpl.BuildingTpl)"
        if echo "$building_api" | grep -q "public boolean isUnique()"; then
            _pass "boolean 字段走 Lombok 的 isUnique()"
        else
            _fail "boolean 字段应有 isUnique()，实际：$building_api"
        fi
        if echo "$building_api" | grep -q "getUnique()"; then
            _fail "boolean 字段不该同时有 getUnique()"
        else
            _pass "boolean 字段没有多余的 getUnique()"
        fi
    fi
fi

cleanup
end_tests
