#!/bin/bash
# tests/11_e2e/test_cpp_roundtrip.sh — C++ 端到端：export → g++ → 运行 → 验证输出
#
# C++ 产物是 header-only，靠文件级静态对象注册表；本测试同时守住
# 「生成的 C++ 能过 -Wall -Wextra 编译」这条底线（之前有个 unused parameter 告警）。
#
# 依赖 nlohmann/json（缺了跳过）：ubuntu 上 `apt install nlohmann-json3-dev`。
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "e2e/cpp-roundtrip"

if ! command -v g++ &>/dev/null; then
    echo "SKIP g++ not found [e2e/cpp-roundtrip]"
    _PASS=1
    end_tests
    exit 0
fi

setup_workspace standard

run_cli export all
assert_exit 0

CPP_SRC="$WORK_DIR/projects/test/gen/server/cpp"
DATA_DIR="$WORK_DIR/projects/test/gen/server/data/json"
assert_file_exists "$CPP_SRC/all_tpls.h"

APP="$(mktemp -d)"
trap 'rm -rf "$APP"; cleanup' EXIT

cat > "$APP/smoke.cpp" <<'EOF'
// 必须在 include 前定义：loader.h 里 init_json 整体在 #ifdef TBL_JSON_LIB_NLOHMANN 内
#define TBL_JSON_LIB_NLOHMANN
#include "all_tpls.h"
#include <cstdio>

int main(int argc, char** argv) {
    if (argc < 2) return 2;
    game::config::init_json(argv[1]);
    const auto* it = game::config::ItemTpl::Get(2001);
    if (it == nullptr) { std::printf("Item 2001 not found\n"); return 1; }
    const auto& c = game::config::GameConstTpl::Get();
    const auto& all = game::config::ItemTpl::All();
    std::printf("item=%s stack_max=%d price=%lld weight=%f\n",
        it->name.c_str(), it->stackMax, static_cast<long long>(it->price), it->weight);
    const auto* hero = game::config::HeroBaseTpl::Get(1001);
    std::printf("hero=%s skills=%zu leader=%d\n",
        hero->name.c_str(), hero->skills.size(), hero->leader);
    std::printf("const max_level=%d item_count=%zu\n", c.maxLevel, all.size());
    return 0;
}
EOF

set +e
g++ -std=c++17 -Wall -Wextra -I"$CPP_SRC" "$APP/smoke.cpp" -o "$APP/smoke" >"$APP/build.log" 2>&1
GXX_EXIT=$?
if [ $GXX_EXIT -eq 0 ]; then
    "$APP/smoke" "$DATA_DIR" >"$APP/run.log" 2>&1
    RUN_EXIT=$?
else
    RUN_EXIT=1
fi
set -e

if [ $GXX_EXIT -ne 0 ]; then
    if grep -qi "nlohmann\|json.hpp" "$APP/build.log"; then
        echo "SKIP nlohmann/json 缺失（apt install nlohmann-json3-dev）[e2e/cpp-roundtrip]"
        _PASS=1
        end_tests
        exit 0
    fi
    _fail "g++ 编译失败: $(tail -20 "$APP/build.log")"
    end_tests
    exit 1
fi
_pass "g++ -Wall -Wextra 编译通过"

if [ $RUN_EXIT -ne 0 ]; then
    _fail "运行失败: $(cat "$APP/run.log")"
    end_tests
    exit 1
fi
_pass "运行 ok"

CPP_OUT="$(cat "$APP/run.log")"
# stack_max 同时验证「数据文件 key 用源名」（@01.8.1）
if echo "$CPP_OUT" | grep -q "stack_max=99"; then
    _pass "Item.stackMax = 99"
else
    _fail "Item.stackMax 不是 99（数据没加载或 key 对不上）: $CPP_OUT"
fi
if echo "$CPP_OUT" | grep -q "item_count=4"; then
    _pass "ItemTpl::All 有 4 行"
else
    _fail "ItemTpl::All 行数不对: $CPP_OUT"
fi
if echo "$CPP_OUT" | grep -q "skills=3 leader=1002"; then
    _pass "HeroBase.skills = 3 个 / leader = 1002"
else
    _fail "HeroBase 字段不对: $CPP_OUT"
fi
if echo "$CPP_OUT" | grep -q "max_level=100"; then
    _pass "GameConst.max_level = 100"
else
    _fail "常量没加载: $CPP_OUT"
fi

end_tests
