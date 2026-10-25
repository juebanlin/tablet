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
只写与默认不同的项；未知 key 忽略。25 个 key：

```text
List  Set  Tuple2  Tuple3  Tuple4  Map.kv  Map.entry
List_Tuple2.tuple  List_Tuple2.list  List_Tuple3.tuple  List_Tuple3.list
List_Tuple4.tuple  List_Tuple4.list
Map_Tuple2.kv  Map_Tuple2.tuple  Map_Tuple2.entry
Map_Tuple3.kv  Map_Tuple3.tuple  Map_Tuple3.entry
Map_Tuple4.kv  Map_Tuple4.tuple  Map_Tuple4.entry
Map_List.kv  Map_List.item  Map_List.entry
```

默认值：`Tuple*` 与各类 `.tuple` = `,`；`List` / `Set` 与各类 `.list` / `.entry` = `;`；
`.kv` = `:`；`Map_List.item` = `,`。
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
# @rule: key = unique()
# @rule: name = ref_exists(Lang.key) [warn]
```

`# @display` 写在段头之后。含义：别的表用 `@本段名` 引用时，UI / Excel 显示这一列而不是原始 id。
只对 table / enum 有意义；列必须是 `str` / `int` / `txt` 的独立列。

`# @rule` 是节点级校验规则（格式 `<列> = <验证器> [error|warn|info]`，一条一行，行序即优先级）：

| 验证器 | 含义 | 适用列 |
|---|---|---|
| `unique()` | 该列非空值不重复 | 全部 |
| `non_empty()` | 必填（整行全空占位行不算） | 全部 |
| `ref_exists(节点.列)` | 值必须出现在目标列里 | 全部 |
| `range(下限, 上限)` | 数值区间，留空一侧不限 | 整数 / 浮点 |
| `len(下限, 上限)` | 字符数区间 | 文本 |
| `one_of(a, b, c)` | 必须是候选之一 | 全部 |
| `matches("正则")` | 匹配正则（整串用 `^...$`） | 文本 |
| `required_if(列, 值)` | 条件必填 | 全部 |
| `compare(列, 运算符)` | 跨列比较 | 整数 / 浮点 |

系统内置规则（`positive_int()` / `upper_snake()`，以及 id/name 上的 `unique()` / `non_empty()`）
是只读的：不写 schema、不可编辑、不可删除；再挂一遍只会得到冗余警告（@01.8.11）。
查看某个节点的有效规则：`tablet-cli schema show --rules`。

无意义组合直接报错：`ref_exists`/`compare`/`required_if` 不能引用本列自己；区间两端不能都空；
`matches("")` 空正则不接受。

- 只对 **Table / Enum** 有意义；**constant 段不支持**（`value` 逐行类型不同）。
- 只对**标量列**生效（`int` / `long` / `float` / `bool` / `str` / `txt` / `@Xxx`）；`List<>` / `Map<>` / `Tuple*` 等复合类型不参与。整数列按数值比较（`01` == `1`），其余按 trim 后字符串比较。
- 级别缺省 `error`；`[warn]` / `[info]` 是否阻断由项目策略决定（`project.toml` 的 `[validate]`，默认 warn=仅提示、info=忽略）。
- 规则只存 `project.tblmanifest`，不写进 `.tbl`，也不进 Excel。
- 一列可以有多条规则。系统已保证的约束（table/enum 的 `id` 唯一·必填·正整数、enum 的 `name`
  唯一·必填·UPPER_SNAKE）不要再手写，重复只会得到一条非阻断的冗余警告。

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

见 `types.md`。类型字符串本身也参与校验：`List<List<int>>`、`Tuple5<...>`、`Set<Tuple2<...>>` 都会报错。

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
- 值里的 `:` 不需要转义（按**第一个**冒号切）；`\` `|` 换行 按需转义。
- `#` 开头的行是注释。行首 `#@color:RRGGBB` 是 Excel 往返用的行颜色标记，生成时不要写。
- **拼错会报错，不会静默丢数据**：记录里出现未声明的字段名、`@field` 里出现
  `desc` / `export` / `type` 之外的属性，都会带 `[记录 id]` / 字段名报错。

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
- schema 有 section、workbook 没有同名 sheet → 项目打不开；workbook 多出来的 sheet → 忽略；
- sheet 名 ≤31 字符，且不能含 `\ / ? * [ ] :`；
- **不要手写这个文件**：二进制、且要保留样式。用 GUI / Excel / CLI 的 schema 命令。

---

## 三·五、运行时清单 `project.tblmanifest`

项目运行时只有这一份“结构索引”，**不要手写**，由工具保存时生成：

```text
#!tblmanifest v1

# @meta id: demo
# @meta name: 演示项目

[hero/HeroBase] table
# @display: name
# @rule: id = unique()

[building/Building] table xlsx
```

- section 语法与模板相同，但没有字段行、没有 `# @preset`；读到会报错。
- 默认 `tbl`，`xlsx` 显式写；同组必须一致。
- 字段名 / 类型 / export / desc 来自 `.tbl` 表头或 workbook 4 行表头。
- 规则 / `# @display` / storage / 项目 meta 只住 manifest。

---

## 四、项目目录

```text
<workdir>/
├── tablet.toml                      工作区配置（全局默认 + last_project）
└── projects/<id>/
    ├── project.tblmanifest            节点索引 / storage / 规则 / display（不含字段）
    ├── project.toml                 可选的 [export] 覆盖
    ├── config/<group>/<Name>.tbl    Tbl 组：数据与结构真相源
    ├── config/<group>.xlsx          Xlsx 组：结构与数据真相源（4 行表头 + sheet）
    ├── gen/                         导出默认落点
    └── .tbl-cache/                  Excel 桥接临时文件
```
