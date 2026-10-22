# 文件格式参考

两套文件，职责不同：

| 文件 | 位置 | 角色 |
|---|---|---|
| `.tblschema` | 独立的输入文件 / `projects/<id>/project.tblschema` | **建项目时**的结构 + 预设数据 |
| `.tbl` | `projects/<id>/config/<group>/<Name>.tbl` | 项目**运行时**的结构与数据的真相源 |

建项目之后，`.tbl` 说了算；`project.tblschema` 是 groups 的投影，`workspace save` 时重写。

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
[<group>/<Name>] <mode>
```

- `group`：`[A-Za-z0-9_一-鿿]+`，等价于 `config/` 下的目录名。
- `Name`：`[A-Z][A-Za-z0-9_]*`，等价于 `.tbl` 文件名与生成类名；全项目唯一（忽略大小写）。
- `mode`：`table` / `constant` / `enum`。

组名 / 段名在**解析时**就校验，非法会带行号报错。

### 段内可选指令

```text
# @display: name
```

写在段头之后。含义：别的表用 `@本段名` 引用时，UI / Excel 显示这一列而不是原始 id。
只对 table / enum 有意义；列必须是 `str` / `int` / `txt` 的独立列。

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

## 三、项目目录

```text
<workdir>/
├── tablet.toml                      工作区配置（全局默认 + last_project）
└── projects/<id>/
    ├── project.tblschema            结构投影（由 .tbl 生成）
    ├── project.toml                 可选的 [export] 覆盖
    ├── config/<group>/<Name>.tbl    数据与结构真相源
    ├── gen/                         导出默认落点
    └── .tbl-cache/                  Excel 桥接临时文件
```
