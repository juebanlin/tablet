# 文件格式参考

两套文件，职责不同：

| 文件 | 位置 | 角色 |
|---|---|---|
| `.tblschema` | 独立的模板文件 | **建项目 / 导入时**的结构 + 可选预设数据 |
| `.tbl` | `projects/<id>/config/<group>/<Name>.tbl` | 项目**运行时**的结构与数据的真相源 |

建项目之后，`.tbl` / workbook 说了算；`project.tblmanifest` 只记录节点索引、storage、规则与 display。

---

## 一、`.tblschema`（v1）

### 整体结构

```text
#!tblschema v1                  ← 第 1 行，必须逐字一致
# @meta id: demo
# @meta name: 演示项目
# @sep List = ,                 ← 一般不需要；这里只展示语法
                                ← 空行随意
[hero/HeroType] enum            ← 第一个段头；之后 # @meta / # @sep 失效
# @preset
1 | WARRIOR | 战士

[hero/HeroBase] table
id | int | cs | 英雄ID
name | str | cs | 名字

[global/GameConst] constant
# @preset
max_level | int | 100 | cs | 等级上限
```

### `# @meta`

格式 `# @meta <key>: <value>`（**冒号**分隔）。重复的 key 后者生效，未知 key 忽略。

| key | 说明 |
|---|---|
| `id` | `[a-z0-9_-]{1,32}`。不写也行；**写了就必须合法**，否则带行号报错 |
| `name` | 显示名。`project new --from-file` 且没给 `--name` 时，这个值会成为项目名 |
| `category` / `version` | 模板分类 / 版本，自由文本 |
| `created_at` / `source_template` / `source_template_version` | 由工具维护，手写无意义 |
| `has_preset` | 派生字段，**以实际 `# @preset` 为准**，手写无效 |

> 项目 id 最终由 `project new --id` 决定；schema 里的 `id` 会被覆盖。

### `# @sep`

格式 `# @sep <key> = <value>`（**等号**分隔，`Map.kv` 里本来就有冒号，所以不能用冒号）。
只写与默认不同的项；未知 key 忽略。34 个 key：

```text
List  Set  Tuple2  Tuple3  Tuple4  Map.kv  Map.entry
List_Tuple2.tuple  List_Tuple2.list  List_Tuple3.tuple  List_Tuple3.list
List_Tuple4.tuple  List_Tuple4.list
Map_Tuple2.kv  Map_Tuple2.tuple  Map_Tuple2.entry
Map_Tuple3.kv  Map_Tuple3.tuple  Map_Tuple3.entry
Map_Tuple4.kv  Map_Tuple4.tuple  Map_Tuple4.entry
Map_List.kv  Map_List.item  Map_List.entry
List_List.list  List_List.item
List_Map.list  List_Map.entry  List_Map.kv
Map_Map.kv  Map_Map.entry  Map_Map.inner_kv  Map_Map.inner_entry
```

默认值：`Tuple*` / 各类 `.tuple` / `List_List.item` / `List_Map.entry` / `Map_Map.inner_entry` = `,`；
`List` / `Set` / `List_List.list` / `List_Map.list` / 其余各类 `.entry` = `;`；
`.kv` / `List_Map.kv` / `Map_Map.kv` = `:`；`Map_Map.inner_kv` = `=`。
运行时还有另一层来源：工作区 `tablet.toml` 的 `[separators]`（schema 的 `# @sep` 优先）。
团队协作时**推荐保持默认**，少一处分歧。

### 段头

```text
[<group>/<Name>] <mode> [xlsx]
```

- `group`：`[A-Za-z0-9_一-鿿]+`，等价于 `config/` 下的目录名。
- `Name`：`[A-Z][A-Za-z0-9_]*`，等价于 `.tbl` 文件名与生成类名；全项目唯一（忽略大小写）。
- `mode`：`table` / `constant` / `enum`。
- `xlsx`（可选）：该组是工作簿组；导入时生成 `config/<group>.xlsx`。同组所有 section 必须一致。

组名 / 段名在**解析时**就校验，非法会带行号报错。

### 段内可选指令

```text
# @display: name
# @rule column key: unique()
# @rule column name: ref_exists(Lang.key) [warn]
# @rule table: all_non_empty(name, hp)
```

`# @display` 写在段头之后。含义：别的表用 `@本段名` 引用时，UI / Excel 显示这一列而不是原始 id。
只对 table / enum 有意义；列必须是 `str` / `int` / `txt` 的独立列。

`# @rule` 是节点级校验规则，两种作用域：

```text
# @rule column <列名>: <表达式> [error|warn|info]
# @rule table: <表达式> [error|warn|info]
```

- `column <列名>`：列级规则；列名是错误锚点，也是列域函数省略的主列参数。
- `table`：表级规则；可用逐行多列 / 全表函数，只能用于 Table。
- 一条一行，**行序即优先级**；级别缺省 `error`。
- 每条规则独立：一条 `# @rule` 内可以组合，跨规则不能写 `and/or`；整体结果按各规则级别聚合
  （error 阻断，warn / info 默认只报告）。
- 旧格式 `# @rule: <列> = ...` 已废弃，加载直接报错。

列域验证器（列级省略主列，表级显式）：

| 函数 | 列级 / 表级 | 含义 | 适用列 |
|---|---|---|---|
| `unique` | `unique()` / `unique(col)` | 非空值不重复（整数 `01` == `1`） | 全部标量 |
| `non_empty` | `non_empty()` / `non_empty(col)` | 必填（整行全空占位行不算） | 全部标量 |
| `ref_exists` | `ref_exists(节点.列)` / `ref_exists(col, 节点.列)` | 值必须出现在目标列 | 全部标量 |
| `range` | `range(min, max)` / `range(col, min, max)` | 数值区间，留空一侧不限 | 整数 / 浮点 |
| `len` | `len(min, max)` / `len(col, min, max)` | 字符数区间 | 文本 |
| `one_of` | `one_of(v...)` / `one_of(col, v...)` | 必须是候选之一 | 全部标量 |
| `matches` | `matches("正则")` / `matches(col, "正则")` | 匹配正则（整串用 `^...$`） | 文本 |
| `compare` | `compare(op, other)` / `compare(col, op, other)` | 跨列比较；`op` = `< <= > >= == !=` | 整数 / 浮点 |
| `positive_int` | `positive_int()` / `positive_int(col)` | 必须是正整数 | 整数 |
| `upper_snake` | `upper_snake()` / `upper_snake(col)` | 必须 UPPER_SNAKE_CASE | 文本 |

表级验证器（只能 `table:`）：

| 函数 | 含义 | 粒度 |
|---|---|---|
| `all_non_empty(col1, col2, ...)` | 这些列都必须有值 | 逐行 |
| `at_least_one(col1, col2, ...)` | 至少一列有值 | 逐行 |
| `exactly_one(col1, col2, ...)` | 恰好一列有值 | 逐行 |
| `row_count(min, max)` | 非占位行数在区间内 | 全表 |
| `id_contiguous()` | id 集合恰好是 `1..=N` | 全表 |
| `unique_combination(col1, ...)` | 列组合不重复 | 全表 |
| `no_empty_rows()` | 不允许整行全空占位行（预留，未实现） | 全表 |

**规则体成员**：规则体是一个组；组成员是函数调用，`unique(...)`、`range(...)`、
`when(条件, 规则表达式)` 都是**同一级**成员。`when` 不是组，也不是 `and/or` 那种连接符：
它过滤行集合（条件只读本节点当前行，不成立的行跳过验证器，全表函数也只统计命中的行），
可以放在组里与其他成员用 `and/or` 连接。`and/or` 是**组合子**，可组合条件，也可组合规则表达式
（规则级 `or` 目前只允许逐行粒度；规则级 `not` 不支持）。嵌套 `when` 等价于 AND。
`required_if(条件列, 值)` 是输入糖，加载时展开成 `when(eq(条件列, 值), non_empty(...))`，
不参与序列化；新规则直接用 `when`。

| 条件函数 | 用法 |
|---|---|
| `eq/ne/lt/le/gt/ge(a, b)` | 比较；`lt/le/gt/ge` 两边必须是数值 |
| `in(列, 值, ...)` | 列属于候选集合 |
| `match(列, "正则")` | 列匹配正则（与验证器 `matches` 区分） |
| `and(...)` / `or(...)` / `not(...)` | 组合条件；`and/or` ≥ 2 个参数 |

裸标识符 = 列，引号字符串 / 数字 = 字面量；反斜杠原样保留（`\d` 与 `\\d` 等价）。
**不支持跨表条件**。

- 只对 **Table / Enum** 有意义；**constant 段不支持**（`value` 逐行类型不同）。
- 只对**标量列**生效（`int` / `long` / `date` / `float` / `bool` / `str` / `txt` / `@Xxx`）；`List<>` / `Map<>` / `Tuple*` 等复合类型不参与。整数列（含 `date` 的毫秒值）按数值比较（`01` == `1`），其余按 trim 后字符串比较。
- 级别缺省 `error`；`[warn]` / `[info]` 是否阻断由项目策略决定（`project.toml` 的 `[validate]`，默认 warn=仅提示、info=忽略）。
- 规则只存 `project.tblmanifest`，不写进 `.tbl`，也不进 Excel。
- 一列 / 一张表可以有多条规则。系统已保证的约束（table/enum 的 `id` 唯一·必填·正整数、enum 的 `name`
  唯一·必填·UPPER_SNAKE）不要再手写，重复只会得到一条非阻断的冗余警告。
- `positive_int` / `upper_snake` 也开放给用户显式书写；系统内置仍自动生效。

### Table 段体

```text
[hero/HeroBase] table
id     | int    | cs | 英雄ID
name   | str    | cs | 名字
skills | List<int> | cs | 技能
```

- 每个字段一行，`|` 分隔，两侧空白会被去掉。
- 4 列：`字段名 | 类型 | 导出码 | 说明`；说明可省略，但**前 3 列不能省**。
- 第一列必须是 `id | int | ...`（名字固定 `id`，类型固定 `int`）。
- 字段名在同一张表内唯一。

### Constant / Enum 段体

段体必须为空，条目全部放进该段的 `# @preset`：

```text
[global/GameConst] constant
# @preset
max_level   | int             | 100  | cs | 等级上限
start_pos   | Tuple2<int,int> | 5,10 | cs | 出生坐标
```
```text
[hero/HeroType] enum
# @preset
1 | WARRIOR | 战士
2 | MAGE    | 法师
```

常量 5 列：`name | type | value | export | desc`（导出码同样只认短码）。
枚举 3 列：`id | name | desc`。

> 为什么强制：旧格式允许直接写在段体里，现在只保留一条路径——`# @preset`。
> 直接写在段体会报 "…段不允许直接的数据行"。

### `# @preset`

- 单独一行，必须写在某个段头之后；遇到下一个 `[` 或文件结束即终止。
- 行内按 `|` 切开，首尾空白去掉，支持 `\|`（字面竖线）、`\n`（换行）、`\\`。
- Table：每行一条记录，按字段顺序。
- Constant / Enum：见上面的列定义。
- **列数不能超过声明**：Table 超过字段数、Constant 超过 5 列、Enum 超过 3 列都会带行号报错
  （多出来的列会被丢弃，等于静默少数据）。少写可以，缺的位置按空处理。

### `.tblschema` 里的合法类型字符串

见 `types.md`。类型字符串本身也参与校验：`List<List<List<int>>>`、`Tuple5<...>`、`Set<Tuple2<...>>` 都会报错。

---

## 二、`.tbl`（数据文件）

### Table —— v3（块式，当前默认）

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

规则：

- 分隔线 `---` 之后才是内容区。
- `@field <name>` 开一个字段块，块内是 `desc:` / `export:` / `type:` 三行（缩进两格，可省）。
- **所有 `@field` 必须排在第一条记录之前**（解析器按已声明的字段名绑定记录）。
- `[<id>]` 开一条记录，方括号里的值就是 `id` 列。
- 记录里每行 `字段名:值`；只写非空字段，省略 = 空值。
- 值里的 `:` 不需要转义（按**第一个**冒号切）；`\` 与换行要转义；
  `|` 在 v3 里是普通字符、原样写（只有 v2 / `# @preset` 这种用竖线分列的布局才需要写成 `\|`）。
- `#` 开头的行是注释。行首 `#@color:RRGGBB` 是 Excel 往返用的行颜色标记，生成时不要写。
- **拼错会报错，不会静默丢数据**：记录里出现未声明的字段名、`@field` 里出现
  `desc` / `export` / `type` 之外的属性，都会带 `[记录 id]` / 字段名报错。

### 普通列 —— `@aux`（非 tablet 规范列）

xlsx 组里不合法的新增列会以普通列保存；`.tbl` 用同类块承载：

```text
@aux aux_1
  name:策划备注
  desc:内部备注
  export:随便
  type:任意类型
```

- `@aux <key>` 在记录前，和 `@field` 同级；记录行用 key（如 `aux_1:值`）；
- `name` 是原始表头，`type` / `export` 原样保存，不校验；
- 普通列不参与校验、引用、导出、代码生成；不要主动创建普通列，除非用户明确要求在
  `.tbl` 里保留一列非 tablet 数据。

### Constant / Enum —— v2（CSV）

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

常量 / 枚举固定用 v2 布局；它们没有 v3 形态。

### 表也可以是 v2（历史格式）

```text
#!tbl v2
#mode table
#desc 英雄ID|名字|血量
#export cs|cs|s
#type int|str|int
#field id|name|hp
---
1001|亚瑟|1200
```

读取端两种都认，**但新内容一律写 v3**。手写时用 v3，改完保存不会丢东西。

### 转义

| 字面量 | 写成 |
|---|---|
| 竖线 | `\|`（只在 `\|` 当分隔符的布局里需要） |
| 换行 | `\n` |
| 反斜杠 | `\\` |

`str` 类型的值**不允许**出现 `|`、换行、制表符（校验会拒绝）；需要这些字符请把列类型改成 `txt`。

---

## 三、Xlsx 组（工作簿形态）

一个组可以不用 `.tbl` 目录，而是 **`config/<group>.xlsx` 一个文件**：sheet 名 = 节点名，数据在 sheet 里，
**结构（字段 / 类型 / export / 顺序）就在 workbook 的 4 行表头里**；manifest 不含字段。

```text
Table sheet（4 行表头，数据从第 5 行起）
  A1: <desc>   A2: 前后端   A3: int   A4: id      A5: 1001
  B1: <desc>   B2: 前后端   B3: str   B4: name    B5: 亚瑟
```

```text
Constant sheet（1 行表头）
  name | type | value | export | desc
```
```text
Enum sheet（1 行表头）
  id | name | desc
```

- 字段按**表头里的字段名**对齐（策划可以在中间插私有列，工具不碰）；
- workbook 里少了 sheet（或整本 `<group>.xlsx` 被删）：启动仍成功，缺的节点 / 组会记警告
  并从 `project.tblmanifest` 移除；
- workbook 新加 sheet：首行是 constant / enum 固定表头，或 4 行表头里有合法 `id` 列，
  会自动登记为节点并补进 manifest；其余 sheet 视为私有，忽略；
- sheet 名 ≤31 字符，且不能含 `\ / ? * [ ] :`；
- **不要手写这个文件**：二进制、且要保留样式。用 `tablet-cli schema` / `tablet-cli excel` 生成或往返。

---

## 三·五、运行时清单 `project.tblmanifest`

项目运行时只有这一份“结构索引”，**不要手写**，由工具保存时生成：

```text
#!tblmanifest v1

# @meta id: demo
# @meta name: 演示项目

[hero/HeroBase] table
# @display: name
# @rule column id: unique()

[building/Building] table xlsx
```

- section 语法与模板相同，但没有字段行、没有 `# @preset`；读到会报错。
- 默认 `tbl`，`xlsx` 显式写；同组必须一致。
- 字段名 / 类型 / export / desc 来自 `.tbl` 表头或 workbook 4 行表头。
- 规则 / `# @display` / storage / 项目 meta 只住 manifest。
- 启动时发现节点数据文件不存在（`config/<group>/<Node>.tbl`、xlsx sheet，或整个组）：仍会启动，
  记警告并从 manifest 移除对应段；文件存在但读不懂（解析失败）仍报错。

---

## 四、项目目录

```text
<workdir>/
├── tablet.toml                      工作区配置（全局默认 + last_project）
└── projects/<id>/
    ├── project.tblmanifest            节点索引 / storage / 规则 / display（不含字段）
    ├── project.toml                 项目配置（[export] / [validate]，首次保存生成）
    ├── config/<group>/<Name>.tbl    Tbl 组：数据与结构真相源
    ├── config/<group>.xlsx          Xlsx 组：结构与数据真相源（4 行表头 + sheet）
    ├── gen/                         导出默认落点
    └── .tbl-cache/                  Excel 桥接临时文件
```
