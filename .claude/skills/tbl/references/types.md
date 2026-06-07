# tbl Type System Reference

The `TblType` parser accepts exactly these forms — anything else is rejected with a schema error. When designing schema, only generate these strings.

## Base types (6)

| token | bytes | range | Java | Go | Lua | example |
|-------|-------|-------|------|-----|-----|---------|
| `int` | 4 | i32 | `int` | `int32` | number | `100` |
| `long` | 8 | i64 | `long` | `int64` | number | `123456789` |
| `float` | 4 | — | `float` | `float32` | number | `1.5` |
| `double` | 8 | — | `double` | `float64` | number | `3.14` |
| `str` | — | UTF-8 | `String` | `string` | string | `hello` |
| `bool` | 1 | true/false | `boolean` | `bool` | boolean | `true` / `false` |

## 14 paradigms

Layer = nesting depth. Every "P" position can independently choose any base type — `Tuple2<int,str>` is fine, `Map<long,float>` is fine.

| layer | paradigm | example value | separators used |
|-------|----------|--------------|----------------|
| 0 | `<P>` (Base) | `100` / `hello` | — |
| 1 | `Tuple2<P,P>` | `5,10` | tuple2 |
| 1 | `Tuple3<P,P,P>` | `1,2,3` | tuple3 |
| 1 | `Tuple4<P,P,P,P>` | `1,2,3,4` | tuple4 |
| 1 | `List<P>` | `1;2;3` | list |
| 1 | `Set<P>` | `1;2;3` | set |
| 1 | `Map<P,P>` | `hp:100;mp:50` | map.kv + map.entry |
| 2 | `List<Tuple2<P,P>>` | `1,2;3,4` | listTuple2.tuple + listTuple2.list |
| 2 | `List<Tuple3<P,P,P>>` | `1,2,3;4,5,6` | listTuple3.* |
| 2 | `List<Tuple4<P,P,P,P>>` | `1,2,3,4;5,6,7,8` | listTuple4.* |
| 2 | `Map<P,Tuple2<P,P>>` | `atk:5,10;def:3,8` | mapTuple2.* |
| 2 | `Map<P,Tuple3<P,P,P>>` | `atk:1,2,3;def:4,5,6` | mapTuple3.* |
| 2 | `Map<P,Tuple4<P,P,P,P>>` | `k:1,2,3,4;j:5,6,7,8` | mapTuple4.* |
| 2 | `Map<P,List<P>>` | `hp:1,2,3;mp:4,5,6` | mapList.kv + mapList.item + mapList.entry |
| — | `@Xxx` (Ref) | `1001` (id) | — |

## Ref (`@Xxx`) — special paradigm

Format: `@<Name>` where `<Name>` matches the section name rules (`[A-Z][a-zA-Z0-9_]*`). The data file stores `int` ids; the schema records the target name.

Allowed targets:

- `@TableName` → references a `[group/Name] table` section by id
- `@EnumName` → references a `[group/Name] enum` section by id
- Forbidden: `@ConstantName` (constants have no id concept; rejected with `"不能引用 constant"`).

The Ref paradigm CANNOT nest: `List<@HeroType>`, `Map<int,@Skill>`, `Tuple2<@A,int>` are all illegal. To express "list of ref", use `List<int>` and document the intent in `desc`.

Validation:

- Empty value is allowed (treated as "no reference"). Default int value 0.
- Non-empty value must be a positive int that exists as an id in the target section.
- Schema with `@Foo` where `Foo` doesn't exist as a section → `"引用的配置项 Foo 不存在"`.

### Generated code semantics

| Ref target mode | Java field type | Go field type | Lua field type |
|----|------|------|------|
| `enum` | `<Name>Enum` (Java enum, `fromId(int)` loader) | `<Name>Enum` (typed int) | `int` (game code looks up enum table) |
| `table` | `int` | `int32` | `int` |

## Forbidden combinations

| combination | reason |
|-------------|--------|
| `List<List<P>>` | outer/inner separators conflict |
| `Set<List<P>>` | same |
| `List<Set<P>>` | same |
| `Map<P,Map<P,P>>` | nested Map ambiguous |
| `Set<Tuple<...>>` | Set element must be Base |
| `Map<bool,P>` | bool keys forbidden (not in `BaseType::map_key_types`) |
| `List<@Xxx>` / `Tuple<@Xxx,...>` / `Map<P,@Xxx>` | Ref doesn't nest |

When a user asks for a forbidden combination, push back and propose:

- "Map of lists keyed by enum" → `Map<int,List<int>>` with `@SomeEnum` understood for key
- "List of nested struct" → split into a sub-table referenced by `@SubName`

## Type string parsing

The `TblType::parse` rules in priority order:

1. `@Xxx` → Ref(name) — name must start uppercase, only `[a-zA-Z0-9_]`
2. Single base type → Base(P)
3. `Tuple{2,3,4}<...>` → fixed-arity tuple, all params must be base types
4. `Set<P>` → Set, P must be base type
5. `List<Tuple{2,3,4}<...>>` → ListTupleN
6. `List<P>` → List, P must be base type (post-step-5, so List of Tuple is intercepted earlier)
7. `Map<K,Tuple{2,3,4}<...>>` → MapTupleN
8. `Map<K,List<P>>` → MapList
9. `Map<K,V>` → Map (both base types)

The parser uses a depth-aware comma split, so commas inside nested `<...>` don't break apart. Use this for sanity checking your generated type strings.

## Cross-language value mapping

| paradigm | .tbl raw | Java | Go | Lua |
|----------|----------|------|-----|-----|
| `int` | `100` | `int 100` | `int32(100)` | `100` |
| `str` | `hello` | `"hello"` | `"hello"` | `"hello"` |
| `Tuple2<int,int>` | `5,10` | `int[]{5,10}` | `[2]int32{5,10}` | `{5,10}` |
| `Tuple2<int,str>` | `1001,sword` | `Tuple2(1001,"sword")` | struct{int32; string} | `{1001,"sword"}` |
| `List<int>` | `1;2;3` | `List.of(1,2,3)` | `[]int32{1,2,3}` | `{1,2,3}` |
| `Set<int>` | `1;2;3` | `Set.of(1,2,3)` | `map[int32]struct{}` | `{[1]=true,[2]=true,[3]=true}` |
| `Map<str,int>` | `hp:100;mp:50` | `Map.of("hp",100,"mp",50)` | `map[string]int32` | `{hp=100,mp=50}` |
| `List<Tuple2<int,int>>` | `1,2;3,4` | `List<int[]>` | `[][2]int32` | `{{1,2},{3,4}}` |
| `Map<str,Tuple2<int,int>>` | `atk:5,10;def:3,8` | `Map<String,int[]>` | `map[string][2]int32` | `{atk={5,10},def={3,8}}` |
| `Map<str,List<int>>` | `hp:1,2,3;mp:4,5` | `Map<String,List<Integer>>` | `map[string][]int32` | `{hp={1,2,3},mp={4,5}}` |

Generated code uses primitive types (no `Integer`, no `*int32` pointer) — business code does not deal with null. Empty cells deserialize to default values (see `validation.md` empty-value table).

## Field design preferences (project-specific)

These reflect explicit user feedback in this project — apply by default unless told otherwise:

1. **Map keys: prefer enum-int over str.** When the keys form a fixed set (attribute name, resource type, building tier), introduce a new enum and use `Map<int,V>`. The Three Kingdoms reference template (`tablet/crates/core/schemas/sanguo.tblschema`) does this throughout: `Map<int,int>` for stat bonuses keyed by `@HeroAttr`, costs keyed by `@ResourceKind`, etc.
2. **Lists of categorical ids: prefer `List<int>` keyed by enum.** `tags: List<int>` + a `@TagEnum` is preferred over `tags: List<str>`.
3. **No `Long` for game ids if `int` fits.** Hero/skill/item ids comfortably fit in i32; reserve `long` for timestamps and counters.
4. **Constant `desc` is informational** — generated code doesn't see desc, so it's free-form Chinese OK.
5. **Tuples for fixed-arity numeric structs only.** Position (x,y), bounds (min,max), modifier (atk,def). Don't use Tuple for "mixed bag" of unrelated fields — make a sub-table instead.
