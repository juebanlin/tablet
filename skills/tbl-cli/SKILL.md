---
name: tbl-cli
description: "Operate the tablet CLI (tablet-cli or tablet with args). Use when running CLI commands: project management, schema operations, data/code export, validation, Excel bridge, workspace operations, separator queries, or file-level utility tools. Covers all subcommands with parameters and usage patterns. Note: tablet is the GUI program that doubles as CLI when given arguments; tablet-cli is the pure CLI binary — both accept the same commands."
user-invocable: true
allowed-tools:
  - Bash
  - Read
  - Glob
  - Grep
---

# /tbl-cli — tablet CLI operation skill

Operate `tablet-cli` or `tablet` (with arguments) to manage .tbl/.tblschema projects from the command line.

**Naming clarification:**
- `tablet` = GUI 程序，带参数时自动转为 CLI 模式
- `tablet-cli` = 纯 CLI 程序（行为完全一致）
- 以下文档中 `tablet-cli` 可替换为 `tablet`，命令效果相同

Arguments: `$ARGUMENTS`

---

## Command Overview

```
tablet-cli [global-options] <command>

Global options (project-context commands only, not util):
  -w, --workdir <PATH>       Working directory [default: .]
  --project <ID>             Target project (overrides last_project)
  -s, --set <KEY=VALUE>      Override config (repeatable)
  --fmt <FORMAT>             Output format: "json" for structured output
```

---

## Commands

### project — Project management

```bash
tablet-cli project list
tablet-cli project info [--id <id>]
tablet-cli project new --template <id> --id <id> [--name <name>] [--switch-after]
tablet-cli project rename --id <id> [--new-id <id>] [--new-name <name>]
tablet-cli project delete --id <id> --confirm
tablet-cli project clone --source <id> --id <new> [--name <name>]
```

### schema — Structure operations

```bash
tablet-cli schema show
tablet-cli schema add-group --name <name>
tablet-cli schema add-table --group <g> --name <n>
tablet-cli schema add-constant --group <g> --name <n>
tablet-cli schema add-enum --group <g> --name <n>
tablet-cli schema rename-group --old <name> --new <name>
tablet-cli schema rename-node --group <g> --old <n> --new <n>
tablet-cli schema delete-group --name <name>
tablet-cli schema delete-node --group <g> --name <n>
```

### export — Data/code export

```bash
# Data (JSON/XML, supports group/node filter)
tablet-cli export data --json [--group <g>] [--node <n>] [-o <path>]
tablet-cli export data --xml [--group <g>] [--node <n>] [-o <path>]
tablet-cli export data   # both JSON + XML

# Code (full project, select languages)
tablet-cli export code --java [--package <pkg>] [-o <path>]
tablet-cli export code --go [--package <pkg>] [-o <path>]
tablet-cli export code --lua [-o <path>]
tablet-cli export code --gdscript [-o <path>]
tablet-cli export code --typescript [-o <path>]
tablet-cli export code --cpp [--namespace <ns>] [-o <path>]
tablet-cli export code --csharp [--namespace <ns>] [-o <path>]
tablet-cli export code --all [-o <path>]

# All (data + code, CI one-shot)
tablet-cli export all [-o <path>]
```

### validate — Validation (5-level granularity)

```bash
tablet-cli validate                                            # full project
tablet-cli validate --group <g>                                # single group
tablet-cli validate --group <g> --node <n>                     # single node
tablet-cli validate --group <g> --node <n> --col <c>           # single column
tablet-cli validate --group <g> --node <n> --row <r> --col <c> # single cell
```

Exit code: 0 = pass, 1 = errors found.

### excel — Excel bridge

```bash
tablet-cli excel export --group <g> [--include <a,b>] [-o <path>]
tablet-cli excel import --group <g> --file <path>
```

### workspace — Workspace operations

```bash
tablet-cli workspace save
tablet-cli workspace reload
tablet-cli workspace clear --confirm
```

### sep — Separator query

```bash
tablet-cli sep show [--defaults] [--config <path>] [--schema <path>]
```

### util — File-level tools (no project context)

```bash
# Parse
tablet-cli util parse-tbl <file.tbl>
tablet-cli util parse-schema <file.tblschema>
tablet-cli util merge-schema <f1> <f2> [...]

# Validate
tablet-cli util validate-tbl <file> [--config <path>] [--schema <path>] [--sep KEY=VALUE]
tablet-cli util validate-type <type> <value> [--config <path>] [--schema <path>] [--sep KEY=VALUE]

# Convert
tablet-cli util tbl-to-xlsx <file.tbl> -o <out.xlsx>
tablet-cli util scaffold <file.tblschema> -o <dir>

# Auxiliary
tablet-cli util diff <a.tbl> <b.tbl>
tablet-cli util fmt <path> [-i]
tablet-cli util stat <path>

# Test code generation
tablet-cli util gen-test --lang <java|go> --format <json|xml> --schema <path> -o <dir> [--package <pkg>] [--code-output <path>]
```

### Top-level

```bash
tablet-cli list-templates
tablet-cli migrate-legacy
```

---

## Usage Patterns

### CI Pipeline
```bash
tablet-cli --project slg-prod validate || exit 1
tablet-cli --project slg-prod export all
```

### Batch multiple projects
```bash
for p in slg-test slg-prod; do
  tablet-cli --project $p export all -o ./artifacts/$p/
done
```

### JSON output for scripting
```bash
tablet-cli --fmt json project list
tablet-cli --fmt json validate
tablet-cli --fmt json sep show --defaults
```

### Separator config with util
```bash
tablet-cli util validate-type "List<int>" "1|2|3" --sep List="|"
tablet-cli util validate-type "Map<str,int>" "a=1;b=2" --schema project.tblschema --sep Map.kv="="
```

---

## Key Rules

1. `--project` and `-s` are ignored by `util` subcommands
2. `export data --group/--node` only filters data files (JSON/XML), not code
3. `export code` always exports full project (code has cross-references)
4. `workspace clear` and `project delete` require `--confirm`
5. `validate` exit code: 0=pass, 1=fail
6. Schema write operations (add/rename/delete) auto-save after execution

---

## Pre-check

Before running commands, verify the CLI is available:

```bash
tablet-cli --version   # or: tablet --version
tablet-cli --help
```

If neither binary is found, build from source:
```bash
cargo build -p tablet-cli --release
cargo build -p tablet-slint --release  # GUI binary with CLI support
```
