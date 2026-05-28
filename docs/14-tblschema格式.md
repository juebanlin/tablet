# .tblschema 结构定义格式

## 定位

`.tblschema` 是 .tbl 系统的**结构定义文件**（类似 SQL 的 DDL），只描述组、表、字段的元信息，不包含数据行。

**用途：**

| 场景 | 说明 |
|------|------|
| 结构备份/恢复 | 导出项目所有表结构为 .tblschema，迁移或恢复时导入 |
| 测试数据生成 | 根据 schema 动态填充数据行，生成 .tbl + TestMain.java |
| 结构导入 | 拿到一个 .tblschema 即可批量创建表结构到项目中 |
| 结构对比 | 两个 .tblschema 文件 diff 即可看出结构变更 |

## 格式规范

```
#!tblschema v1

[hero/HeroBase] table index=id
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
- `options` — 可选参数，空格分隔的 key=value 对
  - `index=field_name` — 主键字段名（仅 table 模式必填）

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

### 注释

`#` 开头的行为注释，解析时忽略。空行也忽略。

```
# 这是注释
[hero/HeroBase] table index=id
# name | type | export | desc    ← 这行也是注释（表头提示）
id     | int  | cs     | 英雄ID
```

## 合并规则

多个 .tblschema 文件可以合并为一个完整的项目结构：

1. 按 `[group/Name]` 为 key 合并
2. 同一个 key 出现在多个文件中 → 报错（结构冲突）
3. 同一个 section 内字段名重复 → 报错
4. 不同 section 的字段名允许重复（不同表可以有同名字段）

## 与 .tbl 的关系

| 维度 | .tbl | .tblschema |
|------|------|------------|
| 内容 | 结构 + 数据 | 仅结构 |
| 粒度 | 一个文件 = 一张表 | 一个文件 = 多张表 |
| 用途 | 运行时数据源 | 结构定义/备份/测试 |
| 编辑 | UI 工具 / Excel | 文本编辑器 |

从 .tblschema 生成 .tbl 时，只生成表头（#field/#type/#export/#desc），数据行为空或由测试工具动态填充。

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
1. 生成 .tbl 数据文件（动态填充行数据）
2. 导出 JSON + Java 代码
3. 生成 TestMain.java（根据 schema 知道加载哪些类、验证哪些字段）
4. 编译运行，对比预期输出
