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

## 代码风格

- 注释与文档用**中文**；代码标识符用英文。
- 单元格 / 配置项等业务概念在注释里点明「这是干什么的」，别只复述代码。
