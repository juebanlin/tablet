# skills

面向 **tablet** 用户的 AI 辅助技能包。复制到你的 AI 编程工具的 skill 目录后，
AI 助手就能按规则**生成配置表并驱动 CLI 跑流水线**。

## 内容

| 技能 | 覆盖 |
|---|---|
| [`tablet/`](tablet/) | `.tblschema` / `.tbl` 文件格式 + `tablet-cli` 全部命令 + 退出码约定 |

一个技能管全流程（写文件 → 建项目 → 验证 → 导出），不再拆成 format / cli 两个包。

```
tablet/
├── SKILL.md                       入口：路线选择、硬规则、流水线、速查
└── references/
    ├── format.md                  .tblschema v1 与 .tbl v3 完整语法
    ├── types.md                   类型系统、引用、关键字、非法组合
    ├── cli.md                     命令全表、退出码、典型流水线
    └── example.tblschema          可被 CLI 直接验证通过的完整例子
```

## 安装

| 工具 | 目标位置 |
|---|---|
| [Claude Code](https://claude.com/claude-code) | `<your-project>/.claude/skills/tablet/` |
| 其它支持 SKILL.md frontmatter 的工具 | 按其约定路径 |
| 不识别 SKILL.md 的工具 | 把 `SKILL.md` 与 `references/*` 作为系统提示 / 上下文资料一起塞 |

装好后说「用 tablet 技能给我生成一套配置表并导出」即可（或 `/tablet`）。

## 自检

`references/example.tblschema` 是**机器可验证**的样板：

```bash
tablet-cli -w /tmp/tbl-check project new \
  --from-file skills/tablet/references/example.tblschema --id demo
tablet-cli -w /tmp/tbl-check --project demo validate   # 期望 exit 0
```

改完技能文档后跑一遍，确保示例没有随实现漂移。

## 更新

技能内容随 tablet 版本演进，覆盖式拷贝即可。
