# Separators Reference

The 25 leaf keys that fully describe how each paradigm serializes its parts.

## Default values

| key | default | used by paradigm |
|-----|---------|------------------|
| `Tuple2` | `,` | `Tuple2<P,P>` |
| `Tuple3` | `,` | `Tuple3<P,P,P>` |
| `Tuple4` | `,` | `Tuple4<P,P,P,P>` |
| `List` | `;` | `List<P>` |
| `Set` | `;` | `Set<P>` |
| `Map.kv` | `:` | `Map<K,V>` |
| `Map.entry` | `;` | `Map<K,V>` |
| `List_Tuple2.tuple` | `,` | `List<Tuple2<P,P>>` |
| `List_Tuple2.list` | `;` | `List<Tuple2<P,P>>` |
| `List_Tuple3.tuple` | `,` | `List<Tuple3<P,P,P>>` |
| `List_Tuple3.list` | `;` | `List<Tuple3<P,P,P>>` |
| `List_Tuple4.tuple` | `,` | `List<Tuple4<P,P,P,P>>` |
| `List_Tuple4.list` | `;` | `List<Tuple4<P,P,P,P>>` |
| `Map_Tuple2.kv` | `:` | `Map<K,Tuple2<P,P>>` |
| `Map_Tuple2.tuple` | `,` | `Map<K,Tuple2<P,P>>` |
| `Map_Tuple2.entry` | `;` | `Map<K,Tuple2<P,P>>` |
| `Map_Tuple3.kv` | `:` | `Map<K,Tuple3<P,P,P>>` |
| `Map_Tuple3.tuple` | `,` | `Map<K,Tuple3<P,P,P>>` |
| `Map_Tuple3.entry` | `;` | `Map<K,Tuple3<P,P,P>>` |
| `Map_Tuple4.kv` | `:` | `Map<K,Tuple4<P,P,P,P>>` |
| `Map_Tuple4.tuple` | `,` | `Map<K,Tuple4<P,P,P,P>>` |
| `Map_Tuple4.entry` | `;` | `Map<K,Tuple4<P,P,P,P>>` |
| `Map_List.kv` | `:` | `Map<K,List<P>>` |
| `Map_List.item` | `,` | `Map<K,List<P>>` |
| `Map_List.entry` | `;` | `Map<K,List<P>>` |

## `# @sep` syntax

Inside a `.tblschema` file, between `#!tblschema v1` and the first section, override defaults with:

```
# @sep <key> = <value>
```

- `<key>`: one of the 25 leaf strings above (case-sensitive, dot/underscore exactly as listed).
- `<value>`: any string. Equals-delimited; outer whitespace trimmed; inner whitespace **kept**.

Examples:

```
# @sep List = |
# @sep Map.entry = ,
# @sep Map_Tuple2.kv = =
# @sep Map_List.item = +
```

## Hierarchy of effective separators

For a given project at runtime:

1. **Project `.tblschema`** parses `# @sep` lines into `schema.separators`.
2. `load_project` copies that into `project.config.separators`.
3. All validation, export (JSON `_sep` / XML `sep_*` attrs), and example value generation read from `project.config.separators`.

The workspace `tablet.toml [separators]` block is **only** used at startup to populate `engine.default_separators`, which is in turn copied to `schema.separators` when the user creates a brand-new empty project. Templates and file-based new-project flows inherit from the source schema's separators, not from toml.

## When to override defaults

Almost never. The defaults are picked so values like `1;2;3` and `hp:100;mp:50` look natural. Override only when:

- Migrating data from an existing pipeline that locked in different separators (e.g. legacy used `|` for List).
- A field's data legitimately contains `;` or `,` and you want a different boundary char.

When generating new schemas from scratch, do **not** emit `# @sep` lines — defaults are equivalent and produce a cleaner file.

## Separator choice constraints

- Each separator must be a single ASCII character (the parser doesn't enforce length, but multi-char separators break example-value rendering and Excel TSV bridge).
- Don't use `|` (the column separator).
- Don't use whitespace (the cell trim collapses it).
- Inner separators must differ from outer separators in the same paradigm (e.g. `List_Tuple2.tuple ≠ List_Tuple2.list`).
- Don't pick CJK characters (the validator explicitly bans Chinese punctuation in cell content).

## Validation against separators

Cell values are split by paradigm's separator chain:

- `Tuple2<int,int>` value `5,10` splits on `Tuple2` (= `,`) → `[5, 10]`.
- `Map<str,int>` value `hp:100;mp:50` splits on `Map.entry` (= `;`) into entries, each entry splits on `Map.kv` (= `:`).
- `Map<str,Tuple2<int,int>>` value `atk:5,10;def:3,8` splits on `Map_Tuple2.entry` (`;`), each entry splits on `Map_Tuple2.kv` (`:`), then value side splits on `Map_Tuple2.tuple` (`,`).

A consecutive separator (`1;;3`) is rejected — empty element is forbidden in all paradigms. A single empty cell (the whole field) is the empty value (semantics in `validation.md`), but elements within must each be non-empty.
