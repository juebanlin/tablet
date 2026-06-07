# Licensing

`tablet` 由三个 crate 组成，授权按 crate 分别落地：

| Crate | 产物 | License | 备注 |
|---|---|---|---|
| `tablet-core` | rlib | Apache-2.0 | 纯库，可被任意复用 |
| `tablet-cli` | `tablet-cli[.exe]` + lib | Apache-2.0 | 不依赖 Slint，Jenkins / 自动化无障碍 |
| `tablet-slint` | `tablet[.exe]` (GUI) | GPL-3.0-only | 静态链 [Slint](https://slint.dev/) GPL 版，整体被 GPLv3 覆盖 |

## 选择理由

希望「任何人都可以使用、可以商用」并且「不希望他人直接拿源码改造重新发布而不开源」。

- **Apache-2.0** 覆盖核心库与 CLI：商用 / 集成 / 二次发布完全自由，仅要求保留版权声明与修改标注（License §4）。
- **GPL-3.0-only** 覆盖 GUI：Slint 自身以 GPLv3 / Royalty-Free / Commercial 三选一授权，本项目选 GPLv3 一支。这意味着任何人想把 `tablet` 源码或二进制 fork 重新发布，必须以 GPLv3 公开其修改。普通用户**用 GUI 编辑配置文件不受影响**——编辑产生的 `.tbl` / `.tblschema` 数据文件不是 GUI 的衍生作品（参考 Blender / GIMP 工作流）。

## 文件

- [LICENSE-APACHE](LICENSE-APACHE) — Apache License 2.0 全文
- [LICENSE-GPL](LICENSE-GPL) — GNU GPL v3 全文
- [NOTICE](NOTICE) — Apache-2.0 §4(d) 要求的归属声明 + 第三方组件清单

## 想换 Slint Royalty-Free / Commercial 授权？

如果未来希望 GUI 二进制摆脱 GPLv3（例如想被闭源商业产品直接集成），向 Slint 申请 [Royalty-Free 授权](https://slint.dev/pricing.html)（年营收 < 100 万美元免费）或购买商业授权后，把 `tablet-slint/Cargo.toml` 的 `license` 字段改为 `Apache-2.0` 即可——核心 / CLI 已经是 Apache-2.0，不用动。
