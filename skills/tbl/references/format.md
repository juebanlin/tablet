# tbl File Format Reference

Complete syntax for `.tblschema` (v1), `.tbl` v2 (CSV), and `.tbl` v3 (block). Treat this as canonical — if SKILL.md is ambiguous, this file wins.

**Format version dispatch**: `.tbl` first line `#!tbl v2` or `#!tbl v3` selects the codec. New tables default to **v3**. Constant / Enum remain v2 CSV regardless; v3 only applies to Table mode.

## .tblschema v1

### Top-level structure

```
#!tblschema v1                       ← line 1, exact match required
# @meta key: value                   ← zero+ metadata directives
# @sep <key> = <value>               ← zero+ separator overrides
                                     ← blank lines OK
[group/SectionName] mode             ← first section begins here
<section body>

[group/Other] mode
<section body>
```

Once the first `[group/...]` line is consumed, the parser flips a "seen_section" flag — any further `# @meta` or `# @sep` lines are treated as plain comments and silently dropped. `# @preset` is the only mid-file directive that survives this transition (and it's section-scoped).

### `# @meta` directives

Format: `# @meta <key>: <value>` (key:value, colon-delimited, both sides trimmed). Repeated keys: last wins. Unknown keys: silently ignored (forward compat).

| key | type | required | use |
|-----|------|---------|-----|
| `id` | `[a-z0-9_-]{1,32}` | yes (else falls back to filename stem) | path identifier; file convention `<id>.tblschema` |
| `name` | any text incl. CJK | no (defaults to id) | UI display label |
| `category` | any text | no | template library filter (`test` / `slg` / `rpg` / ...) |
| `version` | semver | no | template versioning |
| `created_at` | ISO-8601 UTC | no | project creation timestamp; templates leave empty |
| `source_template` | template id | no | which template was instantiated to create this project |
| `source_template_version` | semver | no | source template version pin |
| `has_preset` | `true` / `false` | no | derive field — recomputed from actual `# @preset` blocks at serialize time, regardless of what's written |

### `# @sep` directives

Format: `# @sep <key> = <value>` (key=value, equals-delimited). Both sides trimmed. **Values are NOT inner-trimmed** — to support whitespace separators if anyone needs them.

Only emit `# @sep` lines when overriding the default. Schema serialization compares each field against `SeparatorsSection::default()` and writes only differences. A schema using all-default separators contains zero `# @sep` lines.

The 25 leaf keys and their defaults are listed in `separators.md`.

### Section header

```
[<group>/<Name>] <mode>
```

- `<group>`: group/folder name. Allowed: `[a-zA-Z0-9_一-鿿]+`. Used for filesystem dir + UI tree only — never reaches generated code.
- `<Name>`: section name = `.tbl` filename stem. Allowed: `[A-Z][a-zA-Z0-9_]*` (PascalCase, Java class rules). Project-unique case-insensitively.
- `<mode>`: one of `table` / `constant` / `enum`.

### Table section body

```
[hero/HeroBase] table
id     | int             | cs | 英雄ID
name   | str             | cs | 名称
hp     | int             | s  | 血量
type   | @HeroType       | cs | 英雄类型
skills | List<int>       | cs | 技能 id 列表
# @preset
1001 | 战士 | 100 | 1 | 1;2;3
1002 | 法师 |  80 | 2 | 4;5
```

- 4 columns per field row: `<name> | <type> | <export> | <desc>`. Whitespace around `|` is trimmed.
- First field MUST be `id | int | cs | <desc>`. The id field cannot be removed, renamed, or moved. The desc text is editable.
- Fields after id are user-defined.

### Constant section body

```
[global/GlobalConst] constant
# @preset
max_level   | int             | 100  | cs | 最大等级
start_pos   | Tuple2<int,int> | 5,10 | cs | 出生坐标
gm_password | str             | xxx  | s  | GM 密码
```

- Schema main body is **empty** — fields/entries live exclusively in `# @preset`.
- Each preset row: `<name> | <type> | <value> | <export> | <desc>`.
- A constant section without `# @preset` is a valid empty constant table.

### Enum section body

```
[hero/HeroType] enum
# @preset
1 | WARRIOR | 战士
2 | MAGE    | 法师
3 | ARCHER  | 弓手
```

- Schema main body is **empty** — entries live exclusively in `# @preset`.
- Each preset row: `<id> | <name> | <desc>`. id must be positive int (never 0).

### Why Constant/Enum forbid main-body data

Historical: old format put entries directly under the section header. The current parser rejects that with `"<mode> 段不允许直接的数据行（如需预设值请放进 # @preset 块）"`. This unifies "schema with preset" handling — generation, instantiation, and merge all flow through one path.

### `# @preset` directive

Format: standalone `# @preset` line (no key/value). Anything matching `#@preset xxx` with trailing content is treated as a regular comment and ignored — the preset block opens only on the bare directive.

Position: must appear AFTER a section header `[group/Name]`, never before. The block extends from the directive line to the next `[group/...]` or EOF, whichever comes first.

Inside the block, every non-empty / non-`#` line is a data row. The row is parsed by `|`-split, with each cell trimmed. Cell count and column meanings are determined by the parent section's mode (see Table/Constant/Enum bodies above).

### Comments

`#` at line start = comment. Three exceptions: `# @meta`, `# @sep`, `# @preset` are directives.

`#` mid-line is NOT a comment — line splits on `|`, no inline comment support.

## .tbl v2

```
#!tbl v2                  ← line 1, exact
#mode <mode>              ← line 2 (table/constant/enum)
#desc <descriptions>      ← Table only, |-separated
#export <export-codes>    ← Table only
#type <types>             ← Table only
#field <names>            ← Table only
---
<data rows>
```

Row separator inside `#desc / #type / #field / #export` is `|` (matching the column layout). Data rows below `---` use the same `|` separator.

### Cell escaping

| Literal | Escaped |
|---------|---------|
| `|` (in str cell) | `\|` |
| newline (in str cell) | `\n` |
| `\` itself | `\\` |

### Empty cells

- A cell may be empty (`a||c`) — meaning depends on type (see `validation.md`).
- Tuple cells must NOT be empty (whole tuple zero is too ambiguous).
- Sequential separator within a cell (`1;;3`) is rejected — empty element forbidden.

### Row colour markers (Excel round-trip)

A data row may end with ` #@c:RRGGBB` (uppercase hex, 6 chars, no `#` prefix). This is set by Excel via background colour and round-tripped by tablet. Generation/AI should NOT add colour markers — they're a UI affordance, not part of design.

### Preset → .tbl translation

When a project is instantiated from a template:

- Table preset rows → data rows below `---`, in field order (v3: `[id]` blocks, see below).
- Constant preset rows → constant data rows (the section's data, NOT in a `# @preset` block — `.tbl` doesn't have that directive).
- Enum preset rows → enum data rows.

## .tbl v3 (Table default)

New Table files default to v3 (`#!tbl v3`). The parser reads the first line to select the codec — legacy v2 files load unchanged. **v3 applies to Table mode only; Constant / Enum stay v2 CSV.**

```
#!tbl v3
#mode table
---
@field id
  export:cs
  type:int
@field name
  desc:英雄名
  export:cs
  type:str
@field hp
  desc:血量
  export:s
  type:int

[1001]
  name:战士
  hp:100
[1002]
  name:法师
  hp:80
```

### Layout rules

| Element | Syntax | Notes |
|---------|--------|-------|
| Version header | `#!tbl v3` | Line 1, exact. `#mode table` retained. |
| Separator | `---` | Legacy `#desc/#export/#type/#field` lines after `---` are **ignored** (may be omitted) |
| Field def | `@field <name>` opens a block | 2-space-indented props: `desc:` (optional) / `export:` (required) / `type:` (required) |
| Record | `[<id>]` opens a block | 2-space-indented `fieldname:value`; empty cells are omitted |
| id field | auto-prepended if missing | `@field id` defaulted to `int` / `cs` / desc "ID" |

### Parsing rules

- Empty lines skipped; `#@color:...` rows skipped.
- `@field <name>` starts a field block; `desc:` / `export:` / `type:` sub-keys fill it (defaults: desc="", export=cs, type="str").
- `[<id>]` starts a record; `id` becomes column 0. Subsequent indented `key:value` lines are placed **by field name** (split on first `:`, so values may contain `:`/`;`).
- If no `@field id`, the parser auto-inserts `id` (int / cs / "ID").
- Records resized to field count; empty trailing cells trimmed.

### Serialization rules

- Every field block writes `export:` and `type:` (even if default); `desc:` written **only when non-empty**.
- Every record writes `[<id>]` (may be `[]` if id empty); **only non-empty cells** are written as `field:value` lines.
- Values are escaped per field `tbl_type`.

### v2 vs v3

| Aspect | v2 | v3 |
|--------|----|----|
| Header | `#desc/#export/#type/#field` `|`-separated | none; `@field` blocks after `---` |
| Structure | columns by position | named `field:value` keys |
| Records | one row per line, `|`-aligned | `[id]` block + indented `field:value`, order-independent, columns may be missing |
| id | first cell | `[id]` brackets, no repeated `id:` |
| Empty cells | empty string (`a||c`) | line omitted |
| Diff / readability | row-stable, easy merge | self-describing keys; no holes from missing cols
