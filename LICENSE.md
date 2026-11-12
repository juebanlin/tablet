# Licensing

`tablet` 由三个 crate 组成，全部使用 **Apache-2.0**：

| Crate | 产物 | License | 备注 |
|---|---|---|---|
| `tablet-core` | rlib | Apache-2.0 | 纯库，可被任意复用 |
| `tablet-cli` | `tablet-cli[.exe]` + lib | Apache-2.0 | Jenkins / 自动化无障碍 |
| `tablet`（`crates/app-tauri`） | `tablet[.exe]`（GUI + CLI fallback） | Apache-2.0 | Tauri（MIT/Apache-2.0）+ WebView2 + Univer（Apache-2.0），不传染 GPL（@17） |

## 选择理由

希望「任何人都可以使用、可以商用」并且「不希望他人直接拿源码改造重新发布而不开源」。

- **Apache-2.0** 覆盖全部 crate：商用 / 集成 / 二次发布完全自由，仅要求保留版权声明与修改标注（License §4）。
- 早期 Slint 版 GUI 因静态链接 Slint GPLv3 分支而整体 GPL-3.0-only；Slint 版已在 3.0 移除，仓库不再包含 GPL 组件。

## 文件

- [LICENSE-APACHE](LICENSE-APACHE) — Apache License 2.0 全文
- [NOTICE](NOTICE) — Apache-2.0 §4(d) 要求的归属声明 + 第三方组件清单
