# tablet

游戏 / 应用配置表编辑工具：以 `.tbl` / `.tblschema` 为本体的可视化 + CLI 工作流，支持多 Project 树根管理、模板驱动的新建项目、12 种语言平台导出（Java / Go / C++ / C# .NET / C# Unity / C# Godot / GDScript / Lua / TypeScript / XML / JSON / ...）。

- **`tablet`**（GUI）：Slint 写的桌面端，零参数 / `--gui` 走 GUI；其它参数转 CLI fallback
- **`tablet-cli`**：纯命令行工具，Jenkins / 自动化批处理用

## 文档

完整设计与使用文档在 [`../docs/`](../docs/)：

- [00-概述](../docs/00-概述.md) — 项目目标 / 文档导航
- [01-tbl 系统](../docs/01-tbl系统.md) — `.tbl` / `.tblschema` 文件格式与硬性规则
- [02-核心功能](../docs/02-核心功能.md) — Project / 模板 / 导出 / 验证 / 测试数据生成
- [03-CLI 工程](../docs/03-CLI工程.md) — 源码三层、仓库布局、`tablet-cli` 子命令清单
- [04-UI 设计](../docs/04-UI设计.md) — GUI 交互设计
- [05-Slint 实现](../docs/05-Slint实现.md) — GUI 实现细节
- [06-Excel 桥接](../docs/06-Excel桥接.md) — Excel 来回往返
- [07-开发路线](../docs/07-开发路线.md) — 历史决策 / 演进记录
- [08-测试](../docs/08-测试.md) — 测试体系
- [09-平台发布](../docs/09-平台发布.md) — 跨平台发布策略

## 构建

详细发布脚本见 [`scripts/README.md`](scripts/README.md)。

```bash
# 开发：dev profile（编译速度优先）
cargo build -p tablet-slint     # GUI 二进制 → target/debug/tablet[.exe]
cargo build -p tablet-cli       # CLI 二进制 → target/debug/tablet-cli[.exe]

# 发布：按宿主跑
bash scripts/release.sh host
```

## 授权

本项目按 crate 分别授权，详见 [LICENSE.md](LICENSE.md)：

| Crate | License |
|---|---|
| `tablet-core` / `tablet-cli` | [Apache-2.0](LICENSE-APACHE) |
| `tablet-slint`（GUI） | [GPL-3.0-only](LICENSE-GPL) |

GUI 静态链 [Slint](https://slint.dev/) 的 GPLv3 一支，因此 GUI 整体受 GPLv3 覆盖。**用 GUI 编辑产生的 `.tbl` 数据文件不是 GUI 的衍生作品**——和 Blender / GIMP 一样，工作流产物归用户所有，GPL 不传染。
