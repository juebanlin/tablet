# TBL 系列格式规范

本文档定义 TBL 系统的两种文件格式：

| 格式 | 扩展名 | 内容 | 用途 |
|------|--------|------|------|
| .tbl | `.tbl` | 结构 + 数据 | 运行时数据源，一个文件一张表 |
| .tblschema | `.tblschema` | 仅结构 | 结构定义/备份/测试，一个文件多张表 |

---

# 一、.tbl 数据文件

## 设计原则

| 原则 | 说明 |
|------|------|
| 文本化 | 纯文本，Git 友好，可 diff/merge |
| 行稳定 | 一行一条记录，增删改只影响对应行 |
| 自描述 | 文件头包含完整 schema 信息 |

## Table 模式

```
#!tbl v2
#mode table
#desc 英雄ID|名称|血量|技能组
#type int|str|int|Array<int>
#export 前后端|前后端|服务器|前后端
#field id|name|hp|skills
---
1001|战士|100|1;2;3
1002|法师|80|4;5
1003|弓手|90|6;7;8
```

主键固定为第一列 `id`（int 类型），不可更改。

表头行顺序（从上到下）：
1. `#desc` — 中文描述（策划可读）
2. `#type` — 字段类型
3. `#export` — 导出标记
4. `#field` — 字段名（紧贴数据行，方便程序摘取）

## Constant 模式

```
#!tbl v2
#mode constant
---
max_level|int|100||最大等级
start_pos|IntPair|5,10||出生坐标
gm_password|str|xxx|服务器|GM密码
```

数据行格式：`name|type|value|export|desc`

- export 列留空 = 默认"前后端"（双端导出）
- 仅需限制时填写对应选项

## 导出标记

| UI 显示 | 含义 |
|---------|------|
| 前后端 | 双端导出（默认） |
| 客户端 | 仅客户端导出 |
| 服务器 | 仅服务端导出 |
| 不导出 | 跳过该字段（纯注释/备忘列） |

## 头部指令汇总

| 指令 | 含义 |
|------|------|
| `#!tbl v2` | 格式版本标识 |
| `#mode` | table 或 constant |
| `#desc` | 字段中文描述（仅 table） |
| `#type` | 字段类型定义（仅 table） |
| `#export` | 导出标记（仅 table） |
| `#field` | 字段名（仅 table），第一列固定为 `id` |
| `---` | 头部与数据分隔符 |

## 数据行规则

- 字段用 `|` 分隔
- 字符串中的 `|` 转义为 `\|`，换行转义为 `\\n`
- 空值用空字符串表示
- 复合类型使用各自定义的分隔符（见类型系统）

## 行颜色标记

数据行支持行尾颜色标记，用于策划标注行状态（过时、测试、临时等），颜色含义由策划自行约定。

**格式：** 数据行末尾追加 ` #@c:RRGGBB`

```
1001|战士|100|1;2;3 #@c:FF0000
1003|弓手|90|6;7;8 #@c:CCCCCC
1010|刺客|85|9;10
```

**规则：**
- `#@c:` 后跟 6 位 hex RGB 值（大写），无 `#` 前缀
- 标记与数据之间用一个空格分隔
- 无标记 = 无背景色（默认白色）
- 解析时按最后一个 ` #@c:` 分割，取后 6 位作为颜色值
- 标记跟随行数据，插入/删除行时自动跟随，无需 ID 关联

**工作流：**
- 标色操作仅在 Excel 中进行（设置行背景色）
- UI 工具只读展示颜色，不提供标色入口
- Excel 回读时：行背景色非白色/无填充 → 提取 RGB 写入 `#@c:`；白色/无填充 → 移除标记

**Git diff 效果：**
```diff
-1001|战士|100|1;2;3
+1001|战士|100|1;2;3 #@c:FF0000
```

---

# 二、.tblschema 结构定义文件

## 定位

`.tblschema` 是 .tbl 系统的**结构定义文件**（类似 SQL 的 DDL），只描述组、表、字段的元信息，不包含数据行。

**用途：**

| 场景 | 说明 |
|------|------|
| 结构备份/恢复 | 导出项目所有表结构为 .tblschema，迁移或恢复时导入 |
| 测试数据生成 | 根据 schema 动态填充数据行，生成 .tbl + TestMain.java |
| 结构导入 | 拿到一个 .tblschema 即可批量创建表结构到项目中 |
| 结构对比 | 两个 .tblschema 文件 diff 即可看出结构变更 |

## 格式示例

```
#!tblschema v1

[hero/HeroBase] table
# name | type | export | desc
id     | int       | cs | 英雄ID
name   | str       | cs | 名称
hp     | int       | s  | 血量
skills | List<int> | cs | 技能组

[hero/HeroConst] constant
# name | type | export | desc
max_level | int            | cs | 最大等级
start_pos | Tuple2<int,int>| cs | 出生坐标

[global/GlobalConst] constant
version     | str | cs | 版本号
max_level   | int | cs | 等级上限
server_name | str | s  | 服务器名称
```

## 语法说明

### 文件头

```
#!tblschema v1
```

第一行必须是版本标识。

### Section 声明

```
[group/Name] mode [options]
```

- `group` — 组名（对应 config/ 下的子目录）
- `Name` — 配置项名（对应 .tbl 文件名，大写驼峰）
- `mode` — `table` 或 `constant`

Table 模式的第一个字段必须是 `id`（主键，int 类型）。

### 字段行

```
field_name | type | export | desc
```

用 `|` 分隔，每列含义：

| 列 | 说明 | 示例 |
|----|------|------|
| name | 字段名（snake_case） | `max_level` |
| type | 类型声明 | `int`, `List<int>`, `Tuple2<int,str>` |
| export | 导出标记缩写 | `cs`, `c`, `s`, `-` |
| desc | 中文描述 | `最大等级` |

### 导出标记缩写

| 缩写 | 含义 | 对应 .tbl |
|------|------|-----------|
| `cs` | 前后端 | 前后端（默认） |
| `c` | 仅客户端 | 客户端 |
| `s` | 仅服务器 | 服务器 |
| `-` | 不导出 | 不导出 |

### 注释与空行

`#` 开头的行为注释，解析时忽略。空行也忽略。

## 合并规则

多个 .tblschema 文件可以合并为一个完整的项目结构：

1. 按 `[group/Name]` 为 key 合并
2. 同一个 key 出现在多个文件中 → 报错（结构冲突）
3. 同一个 section 内字段名重复 → 报错
4. 不同 section 的字段名允许重复（不同表可以有同名字段）

## 内置测试 Schema

工具内置多套 .tblschema 文件用于集成测试，位于 `crates/core/schemas/`：

| 文件 | 用途 |
|------|------|
| `basic.tblschema` | 基础类型测试（int/str/bool/float） |
| `collection.tblschema` | 集合类型测试（List/Set/Map） |
| `tuple.tblschema` | 元组类型测试（Tuple2/3/4） |
| `empty.tblschema` | 空值策略测试（含空值字段） |
| `full.tblschema` | 完整项目结构（所有类型组合） |

测试流程根据 schema 文件：
1. 解析 .tblschema 得到表结构
2. 动态填充数据行，生成 .tbl 文件
3. 导出 JSON + Java 代码
4. 生成 TestMain.java（根据 schema 知道加载哪些类、验证哪些字段）
5. 编译运行，对比预期输出
