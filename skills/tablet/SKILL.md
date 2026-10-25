---
name: tablet
description: 生成 / 修改 / 校验 tablet 配置表，并用 tablet-cli 跑通「建项目 → 验证 → 导出」流水线。当用户要新建或扩展 .tblschema 模板、设计字段与类型、追加 # @preset 预设数据、手改 .tbl 数据文件、给列加校验规则（# @rule）、从 Unity 插件 / bat 调外部工具刷新配置、查看工作区版本分支，或执行 tablet-cli 命令（project new / schema / validate / export / excel / workspace）时使用。涵盖 .tblschema v1 与 .tbl v3/v2 格式、7 个基础类型 + 复合范式 + @Ref、内置验证器与错误/警告分级、12 种导出格式、解析器强制的命名规则、退出码约定。
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
- Unity / 外部宿主调用 → `references/unity.md`
- 可验证的完整例子 → `references/example.tblschema`

## 一、先选路线

| 场景 | 怎么做 |
|---|---|
| 从零建一套配置 | 写 `<id>.tblschema`（含 `# @preset` 数据）→ `project new --from-file` |
| 已有的项目加/改数据 | 直接改 `projects/<id>/config/<group>/<Name>.tbl` → `workspace save` |
| 数据在 `.xlsx` 里的组（工作簿组） | 见 §五之二：**不要手写 xlsx**，用 GUI / Excel / schema 命令 |
| 加一个组 / 表 / 常量 / 枚举 | `tablet-cli schema add-*`（会自动校验并落盘） |
| 只想知道结构 | `tablet-cli schema show --fmt json` |
| 看每个节点的有效校验规则 | `tablet-cli schema show --rules` |
| 给某列加约束 | 在 `.tblschema` 段内写 `# @rule`（见 §五之三），或 GUI 右键「校验规则」 |
| 导出项目结构 / 存为模板 | `tablet-cli schema export --out <file>` / `--as-template --meta-id ID` |
| 看工作区在哪个分支 | `tablet-cli workspace info`（不占锁，CLI / Unity 插件都能读） |
| Unity 插件按钮刷新配置 | 见 §六之二；宿主直接调 `export data --json` + `export code --csharp-unity` |
| 打包给程序读 | `tablet-cli export all -o <dir>` |

> **真相源**：两种组后端（@02.26.8）——
> - `.tbl` 组：结构与数据都在各 `.tbl` 里；`project.tblschema` 是投影，保存时重写。
> - `.xlsx` 组：结构在 `project.tblschema` 的 section，数据在工作簿的 sheet 里。
>
> 项目建好之后改 `.tblschema` 不会改动已有的 `.tbl`；`.xlsx` 组反过来，结构以 schema 为准。

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
| 枚举 id | 正整数，**不能是 0**（0 = 未设置）；**不要求连续**，允许跨段预留 | `1` ✅ `0` ❌ |
| Table 的 `id` | 正整数、同表内唯一（整行全空的占位行除外） | `0` / `-1` ❌ |
| `# @meta id` | `[a-z0-9_-]{1,32}`；不写也行，写了就必须合法 | `demo` ✅ `Bad-Id` ❌ |

### 导出列

**只认短码**：`cs`（默认，双端）/ `c`（客户端）/ `s`（服务器）/ `-`（不导出）。
留空 = `cs`。**中文「前后端/客户端/服务器/不导出」不是磁盘格式**，写进文件会解析失败。

### 模式

- `table`：段体就是字段行 `name | type | export | desc`；第一列必须是 `id | int | ...`。
- `constant`：段体必须是空的，条目只写在 `# @preset` 里，5 列 `name | type | value | export | desc`。
- `enum`：段体必须是空的，条目只写在 `# @preset` 里，3 列 `id | name | desc`。
- `# @preset` 必须出现在某个 `[group/Name]` 之后，作用到下一个段头为止。
- preset 行的列数**不能超过**声明（Table = 字段数，Constant = 5，Enum = 3）；少写可以，多写会报错。

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
# @rule: name = unique()        ← 只写 schema，不落 .tbl
# @preset
1001 | 亚瑟 | 1 | 1;2;3 | 5,10

[global/GameConst] constant
# @preset
max_level | int | 100 | cs | 等级上限
```

- `# @meta` / `# @sep` 只在第一个 `[段]` 之前有效，之后当注释。
- `#` 开头是注释；例外是指令 `# @meta` / `# @sep` / `# @display` / `# @rule` / `# @preset`。
- 规范顺序：段头 → 字段行 → `# @display` → `# @rule` → `# @preset`（`# @preset` 必须最后，它会打开数据模式）。
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
- 字段名 / 属性名拼错会**报错**，不会静默丢数据。

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

## 五之二、Xlsx 组（工作簿组）

有的项目把数据放在 `config/<group>.xlsx` 里：**一个组一个工作簿，节点 = sheet**。工具把它当普通组用——
`validate` / `export` / `schema show` 完全一样，导出结果和 `.tbl` 组没有区别。

- **结构仍然在 `project.tblschema`**：字段名 / 类型 / export / 顺序以 section 为准；sheet 只是数据。
- Table sheet：第 1–4 行是 desc / export / type / 字段名（**派生显示**，改了不生效），第 5 行起是数据。
- Constant sheet：1 行表头 `name | type | value | export | desc`；Enum：`id | name | desc`。
- **不要用文本工具手写 `.xlsx`**（二进制）。要改数据：GUI 里编辑、或「用 Excel 打开（原文件）」、
  或把整组转成 `.tbl`（复制组 → 粘贴为 .tbl Group）。
- 建组：`schema add-group --name G --storage xlsx`；之后 `add-table` / `add-constant` / `add-enum`
  自动落成 sheet（节点名 = sheet 名，必须 PascalCase 且 ≤31 字符）。
- GUI 开着时 CLI 会拒绝执行（工作区锁）；`util` / `list-templates` 除外。

## 五之三、校验规则（`# @rule`）

规则**只写在 `project.tblschema` 的段内**（不写 `.tbl`、不进 Excel），对 Table / Enum 生效；
Constant 不支持（`value` 逐行类型不同，写了会在解析期带行号报错）。

```text
[hero/Hero] table
id   | int | cs | 英雄ID
name | str | cs | 名字
kind | int | cs | 类型（引用 HeroType）
hp   | int | s  | 血量
atk  | int | s  | 攻击
# @rule: name = non_empty()
# @rule: kind = ref_exists(HeroType.id)
# @rule: hp = range(0, )
# @rule: hp = required_if(kind, 2)
# @rule: atk = compare(hp, <) [warn]
```

- 格式 `<列> = <验证器> [error|warn|info]`；一条一行，**行序即优先级**；级别缺省 `error`。
- 一列可以有多条规则；同一目标重复会被拒绝（表单禁用 + 校验层兜底）。

| 验证器 | 含义 | 适用列 |
|---|---|---|
| `unique()` | 非空值不重复 | 全部 |
| `non_empty()` | 必填（整行全空占位行不算） | 全部 |
| `ref_exists(节点.列)` | 值必须出现在目标列里 | 全部 |
| `range(下限, 上限)` | 数值区间，留空一侧不限 | 整数 / 浮点 |
| `len(下限, 上限)` | 字符数区间 | 文本 |
| `one_of(a, b, c)` | 必须是候选之一 | 全部 |
| `matches("正则")` | 匹配正则（整串用 `^...$`） | 文本 |
| `required_if(列, 值)` | 条件必填 | 全部 |
| `compare(列, 运算符)` | 跨列比较（两边都要是数字） | 整数 / 浮点 |

**系统内置规则自动生效，不要重复声明**：table 的 `id`（唯一 / 必填 / 正整数）、
enum 的 `id`（唯一 / 必填 / 正整数）、enum 的 `name`（唯一 / 必填 / UPPER_SNAKE）。
system 规则只读、不落盘；再挂一遍只会得到非阻断的 `RuleRedundant` 警告。

**无意义组合会直接报错**：`ref_exists` / `compare` / `required_if` 引用本列自己；
区间两端都空；`matches("")` 空正则；列类型与验证器不匹配（复合类型列不参与）。

- 改了被引用节点（`@X` 或 `ref_exists` 的目标）后，依赖它的节点会**自动重算**，不用手动全量验证。
- 阻断保存的级别由项目策略决定（`project.toml` 的 `[validate]`：error / warning / info
  各取 `block` / `report` / `ignore`，默认 error=block、warning=report、info=ignore）。
  不要手写 `project.toml`，用 GUI 的「项目设置 → 校验」。
- CI 用 `validate --strict`：所有「仅提示」级也算失败。

## 六、CLI 速查

```bash
tablet-cli [-w <workdir>] [--project <id>] [--fmt json] <命令>
```

| 目的 | 命令 |
|---|---|
| 看模板 | `list-templates` |
| 项目 | `project list` / `info` / `new` / `rename` / `delete --confirm` / `clone` |
| 结构 | `schema show [--rules]`；`add-group --name G [--storage tbl\|xlsx]`；`add-table --group G --name N [--tbl-fmt v2\|v3]`；`add-constant --group G --name N`；`add-enum --group G --name N --entry 1:X:说明` |
| 导出结构 | `schema export --out <file> [--group G] [--node G/N] [--with-preset]`；`schema export --as-template --meta-id ID` |
| 改名 / 删除 | `schema rename-group --old A --new B`；`schema rename-node --group G --old A --new B`；`schema delete-group --name G`；`schema delete-node --group G --name N` |
| 验证 | `validate [--group G] [--node N] [--strict]` |
| 导出 | `export data [--json\|--xml] [--group G] [--node N] [-o DIR]`；`export code --java\|--go\|--cpp\|--lua\|--gdscript\|--typescript[-client\|-server]\|--csharp[-dotnet\|-unity\|-godot]\|--all [-o DIR]`；`export all [-o DIR]` |
| Excel | `excel export --group G [-o f.xlsx]`；`excel import --group G --file f.xlsx` |
| 工作区 | `workspace info`（分支 / 修订 / 项目列表）；`workspace save`（同时刷新 `project.tblschema`） |

- `--fmt json` 只对查询类命令有效：`project list/info`、`schema show`、`schema export`、`validate`、`export`、`workspace info`、`sep show`。
- `--project` 覆盖 `last_project`；不写则跟随 `[project] last_project`。
- **`-w` / `--project` / `-s` 必须写在子命令前面**；`--fmt` 是全局参数，位置随意。
- `-o` 的**相对路径按项目根 `projects/<id>/` 算**（不是当前目录）；默认导出到 `projects/<id>/gen/`。
- `export code` 的 `-o`：只选一种格式 = 精确目录；选多种 = 输出根，按 `server/java`、`client/csharp_unity` 等标准子目录展开。
- `--switch-after false` 可让 `project new` 不修改 `last_project`。

## 六之二、Unity 插件 / 外部宿主调用

tablet 不内置 Unity 配置或命令。宿主（bat / Unity EditorWindow）自己调用：

1. 一次性把导出路径设好（GUI「项目设置 → 导出」）：
   - `[export.server] data_output` → JSON 落到 `<data_output>/json/`
   - `[export.client.csharp_unity] code_output` → C# 落到该目录
   - 绝对 / 相对都可以；**相对路径按 `projects/<id>/` 解析**。
2. 宿主调三条命令（`-w` / `--project` 在子命令前）：

```bash
tablet-cli -w <config仓库根> --project game validate
tablet-cli -w <config仓库根> --project game export data --json
tablet-cli -w <config仓库根> --project game export code --csharp-unity
```

3. 显示当前配置分支：`tablet-cli -w <config仓库根> --fmt json workspace info`
   （不加载 `.tbl`、不占工作区锁；Git / SVN 都支持）。

完整布局、`build_config.bat`、Unity `EditorWindow` 代码和失败表 → `references/unity.md`。

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
- ❌ 不要用中文导出码、不要给枚举或 table 的 id 用 0 / 负数、不要用关键字当字段名。
- ❌ 不要给系统已保证的列再挂 `unique()` / `non_empty()`（只会得到冗余警告）。
- ❌ 不要在 constant 段写 `# @rule`（解析期直接报错）。
- ❌ 不要用 `export code --csharp` 给 Unity：它会同时生成 dotnet / unity / godot 三套；
  用 `--csharp-unity` 只出 Unity。（多格式 `-o` 现在会按标准子目录展开，不会互相覆盖。）
- ❌ 不要在 Unity 按钮里跑 `workspace save` / `cargo build` / `git pull`；导出只读磁盘，按钮只做 validate + export。
- ❌ 不要把导出目录指到 `Assets/Scripts` 根：sync 会删除目录里未生成的 `.cs` / `.json`。
- ❌ 不要为了"兼容"去猜格式：读不懂就报错是有意为之。
- ❌ 不要跳过 `validate` 直接 `export` 交付。

## 九、仓库约定（.gitignore）

**必须提交**：`tablet.toml`、`projects/<id>/project.tblschema`、`projects/<id>/project.toml`（若存在）、
`projects/<id>/config/**`（`.tbl` 文件，以及 xlsx 组的 `config/<group>.xlsx`）。

**应忽略**（工具产生的临时物 / 派生产物）：

```gitignore
# 本机用户数据（列宽等）+ 进程锁；不进版本库
.data/
# 运行日志（写在 workdir 根）
tablet.log
tablet-crash.log
# Excel 桥接 / 编辑产生的临时工作簿（GUI 启动、退出时自动清理）
projects/*/.excel/
projects/*/.tbl-cache/
# xlsx 组保存时的中间文件
**/*.xlsx.tmp
# Excel / LibreOffice 打开工作簿时产生的锁文件
**/~$*.xlsx
**/.~lock.*#
# 导出产物（团队若要把生成代码入库，删掉这行或用 git add -f）
projects/*/gen/
# 系统垃圾
.DS_Store
Thumbs.db
```

`projects/*/gen/` 是 `export` 的产物，**不要手改**；要不要提交取决于你们是否让 CI 重新生成。

`.data/` 是每个 checkout 的本机用户数据（`user-data.json`：列宽等视图偏好）和进程锁
（`tablet.lock`），**不要提交**。

- **SVN 仓库**：用同一份清单 `svn propset svn:ignore -F .svnignore .`；`tablet-cli.exe` 可以直接提交，
  Git 仓库建议走 LFS。
- config 作为子模块时，工作区不要放进 `Assets/`，也不要在按钮里 `cargo build`。

## 十、交付前的自检

1. `tablet-cli --project <id> validate` 退出 0。
2. `export all` 退出 0，且日志里没有 `[警告]`。
3. 抽查生成的数据文件（`gen/server/data/json/*.json`）键值与设计一致。
4. 把「加了哪些段、多少行数据、用了哪些类型」用 3-5 条说清楚，不要复述整份文件。
