# tbl 系统

本文档定义 .tbl / .tblschema 文件的全部规则：文件结构、表头、类型范式、命名约束、值验证。所有面向 tbl 系统的硬性规则都在这一篇——其它文档不再重复，只做引用。

## 1. 设计原则

| 原则 | 说明 |
|------|------|
| 文本化 | 纯文本格式，Git 可 diff/merge |
| 行稳定 | 一行一条记录，增删改只影响对应行 |
| 自描述 | 文件头自带 schema 信息 |
| 类型内置 | 类型范式在工具里固定，配置只管分隔符；升级走工具新版本 |
| 引用平级 | `@Xxx` 与 `int / List<P> / Tuple2<P,P>` 在 schema 层都是同级 variant，不能进入嵌套 |
| 跨平台一致 | 同一份 .tbl 同时供 Java / Go / Lua 使用，零运行时分歧 |

## 2. 文件分两类

| 扩展名 | 内容 | 一文件存几张表 | 用途 |
|--------|------|-----------|------|
| `.tbl` | 结构 + 数据 | 一张 | 运行时数据源；策划日常编辑的对象 |
| `.tblschema` | 仅结构 | 多张 | 结构备份/迁移、生成测试数据、批量初始化项目 |

`.tbl` 由 `#mode` 决定具体子类（table / constant / enum，@4 详述）。`.tblschema` 是 schema-only 的 DDL，相当于 SQL 的「create table」。

## 3. Full .tblschema demo

```
#!tblschema v1

# Table：英雄基础表，主键 id 必须是第一列
[hero/HeroBase] table
id     | int       | cs | 英雄ID
name   | txt       | cs | 名称
hp     | int       | s  | 血量（仅服务端用）
type   | @HeroType | cs | 引用枚举：英雄类型
boss   | @HeroBase | cs | 引用同张表：克星
skills | List<int> | cs | 技能 id 列表
atk_def| Tuple2<int,int> | cs | 攻防双数
buffs  | Map<str,int> | cs | buff 名 → 数值

# Constant：全局常量，每行一个常量
[global/GlobalConst] constant
max_level   | int             | cs | 等级上限
start_pos   | Tuple2<int,int> | cs | 出生坐标
gm_password | txt             | s  | GM 密码（仅服务端）

# Enum：英雄类型枚举，三列固定 id|name|desc
[hero/HeroType] enum
1 | WARRIOR | 战士
2 | MAGE    | 法师
3 | ARCHER  | 弓手
```

读完这份 demo，你已经看过了 tbl 系统几乎所有结构元素：三种 mode、引用类型、嵌套类型、导出标记缩写、固定列约束。下面把每一块拆开来讲。

## 4. 三种 .tbl 模式

### 4.1 Table 模式

```
#!tbl v2
#mode table
#desc 英雄ID|名称|血量|英雄类型|技能组
#export 前后端|前后端|服务器|前后端|前后端
#type int|txt|int|@HeroType|List<int>
#field id|name|hp|type|skills
---
1001|战士|100|1|1;2;3
1002|法师|80|2|4;5
1003|弓手|90|3|6;7;8
```

- 表头四行**必须按顺序** `#desc → #export → #type → #field`（与 UI 表头从上到下一致）
- 第一列字段名固定为 `id`（int），不可改名、不可删除、不可移动
- 数据行从 `---` 之后开始，每行一条记录

### 4.2 Constant 模式

```
#!tbl v2
#mode constant
---
max_level  |int            |100  |   |最大等级
start_pos  |Tuple2<int,int>|5,10 |   |出生坐标
gm_password|str            |xxx  |s  |GM密码
```

- 没有 `#desc / #type / #export / #field` 头——5 列固定为 `name | type | value | export | desc`
- `export` 列留空 = 默认 `cs`（前后端）
- 允许 `@Xxx` 引用类型（默认开启；通过 `[ui] constant_ref_allowed = false` 可全局关闭，关闭后已存在的引用列报 schema 错误）

### 4.3 Enum 模式

```
#!tbl v2
#mode enum
---
1|WARRIOR|战士
2|MAGE|法师
3|ARCHER|弓手
```

- 三列固定 `id | name | desc`，**不需要** `#desc / #type / #export / #field` 头
- 枚举内容**写死在生成代码里**，不参与 JSON/XML 数据导出
- 加载时不需要数据来源——Java 反射用 enum 常量、Go 用 typed int + 包级常量、Lua 用 name→id table

### 4.4 模式速览对照

| 维度 | Table | Constant | Enum |
|------|-------|----------|------|
| 表头行 | 4 行（desc/export/type/field） | 0 行（5 列固定语义） | 0 行（3 列固定语义） |
| 主键 | 第一列必须是 `id: int` | 无（每行一个 name） | 第一列 `id: int+`（不为 0） |
| 引用 `@Xxx` | 允许 | 允许（默认开；`[ui] constant_ref_allowed` 关） | 不适用 |
| 数据导出 | JSON / XML | JSON / XML | 不导出（写入代码） |
| 跨平台代码 | Tpl 类（一行一实例） | Tpl 类（每行一字段） | enum / typed int / table |

## 5. 头部指令与表头规则

### 5.1 指令汇总

| 指令 | 含义 | 出现于 |
|------|------|--------|
| `#!tbl v2` | 格式版本标识，必须第一行 | 所有 .tbl |
| `#mode` | `table / constant / enum` | 所有 .tbl |
| `#desc` | 字段中文描述 | 仅 table |
| `#export` | 导出标记 | 仅 table |
| `#type` | 字段 [TblFieldType](#7-tblfieldtype-类型系统) | 仅 table |
| `#field` | 字段名（snake_case） | 仅 table |
| `---` | 头部与数据分隔符 | 所有 .tbl |

### 5.2 主键约束

- **Table**：第一列固定为 `id`（int），不可改名/移动/删除；数据行中 id 列值不能重复且不能为空（除非整行为空）
- **Constant**：无主键概念，但 name 列在同表内不重复
- **Enum**：第一列 id 必须是**正整数且不为 0**（0 保留给「未设置」语义）；id 在同枚举内不重复

### 5.3 导出标记

| .tbl 显示 | .tblschema 缩写 | 含义 |
|----------|-----------------|------|
| 前后端 | `cs` | 双端导出（默认） |
| 客户端 | `c` | 仅客户端 |
| 服务器 | `s` | 仅服务端 |
| 不导出 | `-` | 跳过该字段（纯注释） |

UI 内部统一存短码，渲染时翻译为中文标签。

### 5.4 文件名与配置项名

`.tbl` 文件名（去扩展名后）即配置项名，**必须遵循 Java 类名规则**（PascalCase，详见 @8.1）。同一项目跨组不能重名（忽略大小写）。

### 5.5 表头与列的可编辑性

UI 中表头单元格的可编辑性，按 mode 与位置严格区分。**只读单元格**渲染时背景置灰、不响应双击/单击编辑。

**Table 模式**

| 行 \\ 列 | id 列（首列） | 其它列 |
|----------|--------------|--------|
| desc | 可编辑 | 可编辑 |
| type | 只读（固定 `int`） | 可编辑 |
| export | 只读（固定 `cs`） | 可编辑 |
| field | 只读（固定 `id`） | 可编辑 |

id 列整列不可删除、不可移动、不可改名（仅 desc 行允许编辑中文描述）。其它列允许左右插入、删除、重命名。

**Constant 模式**

5 列固定表头 `name | type | value | export | desc`，全部只读，不可编辑、不可删除、不可移动。每行的数据（name/type/value/export/desc）才是用户编辑对象。

**Enum 模式**

3 列固定表头 `id | name | desc`，全部只读，不可编辑、不可删除、不可移动。

**视觉规则**

- 只读表头单元格：明显深一档的灰底 + 半灰文字（区别于普通表头的浅灰底 + 黑字）
- 鼠标悬停时不出现 hover 高亮
- 不响应双击/单击编辑
- 右键菜单中"删除列/重命名列"项对只读列禁用

## 6. 数据行规则

| 规则 | 说明 |
|------|------|
| 字段分隔 | 总是 `\|` |
| 复合类型分隔 | 由 `[separators]` 配置（详见 @7.4） |
| 字符串中的 `\|` | 转义为 `\\\|` |
| 字符串中的换行 | 转义为 `\\n` |
| 空值 | 写空字符串（`a\|\|c`），含义见 @8.4 |
| 连续分隔符 | **禁止**（`1;;3` 表示有空元素，验证拒绝） |
| 中文标点 | 按列类型分流，详见 @8.3.1 |
| 行尾颜色标记 | 详见 @9 |

## 7. TblFieldType 类型系统

`FieldDef.tbl_type` / `ConstEntry.tbl_type` 存的字符串都叫 **TblFieldType**，对应 Rust 内部 `tablet_core::types::TblType`：

```
TblFieldType ::= 范式
范式         ::= Base | Tuple2..4 | List | Set | Map
                | ListTuple2..4 | MapTuple2..4 | MapList
                | Ref(@Xxx)
```

UI 类型选择器分 [数据类型] / [引用类型] 两个 tab，但只是用户视角的业务分组——schema 层、序列化、代码生成对所有 variant 一视同仁。

### 7.1 基础类型（7 个）

| 类型 | 说明 | Java | Go | Lua |
|------|------|------|----|-----|
| int | 32 位整数 | int | int32 | number |
| long | 64 位整数 | long | int64 | number |
| float | 单精度浮点 | float | float32 | number |
| double | 双精度浮点 | double | float64 | number |
| str | 简单标识符（属性名、技能名、枚举值） | String | string | string |
| bool | 布尔 | boolean | bool | boolean |
| txt | 自由文本（描述、文案、JSON、HTML） | String | string | string |

**`str` vs `txt`**：

| 维度 | `str` | `txt` |
|------|-------|-------|
| 语义 | 简单标识符（如 property name） | 自由文本（如 desc / 文案 / JSON） |
| 可嵌套 | ✅ `List<str>` `Map<str,_>` `Tuple2<str,_>` 等 | ❌ 仅独立列 |
| 含 `\|` / `\n` / `\t` | ❌ 不允许 | ✅ 自动转义 |
| 含 `;` `,` `:` （分隔符） | ❌ 不允许（会破坏 split） | ✅ 允许（独立列不 split） |
| 含中文标点 | ❌ 拦截 | ✅ 允许 |
| 存储编码 | 不转义（Atom 路径） | 反斜杠转义（Text 路径） |
| 代码生成 | `string` / `String` | `string` / `String`（同 `str`） |

> 设计理由：`str` = 程序标识符（Java 属性名语义），`txt` = 人类可读文本。`List<str>` 的每个元素是标识符，不应含分隔符或换行；`txt` 列存完整文案，走转义保证 `\|` `\n` 等字符安全穿越。

### 7.2 元组与集合

| 范式 | Java | Go | Lua |
|------|------|----|-----|
| Tuple2\<P,P\> | int[]/Tuple2 类 | [2]P | {p1, p2} |
| Tuple3\<P,P,P\> | int[]/Tuple3 类 | [3]P | {p1, p2, p3} |
| Tuple4\<P,P,P,P\> | int[]/Tuple4 类 | [4]P | {p1, p2, p3, p4} |
| List\<P\> | List\<P\> | []P | {v1, v2, ...} |
| Set\<P\> | Set\<P\> | map[P]struct{} | {[v]=true, ...} |
| Map\<K,V\> | Map\<K,V\> | map[K]V | {k1=v1, k2=v2} |

约束：

- Set 元素必须是基础类型
- Map 的 K 不能是 bool（仅 `int / long / float / double / str`）
- Map 的 V 支持基础类型、元组、`List<P>`

### 7.3 14 种合法范式（按嵌套层数）

| 层级 | 范式 | 示例值 | 用到的分隔符 |
|------|------|--------|-------------|
| 0 | P | `100` / `hello` / `true` | — |
| 1 | Tuple2\<P,P\> | `5,10` | tuple |
| 1 | Tuple3\<P,P,P\> | `1,2,3` | tuple |
| 1 | Tuple4\<P,P,P,P\> | `1,2,3,4` | tuple |
| 1 | List\<P\> | `1;2;3` | list |
| 1 | Set\<P\> | `1;2;3` | set |
| 1 | Map\<P,P\> | `hp:100;mp:50` | kv + entry |
| 2 | List\<Tuple2\<P,P\>\> | `1,2;3,4;5,6` | tuple + list |
| 2 | List\<Tuple3\<P,P,P\>\> | `1,2,3;4,5,6` | tuple + list |
| 2 | List\<Tuple4\<P,P,P,P\>\> | `1,2,3,4;5,6,7,8` | tuple + list |
| 2 | Map\<P,Tuple2\<P,P\>\> | `atk:5,10;def:3,8` | kv + tuple + entry |
| 2 | Map\<P,Tuple3\<P,P,P\>\> | `hp:1,2,3;mp:4,5,6` | kv + tuple + entry |
| 2 | Map\<P,Tuple4\<P,P,P,P\>\> | `k:1,2,3,4;j:5,6,7,8` | kv + tuple + entry |
| 2 | Map\<P,List\<P\>\> | `hp:1,2,3;mp:4,5,6` | kv + item + entry |
| — | Ref（@Xxx） | `1001`（id） | — |

同一范式内每个 P 位置可独立选择基础类型（如 `Tuple2<int,str>` `Map<str,List<int>>`）。
> 注意：`txt` **不能进入任何复合类型**——仅 `Paradigm::Base` 允许 `txt`。嵌套字符串用 `str`。

### 7.4 分隔符配置

每种范式有自己的分隔符表，互不影响。**配置以项目自身的 `.tblschema` 为单一来源**：每个项目把自己使用的分隔符以 `# @sep key = value` 行嵌在 schema 头部（详见 @08.3）。加载项目时，schema 里的 `separators` 直接覆盖运行时分隔符；同一仓库不同项目可各自独立设定，互不干扰。

```
# @sep List = ;
# @sep Map.kv = :
# @sep Map.entry = ;
```

GUI 入口：项目右键 →「项目设置...」→「分隔符」tab（含 25 个 leaf）。修改后写回项目 `.tblschema` 并触发该项目全表重校验。

`tablet.toml [separators]` 段降级为「程序级默认值」：仅在「新建空项目」时把当前内存中的默认分隔符**拷贝**到新项目 schema 作为初值，之后该项目不再读取 toml；从模板/文件新建项目则继承 source schema 的 separators。

| 行为 | 数据来源 |
|------|---------|
| 已加载项目运行期 | 该项目 `.tblschema` 的 `# @sep` 行 → `Project.config.separators` |
| 新建空项目 | 启动时从 `tablet.toml [separators]` 读到内存 `engine.default_separators`，新项目拷贝此对象 |
| 从模板/文件新建项目 | source schema 的 `separators` |
| 运行期改分隔符 | 「项目设置 → 分隔符」→ 写回 `.tblschema` → revalidate_all |

序列化策略：`serialize_tblschema` **只输出与默认值不同的 leaf**——一个完全使用默认分隔符的项目，schema 里看不到任何 `# @sep` 行，保持文件干净。

支持的 25 个 leaf 见 @03.4.9（`SeparatorsSection` 字段）。键名规则：

- 顶级键：`Tuple2 / Tuple3 / Tuple4 / List / Set`
- 嵌套键：`Map.kv / Map.entry / List_Tuple2.tuple / List_Tuple2.list / Map_Tuple2.kv / ... / Map_List.entry`

### 7.5 引用类型 `@Xxx`

```
@TableName     # 引用一张 table
@EnumName      # 引用一个 enum
```

- 名字必须是有效配置项名（PascalCase，详见 @8.1）
- 数据文件层**永远存 id**（int），不存名字
- UI 渲染时可切换显示名字（详见 @04.5.2 功能区「枚举显示名字」开关）
- 不能引用 constant（无 id 概念）
- 不能进入嵌套：`List<@HeroType>` `Map<int,@Skill>` 都不合法

| 字段类型 | .tbl 存储 | Java 字段 | Go 字段 | Lua 字段 |
|---------|----------|----------|---------|---------|
| `@HeroType`（@enum） | int | `HeroTypeEnum`（fromId 加载） | `HeroTypeEnum`（typed int） | int（业务自查 enum 表） |
| `@HeroBase`（@table） | int | `int` | `int32` | `int` |

### 7.6 不支持的组合

| 组合 | 原因 |
|------|------|
| `List<List<P>>` | 外层与内层分隔符冲突 |
| `Map<P,Map<P,P>>` | 嵌套 Map 分隔符冲突 |
| `List<Map<P,P>>` | 同理 |
| `Set<Tuple>` | Set 元素只能基础类型 |
| `List<@HeroType>` 等引用嵌套 | 引用必须独立一档 |

要表达「列表内引用」语义，用 `List<int>` + 字段名/desc 说明。

### 7.7 跨语言值映射详表

| 范式 | .tbl 值 | Java | Go | Lua |
|------|---------|------|----|-----|
| int | `100` | int 100 | int32(100) | 100 |
| str | `hello` | "hello" | "hello" | "hello" |
| Tuple2\<int,int\> | `5,10` | int[]{5,10} | [2]int32{5,10} | {5,10} |
| Tuple2\<int,str\> | `1001,sword` | Tuple2(1001,"sword") | struct{int32,string} | {1001,"sword"} |
| List\<int\> | `1;2;3` | List.of(1,2,3) | []int32{1,2,3} | {1,2,3} |
| Set\<int\> | `1;2;3` | Set.of(1,2,3) | map[int32]struct{} | {[1]=true,[2]=true,[3]=true} |
| Map\<str,int\> | `hp:100;mp:50` | Map.of("hp",100,"mp",50) | map[string]int32 | {hp=100,mp=50} |
| List\<Tuple2\<int,int\>\> | `1,2;3,4` | List\<int[]\> | [][2]int32 | {{1,2},{3,4}} |
| Map\<str,Tuple2\<int,int\>\> | `atk:5,10;def:3,8` | Map\<String,int[]\> | map[string][2]int32 | {atk={5,10},def={3,8}} |
| Map\<str,List\<int\>\> | `hp:1,2,3;mp:4,5` | Map\<String,List\<Integer\>\> | map[string][]int32 | {hp={1,2,3},mp={4,5}} |

Lua 端全部展开为 table 字面量，不降级为 string。

### 7.8 代码生成统一不用包装类型

| .tbl 类型 | Java 字段 | Go 字段 |
|-----------|----------|---------|
| int | `int` | `int32` |
| long | `long` | `int64` |
| float | `float` | `float32` |
| double | `double` | `float64` |
| str | `String` | `string` |
| bool | `boolean` | `bool` |
| List\<int\> | `List<Integer>` | `[]int32` |
| Set\<int\> | `Set<Integer>` | `map[int32]struct{}` |
| Map\<str,int\> | `Map<String,Integer>` | `map[string]int32` |
| Tuple2\<int,int\> | `int[]` | `[2]int32` |

不使用 `Integer` / `*int` 等装箱/指针类型——业务代码无需判空。

## 8. 隐含约束与验证规则

所有 schema/数据上的硬性约束都在这一节，按 cell → row → schema → project 四个层级递进。

### 8.1 命名规则

| 标识符 | 规则 | 错误信息 |
|--------|------|----------|
| 字段名（Table 字段 / Constant name） | snake_case，小写字母开头，只含 `[a-z0-9_]`；不能是三语言关键字（@8.6） | "不是合法字段名" / "是语言关键字" |
| 配置项名（Table / Constant / Enum 文件名） | PascalCase（Java 类名规则）；大写字母开头，只允许 `[a-zA-Z0-9_]`；项目内跨组不重名（忽略大小写） | "配置项名必须符合Java类名规则" / "配置项名重复" |
| 组名（目录名） | 允许中英文/数字/下划线 `[a-zA-Z0-9_一-鿿]`；不重名（忽略大小写） | "组名只能包含中英文数字下划线" / "组名重复" |
| 枚举条目名 | UPPER_SNAKE_CASE：大写字母开头，只含 `[A-Z0-9_]`；同枚举内不重名；不能是关键字 | "不是合法枚举条目名" / "枚举条目名重复" / "是语言关键字" |

字段名生成时各语言自动转换：
- Java → camelCase（`max_level → maxLevel`）
- Go → PascalCase（`max_level → MaxLevel`）
- Lua → 保持原样

枚举条目名各语言**直接使用原 name**，无需转换：
- Java：`HeroTypeEnum.WARRIOR`
- Go：`HeroTypeEnum_WARRIOR`
- Lua：`HeroType.WARRIOR`

> Group 名仅在工具内部使用（树形导航 + 配置目录名），**不出现在生成代码、import 路径、数据文件路径里**——所以允许中文。详见 @02.3「目录扁平化」。

### 8.2 Table 验证

| 层级 | 规则 | 错误信息 |
|------|------|----------|
| cell | id 列必须是整数 | "ID必须是数字" |
| cell | 其它字段值按 [TblFieldType](#7-tblfieldtype-类型系统) 验证（详见 @8.3） | 视类型而定 |
| row | 非 id 列有值但 id 为空 | "有数据但ID为空" |
| schema | 字段列表非空 | "表没有定义任何字段" |
| schema | 第一列必须是 `id` | "第一列必须是主键 id" |
| schema | 字段名合法且不重复（同表内） | 见 @8.1 |
| schema | 类型字符串可被解析 | "类型 \"X\" 不合法" |
| schema | 引用类型有效（被引用项存在且非 constant） | 见 @8.5 |
| project | 主键 id 在同表内不重复 | "第N行主键值 \"X\" 重复" |

### 8.3 类型值验证

| 检查项 | 说明 | 错误示例 |
|--------|------|----------|
| 基础类型格式 | int/long 必须整数；float/double 必须数字；bool 必须 `true/false`；str 不验证 | `"abc"` 作 int |
| 元组元素数量 | Tuple2 恰好 2 个，Tuple3/4 同理 | `"1,2,3"` 作 Tuple2 |
| 元组元素类型 | 每个位置匹配对应基础类型 | `"1,abc"` 作 Tuple2\<int,int\> |
| 集合元素类型 | List/Set 每元素匹配 | `"1;abc;3"` 作 List\<int\> |
| Map 格式 | 每条目带 kv 分隔符；K/V 类型匹配 | `"hp100"` 缺 `:` |
| 空元素 | 不允许连续分隔符 | `"1;;3"` `"1,,2"` |
| 中文标点 | 按列角色分流，见 @8.3.1 | `"1，2，3"` 作 List\<int\> |

错误信息自动附带示例（用当前配置的分隔符）：

```
[验证] hero/HeroBase D2: "4,5" 列表元素类型不匹配, 示例: 1;2;3
```

#### 8.3.1 中文标点 / 特殊符号拦截规则

中文标点 `，；：、` 等是否拦下来，取决于**该单元格的语义角色**——不一刀切按"是不是字符串"。核心区分：

> **进代码 / 要被分隔符切 → 拦；自由文案 → 放开。**

| 位置 | 拦截 | 原因 |
|------|:---:|------|
| 表头任意行（desc / export / type / field 行） | ✅ | 字段名进代码生成（变量名 / 类型签名）；type 行进 TblType 解析；export 行进枚举字面 |
| Table 数据格 — int / long / float / double / bool 类型 | ✅ | 基础类型 parse 错位 |
| Table 数据格 — 复合类型（Tuple / List / Set / Map / List\<Tuple\> 等） | ✅ | 中文逗号 ≠ ASCII 分隔符，会让 split 错位 |
| Table 数据格 — `@TableName` / `@EnumName` 引用 | ✅ | 实际值是 int(id)，按 int 校验 |
| **Table 数据格 — `txt` 类型**（自由文本：desc 列、name 列、文案列、JSON/HTML/XML） | ❌ 放开 | 玩家可见的自由文本，必须允许中文逗号 / 全角空格 / 引号等 |
| Table 数据格 — `str` 类型（独立列或嵌套） | ✅ | `str` = 简单标识符，不能含特殊字符（见 §7.1 str vs txt） |
| Constant value 列 — `txt` 类型 | ❌ 放开 | 自由文本 |
| Constant value 列 — `str` / 非 str 类型 | ✅ | 同数据格规则 |
| Constant name 列 | ✅ | name 生成成代码端常量名（`MAX_LEVEL`），ASCII identifier 约束 |
| Constant desc 列 | ❌ 不验证 | 注释 / tooltip，不进代码 |
| Constant export 列 | ❌ 不验证 | 取值固定 `cs/c/s/-`，schema 层兜底 |
| Enum id 列 | ✅ | 必须是正整数 |
| Enum name 列 | ✅ | 生成成 enum variant（`enum HeroType { WARRIOR }`），UPPER_SNAKE 严格约束 |
| Enum desc 列 | ❌ 不验证 | 同 Constant desc |

边界示例：

| 场景 | 列类型 | 值 | 结果 |
|------|--------|-----|------|
| Table desc 列填策划文案 | `str` | `步兵基础兵种，克制骑兵` | ✅ 通过 |
| Table desc 列填 M1 编号 | `str` | `M1：[步兵]` | ✅ 通过 |
| Table 等级列 | `int` | `１００`（全角数字） | ❌ 拦 |
| Table 技能 ID 列表写错分隔符 | `List<int>` | `1，2，3`（中文逗号） | ❌ 拦（分隔符错位） |
| `List<str>` 列写中文逗号 | `List<str>` | `战士，法师，弓手` | ❌ 拦（仍是分隔符错位，应写 `战士;法师;弓手`） |
| Constant value（str 类型） | `str` | `欢迎来到，主城！` | ✅ 通过 |
| Enum name 列 | `str` | `战士`（中文） | ❌ 拦（必须 UPPER_SNAKE，前置规则） |

注意 `List<str>` / `Tuple<str, _>` 等**含 str 的复合类型仍然拦中文逗号**——值会按分隔符 split，写错分隔符是真错误，不是文案。豁免只针对"完整自由文本格"形态：`Paradigm::Base + BaseType::Str`。

实现位置：`tablet_core::types::TblType::validate_value`（`types.rs`）入口处 `is_plain_str` 分流；`# @sep` 行的分隔符配置不影响该规则。

### 8.4 空值规则

| 类型分类 | 允许空值 | 加载时默认值 | JSON 表达 |
|---------|---------|-------------|-----------|
| 基础类型 | ✅ | int=0 / long=0 / float=0.0 / double=0.0 / str="" / bool=false | null |
| 集合类型（List/Set/Map/嵌套） | ✅ | 空集合（`[]` / `{}`） | null |
| 元组类型（Tuple2/3/4） | ❌ | — | — |
| 引用 `@Xxx` | ✅（视作"未引用"） | int=0 | null |

元组**不允许为空**——元组通常表示结构化数据（坐标、奖励），零值可能有业务含义，必须显式填写。

### 8.5 引用 `@Xxx` 验证

| 被引用项状态 | 处理 | 错误信息 |
|-------------|------|----------|
| 不存在 | schema 非法 | "引用的配置项 X 不存在" |
| `mode = enum` | 值必须是该 enum 的某个 id | "引用值 5 不存在于 HeroType" |
| `mode = table` | 值必须是该表存在的 id | "引用值 1099 不存在于 HeroBase" |
| `mode = constant` | schema 非法（constant 没有 id 概念，不可被引用） | "不能引用 constant" |
| Constant 字段使用 `@Xxx` 但 `[ui] constant_ref_allowed = false` | schema 非法 | "Constant 不允许引用类型（已在 [ui] 关闭）" |

引用值为空合法（视作"未引用"）。

被引用对象删除/重命名时，工具检测谁还在引用，给警告（不阻断保存），用户自行清理。

### 8.6 三语言关键字（合并保留集）

字段名、Constant name、Enum 条目名都不允许使用以下任一关键字：

**Java**：abstract, assert, boolean, break, byte, case, catch, char, class, const, continue, default, do, double, else, enum, extends, final, finally, float, for, goto, if, implements, import, instanceof, int, interface, long, native, new, package, private, protected, public, return, short, static, strictfp, super, switch, synchronized, this, throw, throws, transient, try, void, volatile, while, true, false, null

**Lua**：and, break, do, else, elseif, end, false, for, function, goto, if, in, local, nil, not, or, repeat, return, then, true, until, while

**Go**：break, case, chan, const, continue, default, defer, else, fallthrough, for, func, go, goto, if, import, interface, map, package, range, return, select, struct, switch, type, var

### 8.7 自动修正

| 场景 | 处理 |
|------|------|
| Constant name 编辑/粘贴 | 自动 trim 两端空格、去除中间空格 |

### 8.8 Constant 验证

| 层级 | 规则 |
|------|------|
| cell | name 符合命名规则；value 按类型验证 |
| row | name 已填但 value 为空 → "name已填但value为空" |
| schema | name 不重复；类型可解析；`@Xxx` 引用类型受 `[ui] constant_ref_allowed` 开关控制（默认开启；关闭时报 schema 非法） |

### 8.9 Enum 验证

| 层级 | 规则 |
|------|------|
| cell | id 是正整数且非 0；name 符合 UPPER_SNAKE_CASE |
| row | name 已填但 id 为空 → "name已填但id为空" |
| schema | 至少一个条目；id/name 不重复 |

### 8.10 待办（已规划未实现）

- Set 值不重复约束

## 9. 行颜色标记

数据行支持行尾颜色标记，由策划用作过时/测试/临时等状态标注。颜色含义不规定，由项目自行约定。

```
1001|战士|100|1;2;3 #@c:FF0000
1003|弓手|90|6;7;8 #@c:CCCCCC
1010|刺客|85|9;10
```

| 规则 | 说明 |
|------|------|
| 格式 | 数据行尾追加 ` #@c:RRGGBB`（6 位 hex 大写，无 `#` 前缀） |
| 分隔 | 标记与数据之间一个空格 |
| 无标记 | 默认无背景色 |
| 解析 | 按最后一个 ` #@c:` 切分，取后 6 位 |
| 跟随行 | 插入/删除行时自动跟随，无需 ID 关联 |

工作流：
- 标色操作仅在 Excel 中进行（设置行背景色，详见 @06）
- UI 工具只读展示，不提供标色入口
- Excel 回读时：行背景非白色/无填充 → 写入 `#@c:`；白色/无填充 → 移除标记

## 10. .tblschema 详解

### 10.1 文件头

```
#!tblschema v1
# @meta id: full
# @meta name: 完整测试模板
# @meta category: test
# @meta version: 1.0.0
# @meta created_at: 2026-06-03T10:30:00Z
# @meta source_template: full
# @meta source_template_version: 1.0.0
# @meta has_preset: true
# @sep List = ;
# @sep Map.entry = ;
```

第一行必须是版本标识。紧随其后的 **directive 行** —— `# @meta` 与 `# @sep` —— 在第一个 `[group/Name]` 之前出现，定义 schema 元数据与项目分隔符配置。

| 字段 | 约束 | 用途 |
|------|------|------|
| id | `[a-z0-9_-]{1,32}` | 程序内唯一标识，文件名约定 `<id>.tblschema` |
| name | 任意文本（含中文） | UI 展示文案 |
| category | 任意文本 | 模板库分类筛选（test / slg / rpg / ...）|
| version | semver | 模板版本，后续模板更新比对用 |
| created_at | ISO-8601（UTC） | 项目创建时间。模板侧通常为空 |
| source_template | 模板 id | 项目从哪个模板新建。手动新建 / 模板自身留空 |
| source_template_version | semver | 来源模板版本（模板更新比对锚点） |
| has_preset | `true` / `false` | 是否含 `# @preset` 数据块（详见 @08.4）。**derive 字段**：序列化时按实际 sections 重算，反序列化也以 section 实际状态为准 |

兼容规则：

- 老 .tblschema 无 `# @meta` 行 → id 走文件名 stem 兜底（`full.tblschema` → `id=full`），name 等于 id
- 重复 key 后者覆盖前者
- key 大小写敏感
- `# @meta` / `# @sep` directive 行必须出现在第一个 `[group/Name]` 之前；之后的 `#` 行视作普通注释（`# @preset` 例外，见 @08.4）

UI 上是否展示 id 还是 name 由全局开关 `[ui] show_meta_id` 决定（@04.5.x、@03.4.10），默认 false 显示 name。

### 10.2 Section 声明

```
[group/Name] mode [options]
```

- `group` — 组名
- `Name` — 配置项名（PascalCase）
- `mode` — `table` / `constant` / `enum`

### 10.3 分隔符 directive `# @sep`

格式：`# @sep <key> = <value>`

- key 取自 `SeparatorsSection` 的 25 个 leaf（@7.4）：`Tuple2 / Tuple3 / Tuple4 / List / Set / Map.kv / Map.entry / List_TupleN.tuple / List_TupleN.list / Map_TupleN.{kv,tuple,entry} / Map_List.{kv,item,entry}`
- value 等号两侧 trim，**值本身不再 trim 内部空格**（保留原样以支持空白类分隔符）
- 必须出现在第一个 `[group/Name]` 之前；之后的 `# @sep` 视作普通注释（与 `# @meta` 同等约束）
- 未列出 / 未识别的 key 静默忽略，前向兼容
- 序列化时**只输出与 `SeparatorsSection::default()` 不同的 leaf**——默认配置的项目 schema 中不会出现任何 `# @sep` 行

加载链路：每个 Project 用自身 schema 的 separators 覆盖 `Project.config.separators`，运行期校验/导出/示例值生成全部读 config，与 workspace toml 的 `[separators]` 解耦（@7.4 / @03.4.9）。

### 10.4 字段行

```
field_name | type | export | desc
```

| 列 | 说明 | 示例 |
|----|------|------|
| name | 字段名（snake_case） | `max_level` |
| type | [TblFieldType](#7-tblfieldtype-类型系统) | `int` / `List<int>` / `Tuple2<int,str>` / `@HeroType` |
| export | 短码 | `cs` / `c` / `s` / `-` |
| desc | 中文描述 | `最大等级` |

Enum mode 的数据行为 `id | name | desc`，无 type/export 列。

### 10.5 预设数据 directive `# @preset`

格式：在 `[group/Name] mode` 段内、字段行之后插入独立 `# @preset` 行；之后的非 `#` / 非 `[` 行按 `|` 切作为该段的预设数据，直到下一个 `[group/Name]` 或 EOF。

```
[hero/HeroBase] table
id     | int       | cs | 英雄ID
name   | str       | cs | 名称
type   | @HeroType | cs | 引用枚举：英雄类型
skills | List<int> | cs | 技能 id 列表
# @preset
1001 | 战士 | 1 | 1;2;3
1002 | 法师 | 2 | 4;5

[global/GlobalConst] constant
# @preset
max_level | int             | 100  | cs | 最大等级
start_pos | Tuple2<int,int> | 5,10 | cs | 出生坐标

[hero/HeroType] enum
# @preset
1 | WARRIOR | 战士
2 | MAGE    | 法师
3 | ARCHER  | 弓手
```

按 mode 不同，预设行的列含义如下：

| Mode | 预设行格式 |
|------|-----------|
| Table | `<id> | <字段2> | <字段3> | ...`（按 schema 字段顺序） |
| Constant | `<name> | <type> | <value> | <export> | <desc>` |
| Enum | `<id> | <name> | <desc>` |

行为约束：

- 必须出现在 `[group/Name]` 之后，否则解析失败（`# @preset 必须出现在某个 [section] 之后`）
- Constant / Enum 段的字段/条目**只能写在 `# @preset` 块里**——schema 主体不允许直接的数据行
- 加载到项目时，`apply_schema_to_project(with_preset)` 决定是否把预设数据灌入新建 Project 的 .tbl
  - 「新建项目」对话框带 `with_preset` 复选框（仅当 `has_preset = true` 时可见）
  - 「合并 Schema」/「导出 Schema」对话框同样可选是否带 preset
- `# @meta has_preset` 字段是 derive：序列化前按实际 sections 重算，反序列化时也以解析到的 preset 行为准（无论 meta 行写什么都会被覆盖）

预设数据是模板「开箱即用」的核心——例如三国题材模板 `sanguo.tblschema` 把所有英雄 / 兵种 / 建筑数据都打进 preset，新建项目时一键生成。

### 10.6 注释与空行

`#` 开头行 = 注释（除 `# @meta` / `# @sep` / `# @preset` 三种 directive）。空行 = 忽略。

### 10.7 多文件合并

| 规则 | 处理 |
|------|------|
| 按 `[group/Name]` 为 key | 合并 |
| 同 key 出现在多个文件 | 报错（结构冲突） |
| 同 section 内字段名重复 | 报错 |
| 不同 section 同字段名 | 允许 |
| metadata / separators | 合并产物清空（多文件无单一来源），调用方按需重填 |

### 10.8 内置测试 schema

工具内置多套 .tblschema 用于回归测试与新建项目模板，位于 `crates/core/schemas/`：

| 文件 | 用途 |
|------|------|
| `standard.tblschema` | 标准范式覆盖：所有类型组合 + Constant/Enum，generate-test 默认源 |
| `sanguo.tblschema` | 三国题材完整 demo：含 10 套枚举 + 8 张 Table + 1 张 Constant，全部带 `# @preset` 数据 |

后续可扩展：文件直接放进 `crates/core/schemas/`，`build.rs` 自动重编。模板加载详见 @02 项目模板章节。

测试流程详见 @10。

## 附录 A. 验证架构

工具内部以**四层函数**实现验证，所有层都返回统一的 `ValidationError` 结构：

```
validate_*_cell(node, row, col)      → Option<ValidationError>   单元格级
validate_*_row(node, row)            → Vec<ValidationError>      行级（调用 cell + 行逻辑）
validate_*_schema(node)              → Vec<ValidationError>      schema 级（结构完整性，row=SCHEMA_ROW）
validate_*(node, sep, refs?)         → Vec<ValidationError>      整表级（schema + 行 + 跨行唯一性）
revalidate / revalidate_all(...)     → 更新 errors 集合           项目级（遍历所有节点 + 跨表引用）
```

`*` 是 `table` / `constant` / `enum` 之一。

整表层是 **节点级聚合入口**，几乎所有上层（UI、日志、ops）调用它就能拿到一个节点的全部错误。节点级 `revalidate` 只负责把整表层结果写入 `validation_errors` 索引。

| 层 | 触发时机 | 内容 |
|---|---------|------|
| cell | 编辑提交、`[ui] realtime_validate=true` 时键入 | 单值类型/格式/命名 |
| row | 同上 | cell 验证 + 行内一致性（如 name 已填 id 为空） |
| schema | 保存前、导出前 | 字段列表完整性、字段名不重 |
| 整表（节点级） | 同上 | schema + 所有行 + 跨行唯一性（id/name 重复） |
| 项目级 | 加载、保存、reload、导出 | 整表层 × N 节点 + RefIndex 跨表引用 |

### A.1 ValidationError 结构

整表层与项目级输出的统一结构（HTTP response 风格：状态码 + 上下文）：

```rust
struct ValidationError {
    code: ValidationCode,       // 预定义错误类型枚举（FieldNameKeyword / TypeInvalid / DuplicateId / ...）
    row: usize,                 // 数据行索引；SCHEMA_ROW (= usize::MAX) 表示表头层错误
    col: usize,                 // 0-based 列索引
    header_row: Option<TableHeaderRow>,  // 仅 Table schema 错误有意义
    field: String,              // 出错字段/常量/枚举条目名
    value: String,              // 出错单元格的值（数据行错误）
    message: String,            // 人类可读消息（不参与日志格式判断，仅用于显示）
}
```

### A.2 行/列范式枚举

为避免 0-based / 1-based 混用导致的歧义，固定范式编号统一以"第 N 行 / 第 N 列"语义（1-based）。内部用作下标必须调用 `.row()` / `.col()`（返回 0-based）：

| 枚举 | 用途 | 1-based 编号 = 第几行 / 第几列 |
|------|------|------------------|
| `TableHeaderRow` | Table 表头行号（4 行 UI 顺序） | 1=Desc, 2=Export, 3=Type, 4=Field |
| `ConstantCol` | Constant 行内列号（5 列固定范式） | 1=Name, 2=Type, 3=Value, 4=Export, 5=Desc |
| `EnumCol` | Enum 行内列号（3 列固定范式） | 1=Id, 2=Name, 3=Desc |

这三个枚举的命名差异（Row vs Col）反映了三种 mode 的**数据模型差异**：

- **Table 是表格型数据**：动态多列（用户定义的若干字段），但每个字段的元数据（描述/导出/类型/字段名）固定为四行 schema 头——所以用 `TableHeaderRow` 标识这四行表头中的某一行。
- **Constant / Enum 是离散型数据**：每条记录就是一行，行内列固定（5 列 / 3 列），没有可变字段——所以用 `ConstantCol` / `EnumCol` 标识行内的列位置。

`TableHeaderRow` 的编号顺序与 UI 表头从上到下一致，也与 `.tbl` 文件 `#desc → #export → #type → #field` 的物理顺序一致。

### A.3 RefIndex（跨表引用索引）

`RefIndex` 是项目级跨表索引，构建一次后供 `validate_ref_value` / `validate_ref_type` 查询：

```rust
RefIndex {
    map: HashMap<String, (RefKind, HashSet<String>)>
    // name → (Table | Enum | Constant, 该项的有效 id 集合)
}
```

### A.4 validation_errors 索引维护

`ProjectEngine.validation_errors: HashSet<(group, name, row, col)>` 是 UI 红框 / 树节点 `!` 的数据源。维护时机：

| 操作 | 索引变化 |
|------|----------|
| 加载 / reload / generate_test / clear | `revalidate_all` 重建全量 |
| 单元格 / 表头编辑提交 | `revalidate(group, name)`，仅当 `realtime_validate=true` |
| 保存 (`save_all`) | 内部 `revalidate_all`，与开关无关 |
| 新建节点（NewTable / NewConstant / NewEnum） | `revalidate(group, name)`，与开关无关 |
| 重命名 group / 节点 | (old_key) → (new_key) 平移 |
| 拷贝节点 (`copy_node`) | `revalidate(group, new_name)` |
| 删除节点 / 删除 group | 按 key retain 清除残留 entries |
| 行/列结构操作（insert/delete row+col、粘贴、清空） | `revalidate(group, name)`，仅当 `realtime_validate=true` |

`realtime_validate` 开关行为详见 @03.4.10.2。

### A.5 日志格式

项目级验证（`ProjectEngine::validate`）输出统一格式 `位置:[内容] -> 原因`：

```
[验证] hero/HeroBase 表头第3行D列:[type] -> "type" 是 Go 关键字
[验证] hero/HeroBase C3:[abc] -> 不是合法int, 示例: 1
```

- 表头错误：`表头第N行X列:[字段名] -> 原因`（Table，N 为 1–4）
  - Constant/Enum 表头无 N：`表头X列:[字段名] -> 原因`
- 数据行错误：`<列字母><行号>:[内容] -> 原因`，内容超过 16 字符截断为 `xxx...`

UI 上的视觉反馈（红框、树节点 `!`、日志框）见 @04.5.4。验证开关配置见 @03.4.10。
