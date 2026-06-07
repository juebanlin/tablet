# skills

面向 **tablet 用户** 的 AI 辅助技能包。复制到自己 AI 编程工具的 skill 目录后，AI 助手即可识别并按规则生成 / 校验 / 扩展 `.tbl` 与 `.tblschema` 文件内容。

## 内容

| 技能 | 作用 |
|---|---|
| [`tbl/`](tbl/) | `.tblschema` v1 / `.tbl` v2 文件格式权威说明 + 工作流（生成新模板、追加 `# @preset` 数据、字段类型设计、手工校验） |

> 范围**只覆盖文件内容**。GUI / CLI / 工具配置等是各自工具的事，不在这里。

## 安装

按你使用的 AI 工具，把目录拷到对应位置：

| 工具 | 目标位置 |
|---|---|
| [Claude Code](https://claude.com/claude-code) | `<your-project>/.claude/skills/tbl/` 或 `~/.claude/skills/tbl/` |
| 其它支持 SKILL.md frontmatter 的工具 | 按其约定路径 |
| 不识别 SKILL.md 的工具 | 把 `tbl/SKILL.md` 与 `tbl/references/*` 作为系统提示 / 上下文资料一起塞 |

拷贝后在编辑会话里说 "用 tbl 技能给我新建一个 schema"（或工具支持的 `/tbl` 触发语法）即可。

## 更新

技能内容随 tablet 版本演进。覆盖式拷贝即可。
