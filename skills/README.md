# skills

面向 **tablet** 用户的 AI 辅助技能包。复制到自己 AI 编程工具的 skill 目录后，AI 助手即可识别并按规则操作 tablet 工具链。

## 术语

| 名称 | 含义 |
|------|------|
| **tablet** | 项目名 / GUI 程序名（带参数时自动转为 CLI 模式） |
| **tablet-cli** | 纯 CLI 程序名（功能与 `tablet` CLI 模式一致） |
| **.tbl** | 数据文件格式（表/常量/枚举） |
| **.tblschema** | 结构定义文件格式（schema + 预设数据） |
| **tbl** | 技能名 — 文件内容格式相关 |
| **tbl-cli** | 技能名 — CLI 命令操作相关 |

## 内容

| 技能 | 作用 |
|---|---|
| [`tbl/`](tbl/) | `.tblschema` / `.tbl` 文件格式规范 + 工作流（生成模板、设计字段类型、追加预设数据、手工校验） |
| [`tbl-cli/`](tbl-cli/) | `tablet-cli` / `tablet` 全部 CLI 命令参考（项目管理、结构操作、导出、验证、Excel 桥接、分隔符查询、文件工具） |

> `tbl` 管**怎么写文件**，`tbl-cli` 管**怎么调命令**。两者互补，按需安装。

## 安装

按你使用的 AI 工具，把目录拷到对应位置：

| 工具 | 目标位置 |
|---|---|
| [Claude Code](https://claude.com/claude-code) | `<your-project>/.claude/skills/tbl/` 和/或 `.claude/skills/tbl-cli/` |
| 其它支持 SKILL.md frontmatter 的工具 | 按其约定路径 |
| 不识别 SKILL.md 的工具 | 把 `SKILL.md` 与 `references/*` 作为系统提示 / 上下文资料一起塞 |

拷贝后在编辑会话里说 "用 tbl 技能给我新建一个 schema"（或 `/tbl`）、"用 tbl-cli 导出 JSON 数据"（或 `/tbl-cli`）即可。

## 更新

技能内容随 tablet 版本演进。覆盖式拷贝即可。
