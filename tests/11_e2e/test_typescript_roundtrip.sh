#!/bin/bash
# tests/11_e2e/test_typescript_roundtrip.sh — TypeScript 端到端：export → tsc → node
#
# TS 占 10 个后端里的 2 个（client / server），此前是唯一一个没有任何 e2e 覆盖的
# 「会生成真代码」的后端。这条守住三件事：
#   1. 生成的 .ts 能过 `tsc --strict`——产物必须是**模块**（有顶层 export），
#      否则对 TS 而言是全局脚本：类型传不出去、还要 @types/node。
#      module_kind 废弃前，CJS 产物每个文件都报 TS2591（@11.3.1 A3）。
#   2. 编译成 CommonJS 后 node 能直接 require——这是 module_kind 删掉之后，
#      CJS 消费方的实际迁移路径（另一条是 Node 22.12+ 直接 require ESM）。
#   3. 数据 / 枚举取值对得上（属性名是 camelCase，查表 key 是源名，@01.8.1）。
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "e2e/typescript-roundtrip"

# tsc 是 npm 全局安装的 shim，通常不在 PATH 上；NPM_GLOBAL 兜底
NPM_GLOBAL="${NPM_GLOBAL:-$HOME/AppData/Roaming/npm}"
TSC="$(find_tool TSC "tsc" "$NPM_GLOBAL")"
NODE="$(find_tool NODE "node")"

if [ -z "$NODE" ]; then
    echo "SKIP node not found [e2e/typescript-roundtrip]"
    _PASS=1
    end_tests
    exit 0
fi
if [ -z "$TSC" ]; then
    echo "SKIP tsc not found（npm i -g typescript，或设 TSC=/path/to/tsc）[e2e/typescript-roundtrip]"
    _PASS=1
    end_tests
    exit 0
fi

setup_workspace standard

run_cli export all
assert_exit 0

SRC_DIR="$WORK_DIR/projects/test/gen/client/typescript"
assert_file_exists "$SRC_DIR/Item.ts"
assert_file_exists "$SRC_DIR/GameConst.ts"
assert_file_exists "$SRC_DIR/HeroType.ts"

# 产物必须是模块：顶层 export 是 TS 判定「模块」的唯一依据
if grep -q "^export const Item" "$SRC_DIR/Item.ts"; then
    _pass "Item.ts 是模块（顶层 export）"
else
    _fail "Item.ts 缺顶层 export，TS 会当成全局脚本: $(head -3 "$SRC_DIR/Item.ts")"
fi
if grep -q "module.exports\|require(" "$SRC_DIR/Item.ts"; then
    _fail "产物里残留 CommonJS 语法（module_kind 已废弃）: $SRC_DIR/Item.ts"
else
    _pass "产物不含 CommonJS 语法"
fi

APP="$(mktemp -d)"
trap 'rm -rf "$APP"; cleanup' EXIT
mkdir -p "$APP/gen"
cp "$SRC_DIR"/*.ts "$APP/gen/"

# 只碰客户端可见的字段：Item.price 是 `export:服务器`，客户端产物里没有
cat > "$APP/smoke.ts" <<'EOF'
import { Item } from './gen/Item';
import { GameConst } from './gen/GameConst';
import { HeroTypeEnum, HeroTypeEnumDesc, HeroTypeEnumNames } from './gen/HeroType';

const it = Item[2001];
// 服务端传来的是 int，按数字查 name——反查表就是为这个场景生成的
const nameOfArcher: string = HeroTypeEnumNames[3];
console.log(
    `item=${it.name} stack_max=${it.stackMax} weight=${it.weight}` +
    ` count=${Object.keys(Item).length} version=${GameConst.version}` +
    ` name3=${nameOfArcher} desc1=${HeroTypeEnumDesc[HeroTypeEnum.WARRIOR]}`,
);
EOF

# target/module 显式钉死，不吃 tsc 默认值（TS 7 已移除 moduleResolution=node10）
cat > "$APP/tsconfig.json" <<'EOF'
{
  "compilerOptions": {
    "strict": true,
    "target": "ES2020",
    "module": "commonjs"
  },
  "include": ["smoke.ts", "gen/**/*.ts"]
}
EOF

set +e
TSC_OUT=$("$TSC" -p "$APP/tsconfig.json" --outDir "$APP/js" 2>&1)
TSC_EXIT=$?
set -e
if [ $TSC_EXIT -ne 0 ]; then
    _fail "tsc 编译失败: $TSC_OUT"
    end_tests
    exit 1
fi
_pass "tsc --strict 编译通过（ESM 源码 → CommonJS 输出）"

assert_file_exists "$APP/js/smoke.js"

set +e
NODE_OUT=$("$NODE" "$APP/js/smoke.js" 2>&1)
NODE_EXIT=$?
set -e
if [ $NODE_EXIT -ne 0 ]; then
    _fail "node 运行失败: $NODE_OUT"
    end_tests
    exit 1
fi
_pass "node 运行编译产物 ok"

# stackMax 同时验证「属性名走 camelCase」（@01.8.1）
if echo "$NODE_OUT" | grep -q "stack_max=99"; then
    _pass "Item.stackMax = 99"
else
    _fail "Item.stackMax 不是 99（camelCase 转换对不上？）: $NODE_OUT"
fi
if echo "$NODE_OUT" | grep -q "item=生命药水"; then
    _pass "Item.name 取到中文"
else
    _fail "Item.name 不对: $NODE_OUT"
fi
if echo "$NODE_OUT" | grep -q "count=4"; then
    _pass "Item 有 4 行"
else
    _fail "Item 行数不对: $NODE_OUT"
fi
if echo "$NODE_OUT" | grep -q "version=1.0.0"; then
    _pass "GameConst.version = 1.0.0（as const 保留）"
else
    _fail "GameConst 不对: $NODE_OUT"
fi
if echo "$NODE_OUT" | grep -q "name3=ARCHER"; then
    _pass "值→名 XxxEnumNames[3] = ARCHER"
else
    _fail "枚举反查表不对: $NODE_OUT"
fi
if echo "$NODE_OUT" | grep -q "desc1=战士"; then
    _pass "值→desc XxxEnumDesc[WARRIOR] = 战士"
else
    _fail "枚举 desc 表不对: $NODE_OUT"
fi

end_tests
