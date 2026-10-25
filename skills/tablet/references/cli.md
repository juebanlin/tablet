# tablet-cli 命令参考

同一个 exe 两种模式：`tablet` 无参数 = GUI，带参数 = CLI；`tablet-cli` 是纯 CLI 二进制，命令完全一致。
下文一律写 `tablet-cli`。

先确认命令可用：`tablet-cli --version`（或 `tablet --version`）。
没有的话从源码编：`cargo build -p tablet-cli`（产物 `target/debug/tablet-cli`）。

```text
tablet-cli [全局选项] <命令>
```

| 全局选项 | 说明 |
|---|---|
| `-w, --workdir <dir>` | 工作区根（`projects/` 的父目录），默认 `.` |
| `--project <id>` | 指定项目；省略则跟随 `tablet.toml` 的 `[project] last_project` |
| `-s, --set KEY=VALUE` | 临时覆盖配置（可重复），不影响磁盘 |
| `--fmt json` | 查询类命令输出 JSON |

`util` 子命令不需要也不接受 `-w` / `--project` / `-s`。

**参数顺序**：`-w` / `--project` / `-s` 是顶层参数，必须写在子命令**前面**；
`--fmt` 是全局参数，位置随意。

---

## project

```bash
tablet-cli project list [--fmt json]
tablet-cli project info [--id <id>]
tablet-cli project new --template <tpl> --id <id> [--name N] [--switch-after true|false]
tablet-cli project new --from-file <x.tblschema> --id <id> [--name N]
tablet-cli project rename --id <id> [--new-id <new>] [--new-name <name>]
tablet-cli project clone --source <id> --id <new> [--name N]
tablet-cli project import-tbl --id <id> [--group N] <PATH>...
tablet-cli project import-xlsx --id <id> <FILE.xlsx>
tablet-cli project delete --id <id> --confirm
```

- `new` 会在验证通过后**立即落盘**；不过关就整个撤回并逐条打印原因。
- `--from-file` 时，`# @preset` 会写进 `config/<group>/*.tbl`。
- `--name` 省略时：`--from-file` 取 schema 的 `# @meta name`，否则 = id。
- `delete` / `clone` 目标不存在 → exit 1，不会假装成功。
- `import-tbl`：文件名 = 节点名，组名默认取共同父目录；节点名项目级冲突直接失败。
- `import-xlsx`：文件 = 组，sheet = 节点；工作簿原样复制，保留样式。

## schema

```bash
tablet-cli schema show [--rules] [--fmt json]

tablet-cli schema add-group --name <G> [--storage tbl|xlsx]
tablet-cli schema add-table    --group <G> --name <N> [--tbl-fmt v2|v3]
tablet-cli schema add-constant --group <G> --name <N>
tablet-cli schema add-enum     --group <G> --name <N> --entry 1:NAME:说明 [--entry 2:OTHER]

tablet-cli schema export --out <file.tblschema> [--group G]... [--node G/N]... [--with-preset] \
                        [--meta-id ID] [--meta-name NAME] [--meta-category CAT] [--meta-version V]
tablet-cli schema export --as-template --meta-id ID

tablet-cli schema rename-group --old <A> --new <B>
tablet-cli schema rename-node  --group <G> --old <A> --new <B>
tablet-cli schema delete-group --name <G>
tablet-cli schema delete-node  --group <G> --name <N>
```

`--storage xlsx` 建的是「工作簿组」：一组一个 `config/<group>.xlsx`，节点 = sheet，数据落在 Excel 里
（结构与数据都在工作簿里；manifest 只记 storage）。默认 `tbl`。

工作簿组对 `validate` / `export` / `schema show` 完全透明；`util` 系列是文件工具，不识别这种组。
GUI 开着时 CLI 会被工作区锁拒绝（`util` / `list-templates` 除外）。

`--tbl-fmt` 只决定新表第一次落盘用哪个 `.tbl` 版本（`v3` 块式，默认；`v2` 稀疏行）。之后以文件头为准。

`schema show --rules` 在结构树每项下面列出该节点的**有效规则**（系统内置 + 用户 `# @rule`，
系统项带 `[系统]` 标记、按 `@01.8.11` 只读）。排查「为什么这行报错」先看它。

`schema export` 对应 GUI 的「导出 Schema... / 另存为模板...」：

- 默认导出全部节点、只导结构；`--with-preset` 才带 `# @preset` 数据。
- `--group` / `--node` 可重复，取并集。
- `--as-template` 写到本地模板目录，需要合法 `--meta-id`；与 `--out` 二选一。
- 元数据缺省继承当前项目；`--meta-name` 空时用 id 兜底。

所有写命令都是 **校验 → 执行 → 落盘 → 看结果**：

- 名字不合规 / 目标不存在 / 撞名 → exit 1，盘上零改动。
- 项目有验证错误 → 保存中止，exit 1 并打印全部错误。
- `add-enum` 的空枚举过不了校验，所以 `--entry` 必填。
- 新建的空 Group 会保留；一个组最后一个节点被删掉时，组目录一并清理。

## validate

```bash
tablet-cli validate                                     # 整个项目
tablet-cli validate --group hero                        # 一个组
tablet-cli validate --group hero --node HeroBase        # 一个节点
tablet-cli validate --group hero --node HeroBase --col 2 --row 0   # 一个格子（0 基）
```

退出码：`0` 通过 / `1` 有错（或 `--group` / `--node` 指向不存在的目标）。

**级别 → 动作由项目设置决定**（`project.toml` 的 `[validate]`，默认 `error=block`、
`warning=report`、`info=ignore`）：`block` 计入 errors、`report` 计入 warnings、`ignore` 不输出。
所以「警告是否失败」取决于三项设置，不是写死的；`--strict` 表示**连 warnings 也算失败**（CI 推荐）。

错误行形如 `[验证] hero/HeroBase C1:[abc] -> 不是合法int`：`C` 是列字母、`1` 是数据行号；
表头层错误写成 `表头第 4 行 B 列`。行号 / 列号都从 1 数。

```bash
tablet-cli --fmt json validate
# {"errors":[{"project_id":"demo","group":"hero","node":"HeroBase",
#             "row":0,"col":2,"message":"…","severity":"error",
#             "display":"[验证] hero/HeroBase C1:[abc] -> …"}],
#  "warnings":[…]}
tablet-cli validate --strict          # 「仅提示」级也算失败
tablet-cli validate --fmt json        # errors / warnings 两个数组
```

## export

| 子命令 | 范围 |
|---|---|
| `export data` | JSON / XML 数据，可按组、节点过滤 |
| `export code` | 全项目代码，按语言选 |
| `export all` | 数据 + 全部代码 |

```bash
tablet-cli export data [--json] [--xml] [--group G] [--node N] [-o DIR]
tablet-cli export code --java|--go|--cpp|--lua|--gdscript \
                       [--typescript|--typescript-client|--typescript-server] \
                       [--csharp|--csharp-dotnet|--csharp-unity|--csharp-godot] \
                       [--all] [--package <pkg>] [--namespace <ns>] [-o DIR]
tablet-cli export all [-o DIR]
```

- 默认输出根：`projects/<id>/gen/`；`-o` 改根目录。
- **`-o` 的相对路径相对项目根 `projects/<id>/`，不是当前目录**；拿不准就写绝对路径。
- `export data` 不给 `--json/--xml` 时两个都导。
- 单个格式失败**不打断**其它格式，但**只要有一个失败，退出码就是 1**。
- `--package` 同时覆盖 Java 与 Go；`--namespace` 同时覆盖 C++ 与 C#。
- `--fmt json` 输出逐格式的 `added/modified/deleted/unchanged/warnings` 统计。
- `export code` 的格式粒度与 UI 的 12 个勾选框一一对应：

  | 参数 | 只导 |
  |---|---|
  | `--typescript` | TS 两端（= `--typescript-client --typescript-server`） |
  | `--typescript-client` / `--typescript-server` | 只前端 / 只 Node.js |
  | `--csharp` | C# 三套（= dotnet + unity + godot） |
  | `--csharp-dotnet` / `--csharp-unity` / `--csharp-godot` | 只一个 runtime |

- `-o` 语义：**单格式 = 精确目录；多格式 = 输出根**，按
  `server/java`、`server/go`、`server/cpp`、`server/csharp`、`server/typescript`、
  `client/lua`、`client/gdscript`、`client/typescript`、`client/csharp_unity`、`client/csharp_godot`
  展开，避免多个 `Loader.cs` 互相覆盖。

UI 的 4 个预设 = CLI 组合：全部 `export all`；数据 `export data`；
后端 `export data` + `export code --java --go --cpp --csharp-dotnet --typescript-server`；
前端 `export code --lua --gdscript --typescript-client --csharp-unity --csharp-godot`。

> Unity 插件 / bat 的完整调用方式（路径配置、分支显示、bat、EditorWindow）见 `unity.md`。
> 关键：给 Unity 用 `export code --csharp-unity`（只出一套）；`--csharp` 会顺带生成 dotnet / godot。

## excel

```bash
tablet-cli excel export --group hero [--include HeroBase,HeroType] [-o hero.xlsx]
tablet-cli excel import --group hero --file hero.xlsx
```

导出给策划编辑；回读时按表头与 schema 对齐，只更新数据、不动结构。

## workspace

```bash
tablet-cli workspace info      # 工作区路径 / VCS 分支 / 项目列表（不加载 .tbl、不占锁）
tablet-cli workspace save      # 落盘 + 刷新 project.tblmanifest 索引
tablet-cli workspace reload    # 丢弃内存改动，从盘重读
tablet-cli workspace clear --confirm   # 清空所有 .tbl（危险）
```

`workspace info` 是只读快照，**不占工作区锁**：GUI 开着也能读，给 Unity 插件 / CI 显示当前分支。
Git 直接读 `.git`（含 submodule / worktree）；SVN 用 `svn info --show-item`（1.9+）。
`--fmt json` 字段：`workdir` / `vcs` / `branch` / `revision` / `detached` / `vcs_error` /
`last_project` / `projects`。

`save` 遇到验证错误会**一个字节都不写**并 exit 1。

## sep

```bash
tablet-cli sep show                 # 当前项目生效值
tablet-cli sep show --defaults      # 内置默认值
tablet-cli sep show --schema x.tblschema
tablet-cli sep show --config tablet.toml
```

## list-templates / util

```bash
tablet-cli list-templates

tablet-cli util parse-tbl <f.tbl>
tablet-cli util parse-schema <f.tblschema>
tablet-cli util merge-schema a.tblschema b.tblschema
tablet-cli util validate-tbl <f.tbl> [--schema s | --config c | --sep KEY=VAL ...]
tablet-cli util validate-type "List<int>" "1;2;3" [--sep List="|"]
tablet-cli util tbl-to-xlsx <f.tbl> -o <f.xlsx>
tablet-cli util xlsx-to-tbl <f.xlsx> --schema <s.tblschema> -o <dir>
tablet-cli util scaffold <s.tblschema> -o <dir>
tablet-cli util diff <a.tbl> <b.tbl>
tablet-cli util fmt <path> [-i]
tablet-cli util stat <path>
tablet-cli util gen-test --lang java|go --schema <s.tblschema> -o <dir>
```

`util validate-type` 是写 schema 时最快的自测手段：先验一个值再落文件。

---

## 退出码

| 码 | 含义 |
|---|---|
| `0` | 成功 |
| `1` | 验证不过 / 目标不存在 / 解析失败 / 保存被拦下 / I-O 失败 |
| `2` | clap 用法错误（缺必填参数、未知选项） |

错误信息走 **stderr**；`--fmt json` 时 stdout 只有 JSON。

## 典型流水线

```bash
set -euo pipefail
W=./config-repo

tablet-cli -w "$W" project new --from-file design/game.tblschema --id game
tablet-cli -w "$W" --project game validate
tablet-cli -w "$W" --project game export all -o "$W/artifacts/game"

# 批量项目
for p in $(tablet-cli -w "$W" --fmt json project list | grep -o '"id": "[^"]*"' | cut -d'"' -f4); do
  tablet-cli -w "$W" --project "$p" validate
  tablet-cli -w "$W" --project "$p" export all -o "$W/artifacts/$p"
done
```

## 常见坑

| 现象 | 原因 |
|---|---|
| `Group 不存在: xxx` | `--group` 拼错，或该组在别的项目里 |
| `节点不存在: g/n` | `--node` 拼错 |
| `schema 校验未通过，未创建项目` | preset 数据 / 字段名 / 引用不过关，看逐条 `line N` |
| `保存失败：项目存在 N 个验证错误` | 先 `validate` 定位，再改 `.tbl` |
| `Project X 打不开: … 解析失败` | 该项目的 `project.tblmanifest` 或某个 `.tbl` 坏了；`project new` 仍可用 |
| `--fmt json` 没输出 JSON | 该命令不支持（只有查询类支持） |
| `工作区正被另一个 GUI 使用（PID xxx）` | GUI 开着时 CLI 拒绝执行；先关 GUI（`util` / `list-templates` 不受限） |
