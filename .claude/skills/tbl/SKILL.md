---
name: tbl
description: Generate, extend, and validate tablet .tblschema / .tbl file content. Use when the user asks to create a new schema template, add data / preset rows to an existing schema, design table fields and types, hand-validate .tbl content, or convert a planning doc into a tbl schema. Covers the v1 schema spec (# @meta / # @sep / # @preset directives), 14 type paradigms with separator config, three modes (table / constant / enum), and naming / validation rules. This skill is about file content only — does not cover the tablet CLI / GUI which changes frequently.
user-invocable: true
allowed-tools:
  - Read
  - Write
  - Edit
  - Glob
  - Grep
---

# /tbl — tablet file format skill

Authoritative spec for `.tblschema` (v1) and `.tbl` (v2) file content. Use when generating templates, extending preset data, hand-validating, or reverse-engineering schema from design docs.

**Scope is file content only.** This skill does not cover the `tablet` GUI, the `tablet-cli` binary, the workspace `tablet.toml`, project layout on disk, or the import/export pipeline — those move quickly and are not stable enough to bake into a skill. If a user asks something CLI / UI / pipeline related, decline gracefully and point them at the in-repo docs (`docs/` directory).

**Do not paraphrase the references from memory** — the files in `references/` are the canonical source for this skill. Read them when you need a rule.

Arguments: `$ARGUMENTS`

---

## When to invoke

Trigger this skill (with or without explicit `/tbl`) when the user asks to:

- Create a new `.tblschema` file (game template, test fixture, demo)
- Add tables / constants / enums to an existing `.tblschema`
- Add `# @preset` data rows to an existing schema
- Translate a design doc (markdown / Excel) into schema content
- Pick types for fields (`int` vs `long`, `str` vs `@EnumName`, `Map<int,int>` vs `List<Tuple2>`)
- Hand-validate or merge `.tbl` / `.tblschema` content
- Configure separators (the 25 leaf keys via `# @sep` lines)

Do NOT invoke this skill for:

- "How do I run the export?" / "How do I open the GUI?" — point at in-repo docs.
- Changing the actual tablet source code in `tablet/crates/` — work directly without the skill.
- Workspace-level config (`tablet.toml [separators]`, `[ui]` toggles, etc.) — out of scope.

---

## Reference layout

```
.claude/skills/tbl/
├── SKILL.md                       (this file — workflow + dispatch)
├── references/
│   ├── format.md                  (.tbl + .tblschema layout, all directives)
│   ├── types.md                   (14 paradigms + Ref, base types, language mapping)
│   ├── separators.md              (25 leaf keys, default values, # @sep syntax)
│   ├── validation.md              (naming rules, keywords, schema/cell validation)
│   └── examples/
│       ├── full.tblschema         (every paradigm covered, validated by parser)
│       └── enum_table_constant.tblschema  (small realistic RPG demo)
```

**Read order when generating new content**: `format.md` → `types.md` → `validation.md` → `separators.md` (only if non-default needed) → relevant example.

The most recent canonical demo in the repo is `tablet/crates/core/schemas/sanguo.tblschema` — read it whenever the user asks for "a realistic full schema demo" so you copy its actual style.

---

## Subcommands

### Default (no args) — show capabilities

List what the skill can do (schema gen / preset extend / hand-validate / type design) and ask the user which they need.

### `new-schema` — generate a `.tblschema` from a design brief

1. Confirm the **target file path** with the user. The skill doesn't care where it lands — common locations are `tablet/crates/core/schemas/<id>.tblschema` (in-repo template) or any path the user names.
2. Confirm **id / name / category / version** (id must match `[a-z0-9_-]{1,32}`). If user gave a Chinese name, derive id from semantic kebab-case ascii.
3. Read `references/format.md` and `references/types.md`.
4. Walk the user's design doc:
   - Identify entities → become `[group/Name] table` sections
   - Identify global config knobs → become `[group/<Name>Const] constant` (one section per scope)
   - Identify finite categorical sets → become `[group/<Name>Enum] enum`
5. **Always check `references/validation.md` before writing field names** — name must be snake_case, must not be a keyword in any of Java/Go/Lua, must not collide.
6. Write the schema file. Order:
   - `#!tblschema v1` (line 1)
   - `# @meta id / name / category / version / created_at` (omit fields user didn't provide)
   - `# @sep` lines (only if non-default; skip otherwise — see `references/separators.md`)
   - blank line
   - sections in dependency order: enums first (so refs resolve forward), then tables, then constants
   - Each table: `[group/Name] table`, then field rows `name | type | export | desc`
   - If user wants demo data, append `# @preset` block within each section
7. After write: explain the design choices in 3-5 bullets — why each enum / why ref vs str / which Maps used int keys.

### `add-preset` — append `# @preset` rows to an existing schema

1. Read the target `.tblschema`.
2. Identify the target section by `[group/Name]`.
3. For Table: rows go in field-order — every column must appear.
4. For Constant: each row is `name | type | value | export | desc`.
5. For Enum: each row is `id | name | desc`.
6. Validate against `references/validation.md`:
   - Ref values are `int` ids — reject if user passes a name string for `@EnumName`
   - Tuple values must have exactly N comma-separated parts
   - List/Set/Map values must respect the file's separators (read `# @sep` lines if present, else default)
7. Append to file in-place using Edit.

### `validate` — hand-check a `.tbl` or `.tblschema` content

1. Read the target file.
2. Check section ordering (`#!` first; `# @meta` / `# @sep` only before first `[group/Name]`).
3. Check every type string parses as a `TblType` (`int` / `List<int>` / `Tuple2<int,str>` / `@HeroBase` etc).
4. Check field / constant / enum names against `references/validation.md`.
5. Check `@Xxx` references resolve to a section in the same file (or note "external; assumed valid").
6. Report findings. Do not edit unless asked.

This is a **content-level check** — for project-level cross-file validation (id uniqueness, ref resolution across many .tbl files), point the user at `tablet-cli validate`.

### `design` — propose schema for a feature without writing files

1. Read user's brief.
2. Output a markdown table proposing each section + fields, types chosen, and one-sentence rationale per non-trivial choice (especially Map key type, Ref vs str, List vs Tuple).
3. Stop. Don't write until user OKs.

---

## Hard rules (apply to every subcommand)

These rules are enforced by `tablet-core::tblschema::parse_tblschema` at file load time. Violating them will fail to parse.

### Identifier rules (full table in `references/validation.md`)

- **Section name** (file name): PascalCase, `[A-Z][a-zA-Z0-9_]*`, project-unique case-insensitive.
- **Group name** (directory): `[a-zA-Z0-9_一-鿿]+`, project-unique case-insensitive. Chinese allowed because group never appears in generated code or import paths.
- **Field name / Constant name**: snake_case, `[a-z][a-z0-9_]*`, must NOT be a Java/Go/Lua keyword (combined set in `references/validation.md`).
- **Enum entry name**: UPPER_SNAKE_CASE, `[A-Z][A-Z0-9_]*`, same keyword exclusion.
- **Enum id**: positive int, **never 0** (0 reserved for "unset").
- **Table id**: positive int, project-unique within the table.
- **metadata id** (`# @meta id:`): `[a-z0-9_-]{1,32}`.

### Type rules (full table in `references/types.md`)

- 14 legal paradigms + Ref. Anything else is rejected (`List<List<int>>`, `Map<str,Map<...>>`, `List<@HeroType>` all illegal).
- Ref (`@Xxx`) cannot nest inside collections — must appear at the outermost level.
- Ref target must be a `table` or `enum` section. Ref to `constant` is forbidden (constants have no id).
- Map key allowed types: `int / long / float / double / str` (no bool).
- Set element must be a base type (no Set of tuples).
- **Prefer enum-int keys over str keys for Maps**: `Map<int,int>` keyed by `@AttrEnum`-id beats `Map<str,int>` with hardcoded "hp"/"mp" strings. The user explicitly favors this pattern (see `tablet/crates/core/schemas/sanguo.tblschema` for the reference style).

### Constant mode rules

- 5 column layout: `name | type | value | export | desc`. Schema main body MUST be empty — write entries only inside `# @preset` blocks.
- `@Xxx` references are allowed by default. (Some projects toggle this off via workspace config — that's project-level config, out of skill scope. Just generate refs naturally; if a project rejects them at load, the user knows the cause.)

### Enum mode rules

- 3 column layout: `id | name | desc`.
- Schema main body MUST be empty — write entries only inside `# @preset`.
- id must be positive int, never 0.

### Table mode rules

- First column is forced `id | int | cs | ID描述` and cannot be moved / renamed / removed.
- All other columns are user-defined fields.

### Directive ordering

```
#!tblschema v1                ← line 1, exact
# @meta id: ...               ← directives, before first [section]
# @meta name: ...
# @sep List = ;               ← optional, only non-default values

[group/Name] mode             ← sections start
field_a | type | export | desc
# @preset                     ← optional preset block per section
data_row_1
data_row_2

[next/Section] mode
...
```

After the first `[group/Name]` line, any `# @meta` or `# @sep` line is silently ignored as a comment. `# @preset` is the only directive valid inside a section.

### Default separators (only override if user explicitly asks)

- Tuple{2,3,4}: `,`
- List / Set: `;`
- Map.kv: `:`, Map.entry: `;`
- Mixed paradigms: outer separator `;`, inner `,`

Full leaf list and override syntax in `references/separators.md`.

### Export tags

`cs` (default, both) / `c` (client only) / `s` (server only) / `-` (skip). When in doubt, write `cs`.

---

## Design heuristics

When the user gives a fuzzy brief, lean toward these defaults:

| Question | Default choice | Why |
|----------|---------------|-----|
| Categorical attribute (5–30 options)? | New `enum` + `@XxxEnum` ref | Type-safe, code-friendly, generates language enum |
| Map keyed by attribute name (hp/mp/atk)? | `Map<int,int>` keyed by `@AttrEnum` id | int keys avoid str typos; matches user preference |
| List of homogeneous structs? | `List<Tuple2/3/4<P,P,...>>` | Tuple paradigms exist for exactly this |
| Long descriptive text? | `str` | No size limit |
| Cross-table pointer? | `@TargetTable` | Ref paradigm; data file stores int(id) |
| Numeric range that fits int32? | `int` | Smaller, faster |
| Timestamp / large counter? | `long` | int64 |
| Money / multipliers? | `double` if precision matters, else `int`+scale | Float rounding bites in game configs |
| Boolean? | `bool` | Or 1/0 in `int` if you anticipate adding states later |

If the user's design has more than 4 components in a tuple, push back: `Tuple4` is the cap. Suggest splitting into a named sub-table referenced by id.

If the user wants nested collections (`List<List<int>>` etc), refuse and propose a flatter design — usually a Map keyed by an outer enum, or a List of Tuples.

---

## After completing a write

End with a 1-2 sentence summary: which sections were added, total row count, any non-default separator used. Mention if `# @preset` data was generated and remind the user that loading the schema with-preset is what brings the data along.
