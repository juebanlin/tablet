# Validation & Naming Reference

All hard rules the parser enforces. When generating schema or preset data, comply by construction — every check here runs at load and save time.

## Naming rules

| identifier | regex | extra rules | error if violated |
|------------|-------|-------------|-------------------|
| **Field name** (Table column, Constant `name`) | `^[a-z][a-z0-9_]*$` | length 1–32; not a Java/Go/Lua keyword (full list below) | `"不是合法字段名"` / `"是 X 关键字"` |
| **Section name** (table/constant/enum filename) | `^[A-Z][a-zA-Z0-9_]*$` | Java class rules; project-unique (case-insensitive) | `"配置项名必须符合Java类名规则"` / `"配置项名重复"` |
| **Group name** (folder) | `^[a-zA-Z0-9_一-鿿]+$` | project-unique (case-insensitive) | `"组名只能包含中英文数字下划线"` / `"组名重复"` |
| **Enum entry name** | `^[A-Z][A-Z0-9_]*$` | UPPER_SNAKE_CASE; per-enum unique; not a keyword | `"不是合法枚举条目名"` / `"枚举条目名重复"` |
| **metadata id** (`# @meta id:`) | `^[a-z0-9_-]{1,32}$` | filename convention `<id>.tblschema` | `"id 不合法"` |

### Why group allows Chinese

Group names live only in the file tree and UI navigation. They never reach generated code, JSON keys, or import paths — those use section names. So projects can use Chinese group names like `英雄` / `战斗系统` without breaking compilation.

## Three-language keyword union

Field, Constant.name, and Enum entry names cannot collide with any of these. The validator checks all three sets against every name.

### Java keywords (53)

```
abstract assert boolean break byte case catch char class const continue
default do double else enum extends final finally float for goto if
implements import instanceof int interface long native new package private
protected public return short static strictfp super switch synchronized
this throw throws transient try void volatile while true false null
```

### Go keywords (25)

```
break case chan const continue default defer else fallthrough for func
go goto if import interface map package range return select struct
switch type var
```

### Lua keywords (22)

```
and break do else elseif end false for function goto if in local nil
not or repeat return then true until while
```

When user proposes a field like `class` or `type` — flag it. Suggest `kind` / `category` / `class_id` instead.

## Identifier transformation across languages

Generated code transforms snake_case automatically:

| source | Java | Go | Lua |
|--------|------|-----|-----|
| `max_level` | `maxLevel` | `MaxLevel` | `max_level` |
| `hp_regen` | `hpRegen` | `HpRegen` | `hp_regen` |

Enum entries use the original name unchanged in all languages:

- Java: `HeroTypeEnum.WARRIOR`
- Go: `HeroTypeEnum_WARRIOR`
- Lua: `HeroType.WARRIOR`

So the UPPER_SNAKE_CASE constraint is shared by all three.

## Empty value semantics

| paradigm | empty allowed? | meaning when empty | default at load |
|----------|---------------|--------------------|-----------------|
| `int` / `long` | yes | absent / unset | 0 |
| `float` / `double` | yes | absent / unset | 0.0 |
| `str` | yes | empty string | `""` |
| `bool` | yes | unset | `false` |
| `List<P>` / `Set<P>` / `Map<K,V>` | yes | empty collection | `[]` / `{}` |
| `List<Tuple>` / `Map<K,Tuple>` / `Map<K,List>` | yes | empty | `[]` / `{}` |
| `Tuple2<P,P>` / `Tuple3` / `Tuple4` | **NO** | — | — |
| `@Xxx` (Ref) | yes | "no reference" | id 0 |

Tuples are forbidden empty because zero-tuple has no defensible default — `(0,0)` for a position vs `(0,0)` for stats both have business meaning, so the user must commit either to a value or to zero.

A `1;;3` (empty element inside a collection) is **always** rejected even when the field type allows empty as a whole.

## Cell-level validation

Per-cell, depending on column type:

| check | applied to | failure message |
|-------|-----------|-----------------|
| `int` / `long` parses | base int field | `"不是合法int"` |
| `float` / `double` parses | base float field | `"不是合法float"` |
| `bool` ∈ {`true`,`false`} | base bool field | `"必须是true或false"` |
| Tuple element count | tuple field | `"元素数量不匹配"` |
| Tuple element type | each position | `"元素类型不匹配"` |
| List/Set element type | each element | `"列表元素类型不匹配"` |
| Map kv-separator present | each entry | `"缺少kv分隔符"` |
| Map K type / V type | each entry | `"key类型不匹配"` / `"value类型不匹配"` |
| No empty element | every collection | `"含有空元素"` |
| No Chinese punctuation | every cell | `"含有中文标点符号"` |

Chinese punctuation set (rejected): `，；：、`. Use ASCII counterparts.

Errors are emitted with an example value computed from the field's actual separator config:

```
[验证] hero/HeroBase D2: "4,5" 列表元素类型不匹配, 示例: 1;2;3
```

## Row-level validation

| mode | rule | message |
|------|------|---------|
| Table | non-id columns have data but id is empty | `"有数据但ID为空"` |
| Constant | name set but value empty | `"name已填但value为空"` |
| Enum | name set but id empty | `"name已填但id为空"` |

## Schema-level validation

| mode | rule | message |
|------|------|---------|
| Table | at least one field defined | `"表没有定义任何字段"` |
| Table | first column is `id: int` | `"第一列必须是主键 id"` |
| Table | field type strings parse | `"类型 \"X\" 不合法"` |
| Table | field name unique within table | `"字段名 \"X\" 重复"` |
| Constant | constant name unique | `"常量名 \"X\" 重复"` |
| Constant | each entry's type parses | `"类型 \"X\" 不合法"` |
| Constant | `@Xxx` only allowed when `[ui] constant_ref_allowed = true` | `"Constant 不允许引用类型（已在 [ui] 关闭）"` |
| Enum | at least one entry | `"枚举至少需要一个条目"` |
| Enum | enum id unique within enum | `"枚举 id X 重复"` |
| Enum | enum name unique within enum | `"枚举名 X 重复"` |
| Enum | enum id is positive int | `"枚举 id 必须是正整数"` |

## Project-level validation (cross-section)

These run on save / export / reload via `ProjectEngine::validate`:

| rule | source |
|------|--------|
| Table id unique within table | scan all rows |
| `@TableX` references resolve to existing id in TableX | RefIndex |
| `@EnumX` references resolve to existing id in EnumX | RefIndex |
| `@Foo` schema-level: Foo exists as table or enum (not constant, not missing) | RefIndex |

When a referenced section is renamed/deleted, current refs become red but save is NOT blocked — user is expected to fix manually.

## Constant refs

Constant entries can use `@TableName` / `@EnumName` types: the data file stores `int(id)`, and generated code resolves it to the referenced object. When generating new schemas, **emit refs naturally** when the design calls for them. Some toolchains may expose a runtime switch to forbid this — that's a tooling concern, not a content rule.

## Auto-correction (no error reported)

| input | corrected |
|-------|-----------|
| Constant name with leading/trailing whitespace | trimmed |
| Constant name with internal whitespace | whitespace removed |

This happens silently at load and at edit-commit. Don't depend on it — generate clean names from the start.
