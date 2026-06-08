# Excel 桥接

UI 层包装 xlsx 编辑流的工作流文档。**xlsx 文件本身的读 / 写规则属于核心层** —— 详见 @02 "生成 Excel 文件" / "导入 Excel 文件"，本篇不重复。

桥接的本质：调起策划机器上的 Excel / WPS 编辑工具核心生成的 xlsx，关闭后把策划改动回写到 .tbl。CLI 不需要这条流程（要批量出 xlsx 直接 `tablet-cli excel export ...`，要回写直接靠脚本调 import 接口）。

## 1. 触发方式

UI 入口共两处：

- **GridRibbon「Excel 编辑」按钮**：选中 Table / Constant / Enum 叶节点时启用，等价于"该节点所属 group + include = [节点名]"。生成的 xlsx 只含该一张 sheet。
- **TreeSection Group 节点右键「用 Excel 打开（整组）」**：等价于"该 group + include = []"。生成的 xlsx 含组内全部 Table / Constant / Enum 一个一张 sheet（规则见 @02）。

两个入口走同一条 core API：`export_group_book(group, include: Option<&[&str]>)`。

## 2. 流程

```
触发（按钮 / 右键）
  │
  ├─ 1. 调用 core::export_group_book(group, include_opt) → 生成 xlsx 字节
  ├─ 2. 写到 <project_root>/.tbl-cache/<group>-<millis>.xlsx（带毫秒后缀，避免和上次残留冲突）
  ├─ 3. 调起编辑器（@launch_xlsx_editor）：open::that 系统默认 → 平台候选链兜底
  ├─ 4. AppState.excel_session = Some({project_id, group, xlsx_path, started_at, ...})
  │      → 状态栏显示「正在 Excel 中编辑 ...」+ 全屏 modal 屏蔽其它写操作
  │
  └─ 后台监控线程（启动后先 sleep 3s 防 Excel 启动窗口期）：
       loop { sleep(1s); xlsx_is_released(path) ? 触发回写 : 继续 }
       │
       ├─ 5. weak.upgrade_in_event_loop → 主线程触发 excel-closed-detected callback
       ├─ 6. 主线程调 core::import_xlsx_into_group(path, group) → Result<GroupPatch>
       │      Ok(patch)  → apply_patch_to_group(group, patch) → revalidate_all
       │      Err(e)     → 日志窗口显示错误，**内存模型保持不动**
       └─ 7. 删除 .tbl-cache/<group>-<millis>.xlsx，AppState.excel_session = None
```

第 1、6 步是核心层调用；2、3、4、5、7 步是 UI 层职责。

xlsx 缓存放在**该 Project 自带的** `.tbl-cache/` 子目录下（@03.2 Project 目录结构），多 Project 同时管理时各自隔离；`.gitignore` 有 `projects/*/.tbl-cache/` 一行兜底（@03.2）。

**dirty 标记自动产生**：`apply_patch_to_group` 替换 records / entries 后调 `update_dirty()`，靠现有比对机制（`serialize(node) != original`）自然冒到树节点 `*` 标记 / 工具栏「保存」按钮。这一路不写任何 .tbl 文件——保存仍是用户主动点的。

### 2.1 跨平台调起策略（@`launch_xlsx_editor`）

优先 `open::that(path)` 走系统默认（尊重用户的 .xlsx 文件关联），失败时按平台候选链兜底：

| 平台 | 候选链 |
|---|---|
| Windows | Microsoft Excel（`Office16/EXCEL.EXE` 等已知路径） → WPS Office（`et.exe`） → LibreOffice Calc（`scalc.exe`） |
| macOS | Microsoft Excel.app → wpsoffice.app → LibreOffice.app → Numbers.app（用 `open -a "<bundle>"` 调起） |
| Linux | `libreoffice` / `soffice` → `et` / `wps` → `onlyoffice-desktopeditors`（用 PATH 查找） |

90% 用户的系统默认就是想用的（已配好关联），候选链只在系统默认失败时启用——日志能看到实际调起的应用名。强诉求改优先级可后续加 `tablet.toml [excel] preferred_app` 配置项。

### 2.2 跨平台关闭探测策略（@`xlsx_is_released`）

三层降级，任一层说"还在用"立即返 false；都说"释放"才真释放：

| 层 | 实现 | Windows | macOS | Linux |
|---|---|---|---|---|
| ① Lock 文件 | 同目录看 `~$<file>` (MS Excel/WPS) 或 `.~lock.<file>#` (LO/OnlyOffice) | ✅ Excel/WPS 主信号 | ✅ Excel for Mac 主信号 | ✅ LibreOffice 主信号 |
| ② `/proc/*/fd/` 扫描 | 遍历进程 fd 符号链接 | — | — | ✅ 兜底，catch 没 lock 文件的编辑器 |
| ③ OS write 锁 | `OpenOptions::write().open()` 是否成功 | ✅ Excel 持 share lock 时极准 | best-effort | best-effort |

各平台的实际可靠性：Windows 三层皆稳；macOS 主靠 ①；Linux 主靠 ① + ② 兜底。

## 3. 容错机制

| 场景 | 处理 |
|------|------|
| Excel 长时间未关闭 | 4 小时超时自动强制放弃（不合并任何改动 + 删 xlsx + log），用户可重新调起 |
| Excel/WPS 崩溃 | 监控线程不区分崩溃与正常关闭——独占文件锁释放就触发回写。三层探测（lock 文件 / Linux /proc fd / OS write）确保跨平台准确 |
| 工具启动时有残留 | `excel_bridge::scan_residuals_on_startup` 启动后扫所有 Project 的 `.tbl-cache/*.xlsx` 静默删除 + 日志通报 |
| 策划主动放弃 | modal 弹窗里的「强制放弃（不合并）」按钮调 `abort_session` |
| **放弃后 Excel 仍开着 → 再次点击「Excel 编辑」** | 每次会话用 `<group>-<millis>.xlsx` 独立文件名（@`unique_xlsx_name`），新会话直接拿新路径，不会和未释放的旧文件冲突；旧残留交给下次启动 / 退出清理 |
| 关 Excel 后无任何改动 | core 解析回与原数据相同的 patch → apply 后 dirty 比对 false，无变更 |
| 退出工具 | `cleanup_all_caches_on_exit` 删除所有 Project 的 `.tbl-cache/*.xlsx` + 终止监控线程 |
| 解析报错 | core 返回 `Err(...)`（结构错误），UI 在日志窗口列出错误信息；**不应用任何 patch**，整次回写原子拒绝 |

**严格 header 校验**：策划改了 xlsx 表头任一格、删 / 加 sheet、改列数 → 整次回写拒绝，日志显示具体错误。这是为了把"xlsx 是数据编辑器，不是 schema 编辑器"这条边界打到日志而不是默默改 schema。详细规则见 @02 §20。

**全局 UI 屏蔽**：session 进行中渲染一个全屏 modal overlay（半透明 scrim + 中央对话框），吃掉所有点击 / 拖拽事件；用户除了等 Excel 关闭或点「强制放弃」之外做不了任何写操作——简单可靠地避免内存模型在 patch 应用前被并发改动。

## 4. 剪贴板兼容（TSV/CSV）

GridArea 单元格 / 选区支持 Excel 风格的复制粘贴：

| 操作 | UI 端处理 |
|------|----------|
| Ctrl+C 复制选区 | 单元格用 `\t` 拼，行用 `\n` 分；调用平台剪贴板（windows: `clipboard-win`，slint: `slint::SharedString` + 平台桥接） |
| Ctrl+V 粘贴 | 读剪贴板字符串，按 `\t` / `\n` 切回 2D；按当前选区起点逐格写入，超出选区自动扩展（与 Excel 一致） |
| 剪贴板含逗号 | 仅切 `\t`，不切 `,`（避免误把 `Tuple<int,int>` 这种值拆开） |

剪贴板格式与 xlsx 是**两条独立的桥**：剪贴板走文本，xlsx 走二进制；它们的字段类型解析都委托回 core 的 `parse_*` 函数，保证 UI / 核心一致。

实现位置：`crates/app-slint/src/ui/grid_actions.rs`（剪贴板读写）+ `crates/app-slint/src/ui/focus.rs`（绑 `grid-shortcut-copy/cut/paste` callback）。底层走 `arboard` 读写系统剪贴板。

## 5. 状态

| 子项 | 状态 |
|------|------|
| 核心 xlsx 写出（`export_group_book(group, include)`） | ✅ S14-A |
| 核心 xlsx 回读（`import_xlsx_into_group(path, group) -> GroupPatch`） | ✅ 含严格 header 校验 + round-trip 单测 |
| CLI 子命令（`tablet-cli excel export --group <g> [--include t1,t2]`） | ✅ |
| UI 调起 / 监控 / 回写 / dirty 标记 / 删 xlsx | ✅ |
| Modal 弹窗全屏屏蔽 + 状态栏提示 + 强制放弃按钮 | ✅ |
| 启动残留扫（静默删除）+ 退出清理 + 4h 超时自动强制放弃 | ✅ |
| 行颜色标记（`#@c:RRGGBB` ↔ xlsx 行背景色） | ⏸ 计划独立任务（S14-Color），见本文 §6 |
| 剪贴板 TSV 复制粘贴 | ✅ |

## 6. 已知限制 / 后置项

- **行颜色标记（S14-Color）**：tbl 行尾 `#@c:RRGGBB` ↔ xlsx 行背景色双向映射。需要扩 .tbl 文本格式 + UI 行号区色块渲染 + xlsx 写出/读回背景色，是横切改动，独立排期。
- **Constant value 浮点精度**：策划在 Excel 里填 `1.5`，calamine 按 f64 解，转回字符串可能出 `1.5000000000000002`。当前 `cell_to_string` 对 `Float` 整数化处理（`1.0 → "1"`）能解决整数被误读的常见情况；非整数浮点的精度边界尚未验证。建议 Constant `value` 列 .tbl 类型用 `str` 包装数字字面量规避。
- **Excel 启动瞬间窗口期**：`open::that()` 返回到 Excel 真正持锁有几百 ms 间隙，期间监控线程的 [`xlsx_is_released`] 可能立即误判为"已关闭"。已通过监控线程启动后 sleep 3s 缓解，仍是 best-effort。
- **WPS 锁文件行为**：WPS for Windows 写 `~$file.xlsx` 与 Excel 一致；但 macOS / Linux 的 WPS 锁文件命名细节未实测全。如发现不准，参照 [`has_lock_file`] 添加新模式。
- **极端行数性能**：中型 group（≤ 20 sheet × 2000 行）写出 + 读回 ≤ 2s 是合理目标。一组 50 万行的极端用例未测，应也工作但 UI 可能 freezing 几秒——建议大表拆 group。
