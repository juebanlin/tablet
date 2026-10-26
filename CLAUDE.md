# CLAUDE.md

本文件记录本项目的开发习惯与约定，供 Claude Code 每个会话自动加载。
**后续任何新的项目习惯 / 约定都追加到这里**（不必另开文件）。

## 构建

- **统一走 dev profile（`cargo build` / `cargo check` / `cargo test`），不要用 `--release`。**
  `Cargo.toml` 的 `[profile.dev]` 已针对**编译速度**调优（`opt-level=0`、`debug="line-tables-only"`、
  `incremental=true`、`codegen-units=256`），日常编译比 release 快很多。
  release 只在**最终交付 / 性能验证**时才用。
- 日常类型检查优先 `cargo check`（比 build 更快）；要跑起来或跑测试时再用 `cargo build` / `cargo test`。
- 全量回归：`cargo test --workspace`。

## 提交

- **不要自动 `git commit`。** 改完只报告「改了什么、验证结果如何」，停下等用户测试验收；
  用户明确说「提交」才提交。
- 需要用户先在界面上验收的改动（GUI 行为、交互），报告时**点明要验收什么**（哪个界面、什么操作、预期现象）。
- 一条待验收的改动还没被验收时，不要顺手把它和别的改动一起提交。

## 本机工具链

- 本机 Rust **默认宿主是 gnu**；编 msvc 目标要显式加 `+toolchain`（如 `cargo +stable-x86_64-pc-windows-msvc ...`）。
  Windows 发布包按 docs/09 走 `windows-gnu`。

## 版本号

- `Cargo.toml` 的 `[workspace.package] version` 是**单一真理源**，三个 crate 用 `version.workspace = true` 继承。
- **发版打 tag 后立即 bump** 这里的版本号。

## 文档

- 文档集中在 `docs/`，命名 `NN-标题.md`（NN 为两位序号，连续）。
- 章节用 `## N.` 数字编号，跨文档引用写 `@NN.N`（同文档省略 `NN`）。
- 相关文档改动要同步：新增文档记得在 `docs/00-概述.md` §4 索引表加一行。
- `skills/` 只写**当前最新范式**：不解释历史沿革、旧实现、兼容原因或防错废话；
  命令、格式和目录约定以当前代码为准。
- `skills/tablet/` 是 skill 的唯一真理源；改完必须同步到 `.claude/skills/tablet/`
  （该目录被 `.gitignore` 忽略，仅供本机加载），提交前用
  `diff -rq skills/tablet .claude/skills/tablet` 确认零差异。

## 代码风格

- 注释与文档用**中文**；代码标识符用英文。
- 单元格 / 配置项等业务概念在注释里点明「这是干什么的」，别只复述代码。

## 数据格式：不做旧数据兼容

> **本节的存废就是开关。要重新允许兼容旧格式，先删掉本节——在那之前，一律按"不兼容"办。**

- 磁盘格式的改动（`.tbl` / `.tblschema` / `tablet.toml` / `project.toml`）**只管新格式**：
  不写迁移代码、不留兼容分支、不"读到不认识的就兜底成默认值"。
- 读到旧的或非法的内容 → **报错展示，带位置**（行号 / 列 / 字段名），由用户自己处理。
- 已经把格式改成新版了就当旧文件不存在；不要为了"老项目能打开"而加回兼容路径。
- 具体到 `export` 列：**只认短码** `cs` / `c` / `s` / `-`（留空 = `cs`）。中文
  「前后端 / 客户端 / 服务器 / 不导出」**仅用于 UI 展示与 Excel 同步回读**，不是磁盘格式的一部分。
  实现上就是 `Export::parse_code`（严格，`.tbl` / `.tblschema` 解析用）与
  `Export::from_input`（宽松，人给的输入 / Excel 用）两个入口——**不要合并**。
