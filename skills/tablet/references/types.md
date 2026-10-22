# 类型系统参考

类型字符串由 `TblType::parse` 解析，**只接受本文列出的形式**。写别的（`List<List<int>>`、`Tuple5<..>`、
`Set<Tuple2<..>>`、`Integer`、`string`）一律报"类型不合法"。

## 基础类型（7）

| 类型 | 含义 | 值示例 | Java | Go | Lua |
|---|---|---|---|---|---|
| `int` | 32 位整数 | `100` | `int` | `int32` | number |
| `long` | 64 位整数 | `123456789012` | `long` | `int64` | number |
| `float` | 单精度 | `1.5` | `float` | `float32` | number |
| `double` | 双精度 | `3.14` | `double` | `float64` | number |
| `str` | 简单字符串（不含 `\|` / 换行 / 制表符） | `hello` | `String` | `string` | string |
| `bool` | 布尔 | `true` / `false` | `boolean` | `bool` | boolean |
| `txt` | 自由文本（描述、JSON、多行）**只能独立成列** | `多行\n文本` | `String` | `string` | string |

## 复合范式

`P` = 任意基础类型（`txt` 除外）或 `@Ref`。

| 类型 | 值示例 | 用到的分隔符 |
|---|---|---|
| `Tuple2<P,P>` / `Tuple3` / `Tuple4` | `5,10` | `TupleN`（默认 `,`） |
| `List<P>` | `1;2;3` | `List`（默认 `;`） |
| `Set<P>` | `1;2;3` | `Set`（默认 `;`） |
| `Map<K,V>` | `hp:100;mp:50` | `Map.kv`（`:`）+ `Map.entry`（`;`） |
| `List<Tuple2<P,P>>`（Tuple3/4 同理） | `1,2;3,4` | `List_TupleN.tuple` + `.list` |
| `Map<K,Tuple2<P,P>>`（Tuple3/4 同理） | `atk:5,10;def:3,8` | `Map_TupleN.kv/.tuple/.entry` |
| `Map<K,List<P>>` | `hp:1,2,3;mp:4,5` | `Map_List.kv/.item/.entry` |
| `@SomeTable` / `@SomeEnum` | `1001` | — |

Map 的 key 用 `int` / `long` / `float` / `double` / `str`。`bool` key 解析器不拦，但 UI 类型选择器不提供、
生成代码也没验证过——别用。

## 引用 `@Xxx`

- `Xxx` 必须是本项目的 table 或 enum 段名（大小写敏感、PascalCase）。
- 数据里存的是 int id；空值 = "无引用"。
- **可以嵌套**：`List<@Item>`、`Map<int,@Hero>`、`Tuple2<@A,@B>` 都合法。
- 目标不能是 constant（常量没有 id）。
- 引用到不存在的 id 会让 `validate` 失败——所以**先建被引用的表/枚举，再写引用方**。

### 生成代码里的样子

| 引用目标 | Java | Go | Lua |
|---|---|---|---|
| enum | `XxxEnum` | 具名 int 类型 | int |
| table | `int` | `int32` | int |

## 非法组合

| 写法 | 原因 |
|---|---|
| `List<List<P>>` / `Set<List<P>>` / `List<Set<P>>` / `Map<K,Map<..>>` | 两层容器分隔符歧义 |
| `Set<Tuple2<..>>` | Set 元素只能是基础类型 / 引用 |
| `List<txt>` / `Map<str,txt>` | `txt` 不能嵌套 |
| `Map<bool,P>` | 解析器不拦，但生成代码没验证过；改用 int / str key |
| `Tuple1<..>` / `Tuple5<..>` | 只有 2/3/4 |

## 空值

单元格留空是合法的，表示"未设置"：

| 类型 | 空值语义 |
|---|---|
| `int` / `long` / `float` / `double` | `0` / `0.0` |
| `str` / `txt` | `""` |
| `bool` | `false` |
| List / Set / Map | 空集合 |
| `@Ref` | 无引用（id 0） |

但**集合内部不能有空元素**：`1;;3`、`a:1;;b:2` 都会被拒绝。中文标点 `，；：、` 在任何单元格里都非法。

## 字段 / 常量 / 枚举条目：不能用的名字

首字符与字符集规则见 SKILL.md；另外这三套关键字取**并集**，命中即报错。

```text
# Java (53)
abstract assert boolean break byte case catch char class const continue default do
double else enum extends final finally float for goto if implements import
instanceof int interface long native new package private protected public return
short static strictfp super switch synchronized this throw throws transient try
void volatile while true false null

# Go (25)
break case chan const continue default defer else fallthrough for func go goto if
import interface map package range return select struct switch type var

# Lua (22)
and break do else elseif end false for function goto if in local nil not or
repeat return then true until while
```

常见踩坑：`type`、`map`、`range`、`class`、`interface`、`default`、`new`、`end`。
换成 `kind` / `category` / `class_id` / `map_kind` 之类。

## 设计习惯

1. **固定集合宁可开枚举**：职业、品质、资源类型 → 一个 `enum` 段 + `@XxxEnum`，比裸 `str` 安全。
2. **Map 的 key 优先用枚举 id**（`Map<int,int>` + `@AttrEnum`），别用 `Map<str,int>` 硬编码字符串。
3. **跨表关联用 `@Table`**，别用 int 裸存。
4. **定长数值对用 Tuple**（坐标、区间、min/max）；字段超过 4 个就拆子表。
5. `long` 留给时间戳 / 大计数；实体 id 用 `int`。
6. 需要换行 / 竖线的长文本用 `txt`，不要硬塞进 `str`。
