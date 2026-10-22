---
name: tablet
description: 生成 / 修改 / 校验 tablet 配置表，并用 tablet-cli 跑通「建项目 → 验证 → 导出」流水线。当用户要新建或扩展 .tblschema 模板、设计字段与类型、追加 # @preset 预设数据、手改 .tbl 数据文件，或执行 tablet-cli 命令（project new / schema / validate / export / excel / workspace）时使用。涵盖 .tblschema v1 与 .tbl v3 格式、7 个基础类型 + 复合范式 + @Ref、解析器强制的命名规则、退出码约定。
user-invocable: true
allowed-tools:
  - Read
  - Write
  - Edit
  - Glob
  - Grep
  - Bash
---

# tablet —— 配置表格式 + CLI 流水线

`tablet-cli` 把若干 `.tbl` / `.tblschema` 编译成 JSON/XML 数据和 7 种语言的代码。
**AI 只做两件事：写文件、跑命令。** 写错了解析器会带行号报错，跑错了退出码非零——不要绕过它们。

- 格式细节 → `references/format.md`
- 类型系统 → `references/types.md`
- 命令全表 → `references/cli.md`
- 可验证的完整例子 → `references/example.tblschema`

## 一、先选路线

| 场景 | 怎么做 |
|---|---|
| 从零建一套配置 | 写 `<id>.tblschema`（含 `# @preset` 数据）→ `project new --from-file` |
| 已有的项目加/改数据 | 直接改 `projects/<id>/config/<group>/<Name>.tbl` → `workspace save` |
| 加一个组 / 表 / 常量 / 枚举 | `tablet-cli schema add-*`（会自动校验并落盘） |
| 只想知道结构 | `tablet-cli schema show --fmt json` |
| 打包给程序读 | `tablet-cli export all -o <dir>` |

> **真相源**：项目建好之后，每张表的结构与数据以 `.tbl` 为准；`project.tblschema` 是它的**投影**，
> 在 `workspace save` 或任何写命令落盘时重写。改 `.tblschema` 不会改动已有的 `.tbl`。

## 二、三步流水线（最常用）

```bash
# 1. 新建项目：schema 里的 # @preset 会落成 .tbl 数据；校验不过则整个项目撤回
tablet-cli -w <workdir> project new --from-file demo.tblschema --id demo

# 2. 验证：0 = 通过；1 = 有错（逐条打印定位）
tablet-cli -w <workdir> --project demo validate

# 3. 导出（默认落在 projects/<id>/gen/ 下；-o 相对项目根解析，拿不准用绝对路径）
tablet-cli -w <workdir> --project demo export all -o artifacts/demo
```

`export all` = 数据（JSON+XML）+ Java/Go/Lua/GDScript/TypeScript/C++/C#。
只要数据：`export data --json`；只要某种代码：`export code --java`。

## 三、硬规则（写错就一定失败）

这些是解析器 / 校验器强制的，不是风格建议。

### 名字

| 对象 | 规则 | 例子 |
|---|---|---|
| 段名（= 表/常量/枚举名 = `.tbl` 文件名 = 生成类名） | 大写字母开头，只含英文数字下划线，全项目唯一（忽略大小写） | `HeroBase` ✅ `hero_base` ❌ |
| 组名（= 目录名） | 只能是中英文、数字、下划线 | `hero` ✅ `战斗系统` ✅ `bad group!` ❌ |
| 字段名 / 常量名 | `[a-z][a-z0-9_]*`，**不能是 Java/Go/Lua 关键字** | `max_level` ✅ `class` ❌ `type` ❌ |
| 枚举条目名 | `[A-Z][A-Z0-9_]*`（UPPER_SNAKE），同一枚举内唯一 | `WARRIOR` ✅ `Warrior` ❌ |
| 枚举 id | 正整数，**不能是 0**（0 = 未设置） | `1` ✅ `0` ❌ |
| `# @meta id` | `[a-z0-9_-]{1,32}`；不写也行，写了就必须合法 | `demo` ✅ `Bad-Id` ❌ |

### 导出列

**只认短码**：`cs`（默认，双端）/ `c`（客户端）/ `s`（服务器）/ `-`（不导出）。
留空 = `cs`。**中文「前后端/客户端/服务器/不导出」不是磁盘格式**，写进文件会解析失败。

### 模式

- `table`：段体就是字段行 `name | type | export | desc`；第一列必须是 `id | int | ...`。
- `constant`：段体必须是空的，条目只写在 `# @preset` 里，5 列 `name | type | value | export | desc`。
- `enum`：段体必须是空的，条目只写在 `# @preset` 里，3 列 `id | name | desc`。
- `# @preset` 必须出现在某个 `[group/Name]` 之后，作用到下一个段头为止。

### 类型

合法类型字符串只有这些（详见 `references/types.md`）：

```
int long float double str bool txt
Tuple2<..> Tuple3<..> Tuple4<..>      List<..>  Set<..>  Map<..,..>
List<Tuple2<..>>  List<Tuple3<..>>  List<Tuple4<..>>
Map<..,Tuple2<..>>  Map<..,Tuple3<..>>  Map<..,Tuple4<..>>  Map<..,List<..>>
@SomeTable   @SomeEnum
```

- `txt` 只能独立成列，不能嵌套；其余基础类型可嵌套，`@Ref` 也能出现在参数位。
- `@Ref` 的目标必须是 table 或 enum，**不能是 constant**；数据里存的是 int id。
- `List<List<..>>`、`Set<Tuple<..>>`、`Tuple2` 之外的两层嵌套都不合法。
- `str` 不能含 `|` 换行 制表符——需要这些字符就用 `txt`。

### 值（`.tblschema` 的 preset / `.tbl` 的格子）

| 类型 | 写法 | 示例 |
|---|---|---|
| 基础 | 直接写 | `100` / `1.5` / `true` / `hello` |
| Tuple | 逗号 | `5,10` |
| List / Set | 分号 | `1;2;3` |
| Map | `k:v` 之间分号 | `hp:100;mp:50` |
| List\<Tuple\> | 元组内逗号，元组间分号 | `1,2;3,4` |
| Map\<K,Tuple\> | `atk:5,10;def:3,8` | |
| Map\<K,List\> | `hp:1,2,3;mp:4,5` | |

默认分隔符就是上面这些；**没有特殊要求不要写 `# @sep`**。空元素（`1;;3`）永远非法；中文标点 `，；：、` 一律非法。

## 四、`.tblschema` 骨架

```text
#!tblschema v1                 ← 第一行，必须逐字一致
# @meta id: demo               ← 可选；最终项目 id 由 --id 决定
# @meta name: 演示项目          ← 可选；--from-file 且没给 --name 时用它当项目名

[hero/HeroType] enum           ← 段顺序无关；先写枚举便于阅读
# @preset
1 | WARRIOR | 战士
2 | MAGE    | 法师

[hero/HeroBase] table
# @display: name               ← 可选：别处 @HeroBase 引用时显示 name 列
id     | int      | cs | 英雄ID
name   | str      | cs | 名字
kind   | @HeroType| cs | 职业
skills | List<int>| cs | 技能
born   | Tuple2<int,int> | cs | 坐标
# @preset
1001 | 亚瑟 | 1 | 1;2;3 | 5,10

[global/GameConst] constant
# @preset
max_level | int | 100 | cs | 等级上限
```

- `# @meta` / `# @sep` 只在第一个 `[段]` 之前有效，之后当注释。
- `#` 开头是注释；三种例外是指令 `# @meta` / `# @sep` / `# @preset`。
- 井号在行中不是注释——不要写行尾注释。

## 五、改已有项目的数据（`.tbl`）

**表（v3，块式）** `projects/<id>/config/<group>/<Name>.tbl`：

```text
#!tbl v3
#mode table
---

@field id
  desc:英雄ID
  export:cs
  type:int

@field hp
  desc:血量
  export:s
  type:int

[1001]
  name:亚瑟
  hp:1200
```

- 所有 `@field` 块必须在第一条 `[id]` 之前；每条记录以 `[<id>]` 开头，id 就是第一列。
- 只写非空字段；省略 = 空（0 / `""` / `false` / 空集合 / 无引用）。
- 值用该类型的分隔符；`txt` 里的 `\` / 换行 要转义（`str` 根本不允许这些字符）。
- 字段名拼错 / 属性名拼错都会**报错**（以前会静默丢数据），别指望它兜底。

**常量 / 枚举（v2，CSV）**：

```text
#!tbl v2
#mode constant
---
max_level|int|100|cs|等级上限
```
```text
#!tbl v2
#mode enum
---
1|WARRIOR|战士
```

**加字段** = 在 `@field` 区加一个块，再在需要的那几条记录里补 `字段名:值`（老记录不补 = 空值）。
**加记录** = 追加一个 `[<新id>]` 块，id 不能与已有记录重复。

改完 `.tbl` 跑一次 `workspace save`，把结构同步进 `project.tblschema`。
**一个 `.tbl` 写坏了，整个项目会拒绝加载**（不会再静默少一张表）。

## 六、CLI 速查

```bash
tablet-cli [-w <workdir>] [--project <id>] [--fmt json] <命令>
```

| 目的 | 命令 |
|---|---|
| 看模板 | `list-templates` |
| 项目 | `project list` / `info` / `new` / `rename` / `delete --confirm` / `clone` |
| 结构 | `schema show`；`add-group --name G`；`add-table\|add-constant --group G --name N`；`add-enum --group G --name N --entry 1:X:说明` |
| 改名 / 删除 | `schema rename-group --old A --new B`；`schema rename-node --group G --old A --new B`；`schema delete-group --name G`；`schema delete-node --group G --name N` |
| 验证 | `validate [--group G] [--node N]` |
| 导出 | `export data [--json\|--xml] [--group G] [--node N] [-o DIR]`；`export code --java\|--go\|--lua\|--gdscript\|--typescript\|--cpp\|--csharp\|--all [-o DIR]`；`export all [-o DIR]` |
| Excel | `excel export --group G [-o f.xlsx]`；`excel import --group G --file f.xlsx` |
| 保存 | `workspace save`（同时刷新 `project.tblschema`） |

- `--fmt json` 只对查询类命令有效：`project list/info`、`schema show`、`validate`、`export`、`sep show`。
- `--project` 覆盖 `last_project`；不写则跟随 `[project] last_project`。
- `-o` 的**相对路径按项目根 `projects/<id>/` 算**（不是当前目录）；默认导出到 `projects/<id>/gen/`。
- `--switch-after false` 可让 `project new` 不修改 `last_project`。

## 七、失败时怎么读

| 退出码 | 含义 |
|---|---|
| `0` | 成功，结果可信 |
| `1` | 验证不过 / 目标不存在 / 解析失败 / 落盘被拦下 |
| `2` | 用法错误（缺参数、未知选项） |

- 错误在 **stderr**；`--fmt json` 时 stdout 只有 JSON。
- `schema add-*` / `rename-*` / `delete-*` 成功才有输出；目标不存在、名字非法、保存失败都会非零退出。
- 见到 `验证通过，无错误` 才算真的通过——`--group` / `--node` 指到不存在的目标同样退出 1。
- 修 schema 报错时优先看行号：`line N: ...` 指向 `.tblschema` / `.tbl` 的具体行。

## 八、不要做的事

- ❌ 除了 `.tblschema` / `.tbl`，不要手写其它配置文件（`project.toml` / `tablet.toml`）；临时改配置用 `-s key=value`。
- ❌ 不要在 `.tblschema` 里给 constant / enum 段写数据行——只能进 `# @preset`。
- ❌ 不要用中文导出码、不要给枚举用 0 号、不要用关键字当字段名。
- ❌ 不要为了"兼容"去猜格式：读不懂就报错是有意为之。
- ❌ 不要跳过 `validate` 直接 `export` 交付。

## 九、交付前的自检

1. `tablet-cli --project <id> validate` 退出 0。
2. `export all` 退出 0，且日志里没有 `[警告]`。
3. 抽查生成的数据文件（`gen/server/data/json/*.json`）键值与设计一致。
4. 把「加了哪些段、多少行数据、用了哪些类型」用 3-5 条说清楚，不要复述整份文件。
