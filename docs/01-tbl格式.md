# .tbl 文件格式规范

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
#index id
#desc 英雄ID|名称|血量|技能组
#type int|str|int|Array<int>
#export 前后端|前后端|服务器|前后端
#field id|name|hp|skills
---
1001|战士|100|1;2;3
1002|法师|80|4;5
1003|弓手|90|6;7;8
```

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
| `#field` | 字段名（仅 table） |
| `#index` | 主键字段名（仅 table） |
| `---` | 头部与数据分隔符 |

## 数据行规则

- 字段用 `|` 分隔
- 字符串中的 `|` 转义为 `\|`，换行转义为 `\\n`
- 空值用空字符串表示
- 复合类型使用各自定义的分隔符（见类型系统）
