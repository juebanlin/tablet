# Excel 桥接

UI 层包装 xlsx 编辑流的工作流文档。**xlsx 文件本身的读 / 写规则属于核心层** —— 详见 @02 "生成 Excel 文件" / "导入 Excel 文件"，本篇不重复。

桥接的本质：调起策划机器上的 Excel / WPS 编辑工具核心生成的 xlsx，关闭后把策划改动回写到 .tbl。CLI 不需要这条流程（要批量出 xlsx 直接 `tablet-cli excel export ...`，要回写直接靠脚本调 import 接口）。

**两条 Excel 流（注意区分）**：

| 流 | 入口 | 核心模块 | 用途 |
|----|------|---------|------|
| **Excel 编辑桥接**（本文 §1-§6） | GridRibbon「Excel 编辑」/ Group 右键「用 Excel 打开」 | `tablet_core::excel` | 导出 xlsx → 调起外部编辑器 → 监控关闭 → 回写内存模型 |
| **Excel 同步面板**（本文 §7） | 底部面板「Excel 文件」tab →「同步」 | `tablet_core::excel_sync` | 对比 `.excel/*.xlsx` 与内存 group，逐表给同步动作 |

两条流共用 `tablet_core::excel` 的严格导出/导入 API 作为基础，但定位不同：编辑桥接是"单向给策划改完自动回填"，同步面板是"人工对比 + 选方向合并"。

## 1. 触发方式

UI 入口共两处：

- **GridRibbon「Excel 编辑」按钮**：选中 Table / Constant / Enum 叶节点时启用，等价于"该节点所属 group + include = [节点名]"。生成的 xlsx 只含该一张 sheet。
- **TreeSection Group 节点右键「用 Excel 打开（整组）」**：等价于"该 group + include = []"。生成的 xlsx 含组内全部 Table / Constant / Enum 一个一张 sheet（规则见 @02）。

两个入口走同一条 core API：`export_group_book(group, include: Option<&[&str]>)`。

> **Xlsx 组不参与这里**（@02.26.8）。那类组的工作簿本身就是真相源，没有"导出临时本 →
> 改完回填"这回事：两个入口对它都改成「先落盘，再用系统程序打开原文件」，
> 同步面板（@7）也会跳过它。

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
| **Excel 同步面板**（底部面板「同步」，`excel_sync` 核心引擎，见本文 §7） | ✅ |
| **Excel 文件浏览器**（卡片式，`.excel` 目录，双击打开） | ✅ |

## 6. 已知限制 / 后置项

- **行颜色标记（S14-Color）**：tbl 行尾 `#@c:RRGGBB` ↔ xlsx 行背景色双向映射。需要扩 .tbl 文本格式 + UI 行号区色块渲染 + xlsx 写出/读回背景色，是横切改动，独立排期。
- **Constant value 浮点精度**：策划在 Excel 里填 `1.5`，calamine 按 f64 解，转回字符串可能出 `1.5000000000000002`。当前 `cell_to_string` 对 `Float` 整数化处理（`1.0 → "1"`）能解决整数被误读的常见情况；非整数浮点的精度边界尚未验证。建议 Constant `value` 列 .tbl 类型用 `str` 包装数字字面量规避。
- **Excel 启动瞬间窗口期**：`open::that()` 返回到 Excel 真正持锁有几百 ms 间隙，期间监控线程的 [`xlsx_is_released`] 可能立即误判为"已关闭"。已通过监控线程启动后 sleep 3s 缓解，仍是 best-effort。
- **WPS 锁文件行为**：WPS for Windows 写 `~$file.xlsx` 与 Excel 一致；但 macOS / Linux 的 WPS 锁文件命名细节未实测全。如发现不准，参照 [`has_lock_file`] 添加新模式。
- **极端行数性能**：中型 group（≤ 20 sheet × 2000 行）写出 + 读回 ≤ 2s 是合理目标。一组 50 万行的极端用例未测，应也工作但 UI 可能 freezing 几秒——建议大表拆 group。

## 7. Excel 同步面板

底部面板「Excel 文件」tab 提供独立的**同步面板**，用于对比 `.excel/*.xlsx` 与内存 group 的数据差异，并按表逐行选择同步动作。与 §1 的"Excel 编辑桥接"（调起编辑器自动回填）不同，同步面板是**人工对比 + 选方向合并**。

### 7.1 Excel 文件浏览器（卡片式）

底部面板「Excel 文件」tab 是 `.excel` 目录（与 excel_bridge 的 `.tbl-cache` 不同的独立目录）的文件列表：

- 扫描 `project_root/.excel/*.xlsx`，按文件名排序，每文件一张卡片（📄 图标 + 文件名）
- **单击**行 = toggle 选中（选中态高亮边框 + 背景）
- **双击**行 = 用系统默认程序直接打开 xlsx（**纯查看**，不建编辑会话、不监控、不回写）
- 工具栏：`同步`（打开同步对话框）/ `刷新`（重扫目录）/ `删除`（删除选中文件，需有选中）/ `打开`（系统默认打开选中文件）

### 7.2 同步对话框

标题「Excel 同步」（920×550），底部「刷新」+「关闭」。数据模型 `EsRow`，每行对应一个 (group, table) 或 (group, xlsx sheet) 配对。

**表头 4 列**：`组 / 节点` ｜ `tablet(行x列)` ｜ `操作` ｜ `Excel(行x列 | 匹配+差异)`

**列只能从 tbl 侧（schema）添加。** xlsx 里多出来的列**一律不碰**——既不收进 schema，也不删。这不是边角情况：**策划经常在 schema 列后面放自己的测试列、公式列**，那些列本来就不该进 schema。所以：

| xlsx 相对 tablet | 给的按钮 |
|---|---|
| 完全匹配，或 xlsx 多出策划自己的列 | `数据→` / `←数据`（匹配前缀内照常同步） |
| tablet 新加了字段，且不与策划的列重名 | `数据→` / `←数据` / `补列→`（**插入**，策划的列往右推） |
| 前缀之后两边有重名穿插（多半是列名被改过） | 只给 `强制→` |
| 表头首列对不上 | 只显示原因，不给任何按钮 |

按这条，进度列在界面上也**不**为「xlsx 多列」记日志——它是常态，每次都报就是噪音。

**xlsx 的表头是 4 行**（desc / export / type / 列名，数据从第 5 行起），与 @02 §19 的导出布局逐格一致。定位是**让策划自由操作 Excel**：约定列名不改、desc 可改、颜色与行高列宽随便调，而这份 xlsx 与 tbl 之间**只交换 desc + 数据**。所以同步是双向的——`数据→` 连 desc 一起刷新，`←数据` 把 desc 和数据一起带回 tbl。**desc 差异也计入「有差异」判定**，只改了 desc 的行同样会亮出同步按钮。

**每行状态**：

| 状态 | 判定 | 显示 |
|------|------|------|
| 完全匹配 | `tablet_only==0 && xlsx_only==0` | 操作：`数据→` / `←数据` |
| 仅 tablet 多列 | `tbl_more_cols` | 操作：`数据→` / `←数据` / `补列→` |
| 两边各有独有列 / 仅 xlsx 多列 | `hdr_mismatch` / `xlsx_more_cols` | 操作：`强制→`（整表覆写） |
| tablet 有、xlsx 无 | `left_only` | 操作：`创建→` |
| xlsx 有 sheet、tablet 无 | `right_only` | 操作：`←导入` |
| 表头首列不匹配 | `matched_prefix==0` | 仅 `⛔`，原因「表头不符合规范」 |

**操作按钮**：

| 按钮 | 核心调用 | 语义 |
|------|---------|------|
| `数据→` | `Push { MatchedOnly }` | 刷新匹配前缀列的表头 + 数据，不补列 |
| `←数据` | `Pull` | 从 xlsx 合并回 tablet（只覆盖 xlsx 有的列，desc 一并带回） |
| `补列→` | `Push { WithTabletColumns }` | **插入**在匹配前缀之后：tablet 新加的字段插进去，策划自己的列往右推 |
| `强制→` | `Push { WholeTable }` | 本表就地重写：表头 4 行 + 全部数据行，并清掉多余的列与数据行 |
| `创建→` | `CreateFile` | xlsx 文件不存在，新建一份 |
| `←导入` | `SyncOp::ImportSheet` | xlsx 独有 sheet 导入（**尚未实现**，点下去会如实报错，不再静默做一次拉数据） |

**`强制→` 是就地重写，不是重建工作簿**：它只在目标 sheet 上重写表头与数据、清掉多余的列和行，同一文件里其他 sheet、策划刷的颜色与调的高宽都留着。文件都不存在时（`创建→`）才走「从零造一份」。

所有操作即时执行，结果写入 UI 日志（`[同步]` / `[同步失败]`）。执行前对 `数据→` / `←数据` 做差异守卫：无差异时拒绝（`数据已同步，无需操作`）；desc 差异算差异。执行完成后自动刷新主界面与对话框。

**回读时「空 = 清空」**：匹配前缀内以 Excel 为准，空串一样覆盖 tbl 的值。以前有一道「空值不覆盖」的守卫，它同时干着划范围和不覆盖两件事——结果是策划在 Excel 里清空一个格、tbl 的值却清不掉。范围现在由 `matched_prefix` 显式表达。

**回读会正确产生未保存标记**：回读走 `update_dirty()` 比对落盘快照，不伪造快照。所以同步完树节点会出现 `*`，**必须点「保存全部」才真正写进 `.tbl`**。

**type / export 行被改会记一行日志**：这两行按约定策划不该动，改了工具不会照做，但也不会静默忽略（那和本仓库栽过的几类「默默吃掉改动」同源）。日志形如 `[同步] hero / HeroBase：type 或 export 行被改过，已忽略（只有 desc 可改）`。

### 7.3 核心引擎（`tablet_core::excel_sync`）

与 `tablet_core::excel`（严格导出/导入桥）平级的新模块，**仅 GUI 同步面板使用，CLI 未暴露**：

| 模块 / API | 作用 |
|-----|------|
| `vo::SheetVo` | 两侧共用的行列表模型：列轴（`ColumnVo`，带物理列号）+ 行轴（`RowVo`，带物理行号）+ `range` |
| `read::read_xlsx_sheets(path, group)` | 读 xlsx 全部 sheet → `Vec<SheetVo>`；`group` 用来判定表头是新 4 行布局还是旧的 1 行布局 |
| `read::tbl_sheet_vo(table)` | `Table` → `SheetVo` |
| `plan::plan_group(group, sheets)` | **纯函数**：两侧 VO + schema → `GroupPlan`。每个配对的 `ops` 就是「这一行允许做哪些操作」 |
| `apply::execute(path, group, node, op)` | 重算计划 → 校验 op 仍合法 → 落盘（写 xlsx 或写内存模型） |

「怎么对齐列」是 `plan` 的黑盒，结果放进 `PairPlan.col_match`；`apply` 只消费不重算（有一条源码级断言钉着：`apply.rs` 里不出现 `compute_column_match`）。换匹配机制 = 只改 `plan`。

`SyncOp` 的**内容就是它的身份**——面板回传的是操作本身而不是它在列表里的下标。

底层用 **umya-spreadsheet** 读写（保留已有 xlsx 格式化），与 `tablet_core::excel` 的 calamine + rust_xlsxwriter 路线不同。推送走 `lazy_read`，只反序列化要写的 sheet，其余走 zip 原始字节直通（@02 §20.1）。

### 7.4 已知缺口

- **`←导入` 尚未实现**——从 xlsx 独有的 sheet 建表这件事本身还没做。按钮照常显示，但点下去会**如实报「尚未实现」**；它以前和 `←数据` 共用同一条路径，点了会静默地做一次拉数据，是另一种「默默吃掉操作」。
- `.excel/` 目录与 `.tbl-cache/` 相互独立；启动/退出清理只处理 `.tbl-cache`，`.excel` 下的 xlsx 由用户手动管理或删除。
- **`lazy_read` 保真的边界**：没被同步的 sheet 逐字节原样保留；**被同步的那几张表**会走 umya 的再序列化，丢掉它没有建模的元素——行/列分组折叠（大纲）、已应用的筛选/排序条件（筛选范围还在、条件没了）、适应页面缩放、迷你图/切片器、条件格式里的自定义数字格式、`tableStyles` 样式列表。单元格样式、列宽行高、冻结窗格、合并单元格、条件格式、数据验证、批注、超链接、图片图表都保留。
- **初次生成的产物是纯数据文本**：不带任何样式、冻结窗格、工作表保护。样式是 Excel 侧的东西——生成时不预设格式，之后策划在这个文件上做的一切（底色 / 边框 / 列宽 / 行高 / 公式列）由 `push` 的定点写入原样留存。有单测钉着（`fresh_file_carries_no_styling_at_all`）。
- **umya 必须 ≥ 3.1**：3.0.1 的读 → 写往返会把**边框定义**清空（`<left style="thin"><color auto="1"/></left>` 变成 `<left/>`），连带让「只有边框」的格子连样式引用都掉光——现象就是「同步一次，边框全没了」。3.1.1 已修。`apply.rs` 的 `umya_round_trip_keeps_all_cell_styles` 把这条钉住了，升/降级回归会立刻红。
- **列名不锁**：xlsx 不带 sheet protection。原因是这条流用 umya 原地编辑，而 umya 不认识 `<protectedRanges>`（"放开某片区域可编辑"那个元素）、会静默丢掉，却保留整表保护开关——结果是把文件变成整张只读。列名靠约定 + 面板兜底（改坏了列匹配前缀会变短，该行显示「表头不符合规范」）。
