#!/bin/bash
# tests/11_e2e/test_csharp_roundtrip.sh — C# 端到端：export → dotnet run → 验证输出
#
# 这条测试是有来历的：C# 的 Loader 曾经因为「C# 静态构造器是惰性的」而**静默不加载
# 任何表**——按 Example.cs 文档的顺序（先 LoadAll 再 Get）调用，Get 全返回 null。
# 所有字符串级断言都是绿的，只有真跑起来才发现。详见 docs/11 §3.3。
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../helpers/common.sh"
begin_test "e2e/csharp-roundtrip"

if ! command -v dotnet &>/dev/null; then
    echo "SKIP dotnet not found [e2e/csharp-roundtrip]"
    _PASS=1
    end_tests
    exit 0
fi

# TFM 跟本机 SDK 走，避免写死 net8.0 撞上装了别的版本的环境
SDK_MAJOR="$(dotnet --version 2>/dev/null | cut -d. -f1)"
if [ -z "$SDK_MAJOR" ]; then
    echo "SKIP dotnet --version 无输出 [e2e/csharp-roundtrip]"
    _PASS=1
    end_tests
    exit 0
fi

setup_workspace standard

run_cli export all
assert_exit 0

CSHARP_SRC="$WORK_DIR/projects/test/gen/server/csharp"
DATA_DIR="$WORK_DIR/projects/test/gen/server/data/json"
assert_file_exists "$CSHARP_SRC/Loader.cs"

# ── 组装一个最小 .NET 工程：生成代码 + 一个 Smoke 入口 ──
APP="$(mktemp -d)"
trap 'rm -rf "$APP"; cleanup' EXIT

cp "$CSHARP_SRC"/*.cs "$APP/"
cat > "$APP/smoke.csproj" <<EOF
<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net${SDK_MAJOR}.0</TargetFramework>
    <Nullable>disable</Nullable>
    <AssemblyName>smoke</AssemblyName>
  </PropertyGroup>
</Project>
EOF

cat > "$APP/Program.cs" <<'EOF'
using System;
using Game.Config.Server;

public static class Program {
    public static void Main(string[] args) {
        // 顺序必须跟 Example.cs 里写的一致：先 LoadAll 再 Get。
        // Loader 内部会先强制触发各 *Tpl 的静态构造器，否则注册表是空的。
        Loader.LoadAll(args[0]);
        var it = ItemTpl.Get(2001);
        Console.WriteLine($"item={it.Name} stack_max={it.StackMax} price={it.Price} weight={it.Weight}");
        var hero = HeroBaseTpl.Get(1001);
        Console.WriteLine($"hero={hero.Name} skills={string.Join(",", hero.Skills)} leader={hero.Leader}");
        Console.WriteLine($"const max_level={GameConstTpl.Get().MaxLevel} item_count={ItemTpl.All.Count}");
    }
}
EOF

# ── 编译 + 运行 ──
set +e
CS_OUT=$(cd "$APP" && dotnet run --project smoke.csproj -- "$DATA_DIR" 2>&1)
CS_EXIT=$?
set -e

if [ $CS_EXIT -ne 0 ]; then
    _fail "dotnet run failed (exit $CS_EXIT): $(echo "$CS_OUT" | tail -20)"
    end_tests
    exit 1
fi
_pass "dotnet build + run ok"

# ── 验证数据真的加载进来了（这几条曾经全是 null） ──
# stack_max 同时验证「数据文件 key 用源名」这条规则（@01.8.1）：key 若是 camelCase
# stackMax，这里会读到 0
if echo "$CS_OUT" | grep -q "stack_max=99"; then
    _pass "Item.stack_max = 99"
else
    _fail "Item.stack_max 不是 99（数据没加载或 key 对不上）: $CS_OUT"
fi
if echo "$CS_OUT" | grep -q "item_count=4"; then
    _pass "ItemTpl.All 有 4 行"
else
    _fail "ItemTpl.All 行数不对（注册表没填好？）: $CS_OUT"
fi
if echo "$CS_OUT" | grep -q "skills=1,2,3"; then
    _pass "HeroBase.skills = [1,2,3]"
else
    _fail "HeroBase.skills 不对: $CS_OUT"
fi
if echo "$CS_OUT" | grep -q "max_level=100"; then
    _pass "GameConst.max_level = 100"
else
    _fail "GameConst 常量没加载: $CS_OUT"
fi

end_tests
