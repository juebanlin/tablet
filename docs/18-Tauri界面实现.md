# Tauri 界面实现

> **状态：进行中（2026-11）。** 本文记录 Tauri 版 GUI 的实现约定与当前进度。
> 双 GUI 布局 / 打包 / 崩溃日志见 @17；界面需求与交互语义见 @04。
> **核心约定：从 Slint 版反推「用户需要什么」，再用 Tauri/Web 的方式实现；不照抄 Slint 的实现机制。**

## 1. 为什么不能照抄 Slint

Slint 和 Web 是两套完全不同的 UI 机制。Slint 源码里大量写法是**框架限制下的规避手段**，不是需求本身：

| Slint 里的写法 | 它其实在解决什么 | 照抄会走歪成什么 |
|---|---|---|
| 手写 4px `Rectangle` + `TouchArea` 的 splitter | 没有内置 splitter | 在 HTML 里重复造轮子，忽略 `pointer events` + CSS |
| 表头/行号手动同步 `scroll-x` / `scroll-y` | Slint 的 ScrollView 不支持 sticky | 写一套滚动同步，忽略 CSS `position: sticky` |
| `PopupWindow` 不能放进 `for`/`if`，改用全屏 Rectangle 模拟 | Slint 组件树限制 | 在 Web 里手写浮层，忽略原生 `<select>` / 定位浮层 |
| 22px 行高、0.5px 边框、`fs-tiny` 字号 | Slint 渲染与字体度量 | 把像素级细节当契约，牺牲 Web 的可读性和可维护性 |
| `for` 里包 `TouchArea` 做整行点击 | Slint 事件模型 | 忽略 DOM 事件委托 |
| 编辑态靠 `if` 切换 `LineEdit` 并处理销毁时序 | Slint 生命周期坑 | 在 Web 里担心不存在的重入/use-after-free |

**正确流程：**

1. 读 Slint 版和 @04，列出**用户要完成的事**（选择、编辑、拖选、改列宽、看公式栏…）。
2. 按 Web/Tauri 的机制选实现（sticky、pointer events、原生 input/select、事件委托、`navigator`/`clipboard` 事件）。
3. 只在「布局层级、区域归属、信息密度」上参考 Slint；尺寸和视觉不必逐像素一致。

视觉也按同一原则处理：Slint 的配色、字号、控件外观是配套它那套渲染的，不作为规范。Tauri 版采用现代桌面应用的视觉语言——系统中性色板 + 单一蓝色强调 + 圆角/轻阴影 + 系统可变字体，深浅色跟随系统；布局结构与交互语义保持一致即可。

## 2. 区域需求（从 Slint 反推）

| 区域 | 需求 | Tauri 实现 |
|---|---|---|
| Toolbar | 撤销/重做、保存/重新加载、当前项目 + 脏标记 | 固定 32px 横条，按钮禁用态跟随状态 |
| TreeSection | 项目 → 组 → 节点三层；展开/收起、搜索、过滤、完整组、新建项目、排序 | 宽 240px（可拖，最小 220），节点列表独立滚动 |
| GridSection | 表名/行列数、表级动作、公式栏、表格、选区状态 | 纵向五段：Header 24 / Ribbon 24 / Formula 26 / Grid 自适应 / Status 20 |
| BottomPanel | 日志（级别过滤、清空）与 Excel 文件两个 tab | 默认 120px（可拖，最小 40），两个 pane 切换 |
| 分隔线 | 左右、上下两处可拖 | 4px，`pointerdown + pointermove`，不持久化 |

字体走 Web 的常规做法：`-apple-system / Segoe UI Variable / Segoe UI` 打头，CJK 依次回退
`PingFang SC → Microsoft YaHei UI → Noto Sans CJK SC → WenQuanYi`，最后 `system-ui, sans-serif`；
等宽字体（Cascadia / SF Mono / Consolas）后面同样补 CJK 字体，避免中文落到宋体。
按钮 / 输入框 / 下拉统一 `font: inherit`，不混用浏览器控件默认字体。

## 3. 表格交互：需求 → Web 实现

| 需求 | Web 实现 | 备注 |
|---|---|---|
| 表头、列字母、行号固定；数据区双向滚动 | `<table>` + `position: sticky`（表头 `top`、行号 `left`） | 不手写滚动同步 |
| 单格选中 / Shift 扩选 / 拖拽框选 | `anchor + focus` 选择模型；拖拽用 `pointerdown/move/up` | 拖到边缘自动滚动（rAF） |
| 整行 / 整列 / 全选 | 点行号、列字母、左上角 | |
| 键盘导航 | 方向键、Shift+方向、Tab、Home/End、Ctrl+Home/End、PageUp/Down | 选中后自动 `scrollIntoView` |
| 编辑 | 双击 / F2 / 直接打字进入；`input` 覆盖单元格 | Enter 提交下移、Tab 右移、Esc 取消、失焦提交 |
| 公式栏 | 与当前单元格双向联动；Enter 提交并下移 | 只读列禁止编辑 |
| 复制 / 剪切 / 粘贴 / 删除 | `copy` / `cut` / `paste` 事件 + TSV；Delete 清空选区 | 不手写系统剪贴板通道 |
| 列宽 | 列字母右侧 6px 热区拖拽；双击自适应 | 手动拖拽会关闭「自动列宽」 |
| 列字母 | Excel 式编号：A…Z、AA、AB… | 与 Slint 版 `col_letter` 一致 |
| 横向滚动 | 表格区滚动条常驻（覆盖 WebView2 的自动隐藏）；插入列后自动把新列滚进可视区 | 否则列一多就看不出横向滚动入口、新列停在右边界外 |
| 错误单元格 | 红色内描边 | 与 @04 的错误展示一致 |
| 拖选性能 | 选择变化只切 class，不重建 DOM | 避免每次 pointermove 重绘整表 |
| 只读列 | `id` 等列灰底、不可编辑 | 与 @04 的 ReadOnly 语义一致 |

数值列按类型行右对齐（`int/long/float/double`），文本列左对齐；这是 Web 表格的常规做法，Slint 版没有，属于实现层改进。

## 4. 当前进度

已完成：

- Toolbar / TreeSection / GridSection / BottomPanel + 两处分隔线
- 树：三层结构、展开收起、搜索、过滤、节点选中
- 表格：固定表头/行号、单格/矩形/整行/整列选择、键盘、内联编辑、表头编辑、TSV 剪贴板、列宽拖拽与自适应
- 公式栏、状态栏、日志过滤与 tab 切换
- **真实数据**：Tauri command 直连 `tablet_core::ops::ProjectEngine`（项目树、表格投影、保存/撤销/重做/重新加载/验证）
- **右键菜单**：项目 / 组 / 节点 / 表格单元格、行号、列字母
- **节点增删改**：新建 Group（tbl / xlsx）、新建 Table·Constant·Enum、重命名、复制/粘贴、软删除、还原到已保存
- **表格行列操作**：右键插入/删除行、插入/删除列
- **新建项目**：空项目 / 内置·本地模板 / `.tblschema` 文件三种来源；新建后进入内存态（Pending），保存时落盘
- **项目设置**：身份（项目 ID 只读、名称、分类、版本）、分隔符（25 项 + 恢复默认）、导出（编码/换行/JSON·XML 空值、服务端数据目录与各语言包名·输出目录·自定义区间、客户端 Lua/GDScript/TS/Unity/Godot 输出）、校验策略（error/warn/info → block/report/ignore）；保存后自动重算校验
- **校验规则编辑器**（按 rule-builder-v2 / Slint 版重做）：列规则 / 表级规则；左侧列清单（规则条数）；规则卡片树（每一行是一个函数——验证器或条件函数；组；行尾 and·or；+行 / +组 / 分散 / 合并）；参数与说明/示例在底部 inspector（选中行后编辑，函数下拉同时含验证器与条件函数）；还原 / 保存；有未保存改动时切换列 / 作用域先显示「丢弃并切换 / 取消」确认条（同 Slint）；切到系统规则只是切视图、不动草稿；未保存关闭时丢弃确认
- **表头选择器**：`export` 单击弹出自定义下拉；`type` 单击弹出分组菜单（基础 / 容器 / 引用目标 / 自定义输入），不再依赖原生 `<select>`
- **引用显示**：显示引用名称总闸；被引用展示列（表默认原始 id，枚举默认 name）；列表引用按元素翻译
- **全局设置**：界面（表头/数据选择器单击·双击、失焦提交、实时校验、项目列表显示 ID、Constant 允许 @Xxx）、日志级别、默认分隔符（25 项）；写入 `tablet.toml`
- **导出执行**：JSON/XML + Java/Go/C++/C#(.NET·Unity·Godot)/Lua/GDScript/TS(客户端·服务端) 勾选导出，输出路径取项目导出配置
- **Schema 导入导出**：导出整项目 / 按分组·节点过滤 / 可选预设；导入 `.tblschema` 并合并（新增 / 覆盖统计）
- **Excel**：分组导出 `.xlsx`、导入回写、`.excel/` 文件列表与打开目录、「用 Excel 打开」、任意 Excel 映射导入（sheet / 表头行 / 数据起始行 / 自动字段建议）
- **问题面板**：校验错误 / 警告列表，点击跳转到对应节点与单元格；重新验证按钮
- **关于**：版本 / 目标平台 / 工作区 / 版本库；检查更新打开 GitHub
- **操作 Toast**：保存、导出、导入、校验结果与错误提示
- 树排序下拉已接通（ID / 名称 / 已打开 / 创建时间 / 手动；创建时间无字段时退回名称）
- 统一输入弹窗与确认弹窗

## 4.2 与 Slint 界面对照（逐界面）

| Slint 界面 / 组件 | Tauri 版状态 |
|---|---|
| Toolbar（撤销/重做/保存/重新加载/当前项目+脏标记） | 已对齐；额外增加 验证 / 导出 / 设置 / 关于 |
| TreeSection（标题 / 新建+排序 / 搜索+过滤+完整组 / 节点列表） | 已对齐；搜索支持拼音首字母；排序下拉已接通 |
| GridHeaderBar（标题、行列数、自动列宽） | 已对齐 |
| GridRibbon（还原 / Excel / 校验规则 / 显示引用名称 / 被引用展示列） | 已对齐 |
| FormulaBar（折叠单行 / 展开多行 / 只读标记） | 已对齐（textarea 多行，只读显示「只读」） |
| GridArea（选择、框选、键盘、编辑、表头类型·导出下拉、引用选择器、列宽、右键） | 已对齐；单值 / 列表引用都有选择器；类型走「选择类型」对话框，export 是自绘下拉 |
| StatusBar（选区 / 光标 / Excel 会话 / 单元格错误原因） | 已对齐：单格有校验问题时追加 `⚠ 原因`（即时重算，不受实时校验开关影响） |
| BottomPanel 日志 | 已对齐（级别过滤、清空、最新在上） |
| BottomPanel Excel 文件 | 已对齐：卡片网格 104×68、单选 / Ctrl 多选、双击打开；工具条 同步 / 刷新 / 删除 / 打开 / 打开目录 |
| 右键菜单（项目 / 组 / 节点 / 表格） | 已按 Slint 项集复刻：新建 Table v3/v2、导入 .tbl / Excel Group、另存为模板、合并 Schema、粘贴 v2/v3、粘贴 Group（tbl/xlsx）、打开目录、重新加载；**不可用项直接隐藏**（与 Slint 版一致） |
| 新建项目（空 / 从文件 / 从模板） | 已对齐：三来源 tab、原生文件选择、模板来源（内置/本地）、搜索、段多选 + 全选、项目 ID/名称/分类/版本、目录预览、立即打开、灌入预设；切换来源会清空上一来源的预览、段勾选与身份字段 |
| 原生文件对话框 | 已接入 rfd：新建项目文件、Schema 导入/导出、Excel 导入/导出、任意 Excel 映射 |
| 模板库 | 新建项目对话框内选择；无独立浏览窗口 |
| 项目设置（身份 / 分隔符 / 导出 / 校验） | 已对齐 Slint 模型：无「取消」、点「确定」直接落内存；每个改过字段淡蓝底 + ↺；底部 全部还原 / 应用模板（仅分隔符·导出页） |
| 全局设置（UI / 分隔符模板 / 导出模板） | 已对齐：编辑缓冲 + 每字段 ↺ + 「使用默认值」重置当前 tab；导出模板 29 个叶子与项目设置共用同一套控件 |
| 导出（数据 + 多语言代码） | 已对齐 |
| Schema 导入 / 导出 | 已对齐：metadata（ID/名称/分类/版本）+ 组/节点三态勾选 + 预设开关；导入先读预览并标 `⚠ 已存在(将覆盖) / ✓ 新增`；「另存为模板」走同一对话框 |
| 任意 Excel 映射导入 | 基础对齐（sheet / 表头行 / 数据起始行 / 字段建议） |
| 引用选择器 | 已对齐 Slint 版：id + 辅助列的表格（表头两行：说明 + 字段名）；表可切「智能 / 全部」列展示（默认取全局 `[ui.ref_picker]`）；当前值可直接输入；搜索 id / 任意列；单值单击选中、双击或「确定」提交，列表引用多选；「清空 / 取消 / 确定」 |
| 类型选择器 | 已对齐：独立对话框（数据类型 / 引用类型双 tab）；范式列表 + 参数槽 + `结果 / 示例 / Java / Go / Lua` 预览；constant 按全局开关禁用引用 tab |
| 规则编辑器 | 已按 rule-builder-v2 / Slint 版重做：规则卡片树，**每一行就是一个函数**（验证器函数或条件函数 eq/ne/in/match…），行尾 and·or；`＋行` 新增函数行、`＋组` 新增组；底部 inspector 统一「更换函数 + 参数 + 说明/示例」。条件行与同组验证器由保存时编译成 `when(条件, 验证器)`（与 Slint 相同，树里不出现 when 节点） |
| Excel 实时编辑 | 已实现：导出临时工作簿 → 用 Excel 打开 → 2s 轮询 mtime → 自动导入回内存；状态栏显示编辑中，右键可「结束 Excel 编辑」 |
| Excel 同步面板 | 已实现：读取 `core::excel_sync::plan_group`，逐 (组, 节点) 显示两侧行列数 / desc 差异 / 表头越权提示 / 可用操作（数据→、补列→、强制→、←数据、创建→、←导入）；点按钮前 core 会重算计划，过期操作直接拒绝 |
| 粘贴 Group（冲突命名） | 已对齐：先只读预览，有重名才弹「粘贴 Group」对话框，组名 + 每个子项可改（预填 `_copy`）；无冲突直接粘贴 |
| 克隆 / 关闭 / 删除项目 | 已对齐 |
| 行颜色标记 | 无数据来源：`core` 不解析 `#@color`，Slint 版也渲染为透明（两边一致） |
| 窗口标题（版本 + 分支） | 已对齐（Rust 侧设置原生标题） |

## 4.1 已知限制（非缺失菜单）

- **行颜色标记**：`core` 不把 `#@color` 解析进数据模型（Slint 版同样渲染为透明），没有数据来源可展示；不属于 Tauri 版与 Slint 版的差异。
- **Excel 实时编辑的实现方式**：用 mtime 轮询（2s）而不是 Windows 文件句柄监控；Excel 保存后最多 2 秒回写。Excel 未保存时不会读到半成品。
- **项目排序**：「创建时间」在 UI 树数据里没有字段，退回按名称；排序状态是会话内状态。

## 5. 开发 / 调试 / AI 驱动

Tauri 版把 WebView2 的 CDP 端口作为一等调试入口。相比 Slint 版只能靠 `PrintWindow` 截屏 + `PostMessage` 点击，这套能力可以直接读页面状态、模拟操作、截屏，适合 AI 驱动回归。

### 5.1 启动

```powershell
powershell -File crates/app-tauri/scripts/start-debug.ps1 -Workdir <工作区>
```

脚本会设置 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222` 并等待端口就绪。也可以手工设置该环境变量后启动 exe。

### 5.2 调试命令

```bash
node crates/app-tauri/scripts/dev.mjs snap                       # 树 / 表格 / 状态摘要（JSON）
node crates/app-tauri/scripts/dev.mjs shot dev.png               # 截屏到 target/tauri-shots/
node crates/app-tauri/scripts/dev.mjs click "<CSS 选择器>"        # 模拟按下 + 松开 + click
node crates/app-tauri/scripts/dev.mjs clicktext "Building"       # 按可见文本点击（PowerShell 下更稳）
node crates/app-tauri/scripts/dev.mjs key "ArrowDown"            # 对当前焦点派发按键
node crates/app-tauri/scripts/dev.mjs type "#tree-search" "hero" # 输入框赋值 + input/change
node crates/app-tauri/scripts/dev.mjs eval "<js 表达式>"          # 任意页面脚本
```

命令输出统一 JSON，便于脚本或 AI 解析。

### 5.3 页面侧调试接口

`window.__tt` 暴露 `{ grid, state, runOp, applySnapshot, selectTree }`：

- `state.snapshot`：最近一份后端快照（树 / 表格 / 状态 / 日志）。
- `runOp(cmd, args)`：串行调用 Tauri command 并应用返回快照，测试里也可以直接驱动业务动作。

### 5.4 回归脚本

`crates/app-tauri/scripts/` 下的回归脚本，都通过 CDP 驱动真实页面：

- `verify-grid.mjs`：节点加载 / 选择 / 编辑回写 / 撤销 / 表头 / 粘贴 / 列宽 / 保存
- `verify-crud.mjs`：右键菜单 / 新建 Group·Table / 重命名 / 复制粘贴 / 删除 / 删除行
- `verify-project.mjs`：模板列表 / 新建空项目 / 模板项目 / 项目设置 / 保存
- `verify-rules.mjs`：规则编辑器打开 / 列规则 / 表级规则 / 系统规则只读 / 丢弃确认
- `verify-settings.mjs`：分隔符 25 项 / 导出设置 / 校验策略 / 回填 / 保存落盘
- `verify-header-ref.mjs`：表头 export·type 下拉 / 自定义类型 / 引用显示开关
- `verify-global-export.mjs`：全局设置默认值·回填 / 数据选择器单击 / JSON 导出
- `verify-io.mjs`：Schema 导出·导入 / Excel 导出·文件列表 / 映射导入 / 问题跳转 / 关于 / 条件构建器
- `verify-fixes.mjs`：真实鼠标拖动选区 / 单值·列表引用选择器 / 拼音搜索 / 公式栏多行
- `verify-ui-polish.mjs`：新建项目来源切换复位 / 多列横向滚动与列字母 / 拖选半透明样式
- `verify-ref-picker.mjs`：枚举 / 表引用的表格展示（智能·全部列）/ 搜索 / 单击双击提交 / 列表多选
- `verify-excel-sync.mjs`：Excel 会话建立 / mtime 轮询 / 自动回写 / 结束会话

每个脚本都会改动工作区数据（编辑单元格、导入节点等），**请各自使用独立的新工作区**，不要在同一个工作区连续跑多个脚本。
`verify-rules.mjs` / `verify-io.mjs` 需要带 `hero/HeroBase`、`building/Building` 的工作区，用 `crates/app-tauri/scripts/fixtures/demo.tblschema` 建：`tablet-cli -w <dir> project new --from-file .../demo.tblschema --id demo`。

截图默认写入 `target/tauri-shots/`。浏览器里也可以直接打开 `ui/index.html` 调版式，但此时没有 Rust 侧数据。
