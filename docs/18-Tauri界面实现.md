# Tauri 界面实现

> **状态：进行中（2026-11）。** 本文记录 Tauri 版 GUI 的实现约定与当前进度。
> 双 GUI 布局 / 打包 / 崩溃日志见 @17；界面需求与交互语义见 @04。
> **核心约定：从 Slint 版反推「用户需要什么」，再用 Tauri/Web 的方式实现；不照抄 Slint 的实现机制。**
> **分层约定：core ← GUI / CLI 平级。GUI 业务（表格、Excel、Schema、导出、规则）直接调 core，
> 不调 `tablet-cli` 的 action 层；`tablet-cli` 只负责 CLI 参数解析与命令执行，GUI 仅在 CLI 模式下转发给它。**

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
| TreeSection | 项目 → 组 → 节点三层；展开/收起、搜索、过滤、完整组、显示 ID、新建项目、排序 | 宽 240px（可拖，最小 220），节点列表独立滚动；“完整组”忽略搜索/过滤展示全部组和节点（只隐藏已删除）；“显示 ID”切换项目行显示元数据 ID / 显示名称，与全局设置里的同一项共用 |
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
| 大表（>500 行） | 只渲染可视窗口 ±10 行，上下用 spacer 撑滚动高度；滚动时 rAF 换窗口 | 10 万行 × 20 列：DOM 约 800 个 td，不再生成 200 万个 |
| 纵向滚动对齐整行 | `snapScrollToRow()`：滚动位置取整到行高，虚拟窗口从第一整行开始（起点留 1px 容差，吸收 DPI 带来的小数 `scrollTop`）；末尾补不足一行的空白 | 像素滚动会在 sticky 表头下露出半行（数字被切一半）或留空白带，看起来像表头重叠；`ceil` 不带容差时会对齐位置多算一整行 |
| 同节点快照刷新不跳视口 | `setData` 保存并恢复 `scrollTop/scrollLeft`；`render()` 不再用 `getBoundingClientRect()` 量表头高（按 `--row-h` 行数计算） | 粘贴 / 插入删除行 / 撤销走完整 Snapshot 时，清空 tbody 的瞬间布局会把 `scrollTop` 钳到 0，视口弹回第一行 |
| 双击编辑 | 点选时 `scrollIntoView` 只在滚动位置变化或目标行不在窗口内才重建 tbody | 每次点选都重建会把 mouseup 的 `td` 换掉，浏览器不再派发 `click`/`dblclick`，双击编辑失效 |
| 行号宽度 | 按最大行号的位数自适应（如 10 万行 → 6 位数字，62px） | 不再固定 32px |
| 单格选中 / Shift 扩选 / 拖拽框选 | `anchor + focus` 选择模型；拖拽用 `pointerdown/move/up` | 拖到边缘自动滚动（rAF） |
| 整行 / 整列 / 全选 | 点行号、列字母、左上角 | |
| 键盘导航 | 方向键、Shift+方向、Tab、Home/End、Ctrl+Home/End、PageUp/Down | 选中后自动 `scrollIntoView` |
| 编辑 | 双击 / F2 / 直接打字进入；`input` 覆盖单元格 | Enter 提交下移、Tab 右移、Esc 取消、失焦提交 |
| 公式栏 | 与当前单元格双向联动；Enter 提交并下移 | 只读列禁止编辑 |
| 复制 / 剪切 / 粘贴 / 删除 | `copy` / `cut` / `paste` 事件 + TSV；Delete 清空选区；单元格按 Excel 规则处理引号（含 `\t` / 换行 / `"` 的值加 `"..."`，`""` 转义），多行单元格不会拆成多行 | 不手写系统剪贴板通道 |
| 列宽 | 列字母右侧 6px 热区拖拽；双击自适应 | 手动拖拽会关闭「自动列宽」 |
| 列字母 | Excel 式编号：A…Z、AA、AB… | 与 Slint 版 `col_letter` 一致 |
| 横向滚动 | 表格区滚动条常驻（覆盖 WebView2 的自动隐藏）；插入列后自动把新列滚进可视区 | 否则列一多就看不出横向滚动入口、新列停在右边界外 |
| 错误单元格 | 红色内描边 | 与 @04 的错误展示一致 |
| 拖选性能 | 选择变化只切 class，不重建 DOM | 避免每次 pointermove 重绘整表 |
| 只读列 | `id` 等列灰底、不可编辑 | 与 @04 的 ReadOnly 语义一致 |

数值列按类型行右对齐（`int/long/float/double`），文本列左对齐；这是 Web 表格的常规做法，Slint 版没有，属于实现层改进。

### 3.1 命名规则由 core 强制

`NewGroup` / `NewTable` / `NewConstant` / `NewEnum` / `RenameGroup` / `RenameNode` 统一走
`ProjectEngine::validate_action()`：

- 组名：中英文 / 数字 / 下划线，**项目内忽略大小写唯一**；
- 节点名：PascalCase，**项目内跨组、跨 Table/Constant/Enum 忽略大小写唯一**；
- 重命名排除自身。

校验失败不落地、不记撤销，`execute_action` 返回错误由 UI 展示。Tauri 的输入弹窗通过
`validate_name` 调同一个校验做实时提示（确定按钮禁用），但真正的拦截在 core——UI 只负责显示，
不能绕过或另写一套规则。组名/节点名重复的旧协作场景（外部文件、旧数据）由保存前的校验兜底拒绝。

## 4. 当前进度

已完成：

- Toolbar / TreeSection / GridSection / BottomPanel + 两处分隔线
- 树：三层结构、展开收起、搜索、过滤、节点选中
- 表格：固定表头/行号、单格/矩形/整行/整列选择、键盘、内联编辑、表头编辑、TSV 剪贴板、列宽拖拽与自适应
- 公式栏、状态栏、日志过滤与 tab 切换
- **真实数据**：Tauri command 直连 `tablet_core::ops::ProjectEngine`（项目树、表格投影、保存/撤销/重做/重新加载/验证）
- **右键菜单**：项目 / 组 / 节点 / 表格单元格、行号、列字母；项目 / 组 / 节点的动作按用途分组，导出 / 导入 / 新建 / 粘贴用子菜单；三级都提供「打开所在目录」。
- **节点增删改**：新建 Group（tbl / xlsx）、新建 Table·Constant·Enum、重命名、复制/粘贴、软删除、还原到已保存
- **表格行列操作**：右键插入/删除行、插入/删除列
- **新建项目**：空项目 / 内置·本地模板 / `.tblschema` 文件三种来源；新建后进入内存态（Pending），保存时落盘
- **项目设置**：身份（项目 ID 只读、名称、分类、版本）、分隔符（34 项 + 恢复默认）、导出（编码/换行/JSON·XML 空值、BIN 整包/按节点拆分开关、服务端数据目录与各语言包名·输出目录·自定义区间、客户端 Lua/GDScript/TS/Unity/Godot 输出）、校验策略（error/warn/info → block/report/ignore）；保存后自动重算校验
- **校验规则编辑器**（按 rule-builder-v2 / Slint 版重做）：列规则 / 表级规则；左侧列清单（规则条数）；规则卡片树（每一行是一个函数——验证器或条件函数；组；行尾 and·or；+行 / +组 / 分散 / 合并）；参数与说明/示例在底部 inspector（选中行后编辑，函数下拉同时含验证器与条件函数）；还原 / 保存；有未保存改动时切换列 / 作用域先显示「丢弃并切换 / 取消」确认条（同 Slint）；切到系统规则只是切视图、不动草稿；未保存关闭时丢弃确认
- **表头选择器**：`export` 单击弹出自定义下拉；`type` 单击弹出分组菜单（基础 / 容器 / 引用目标 / 自定义输入），不再依赖原生 `<select>`
- **引用显示**：显示引用名称总闸；被引用展示列（表默认原始 id，枚举默认 name，下拉只列可选辅助列，id 由「ID」默认项表示）；单值、List/Set、Map（键 / 值引用）及其复合变体按项目分隔符逐元素翻译
- **全局设置**：界面（表头/数据选择器单击·双击、失焦提交、实时校验、项目列表显示 ID、Constant 允许 @Xxx）、日志级别、默认分隔符（34 项）；写入 `tablet.toml`
- **导出执行**：JSON/XML/BIN（整包或按节点拆分）+ Java/Go/C++/C#(.NET·Unity·Godot)/Lua/GDScript/TS(客户端·服务端) 勾选导出；预设含「全部 / 数据 / 二进制 / 后端 / 前端」，输出路径取项目导出配置
- **Schema 导入导出**：导出整项目 / 按分组·节点过滤 / 可选预设；导入 `.tblschema` 并合并（新增 / 覆盖统计）
- **Excel**：导出对话框跨组勾选、只生成一本 `<project>-excel.xlsx`；导入对话框逐 sheet 选择目标组 + 目标节点名，左侧树预览各组的覆盖 / 新增，支持导入为新组；导入是一个撤销步骤、只进内存，保存才落盘；`.excel/` 文件列表与打开目录、「用 Excel 打开」、任意 Excel 映射导入（sheet / 字段行 / 数据起始行 / 自动字段建议）；xlsx 组里不符合 tablet 规范的新增列按**普通列**展示（底色区分、可编辑、不导出；改成合法表头 / 类型 / export 后自动升级为 TBL 列，树上标「普」）。
- **Univer / Excel 回写**：按 workbook 表头重建列结构；在 Univer / Excel 里新增的列会带回 tablet——合规列成为 TBL 列，不合规列成为普通列，不再因为多一列而整次拒绝。快速编辑会话可「结束」（自动回写内存）或「放弃改动」；xlsx 组每 2s 检测外部修改，干净时自动重载，本地有脏改动时提示并跳过。
- **视图偏好**：列宽 / 自动列宽按 (project, group, kind, node) 持久化到 `.data/user-data.json`，与项目 dirty 无关；项目排序方式写入 `tablet.toml`，跨启动保留。
- **全局设置生效**：失焦提交、实时校验、项目列表显示元数据 ID 都已接入实际行为；「应用模板」使用全局分隔符 / 导出模板，新建项目继承全局分隔符。
- **锁与生命周期**：Univer / Excel 编辑中的组在主界面整块置灰；项目级操作（撤销/重做、规则保存、Schema 合并、重新加载、关闭/删除项目、映射导入）在后端也会拒绝；关闭有脏改动的项目 / 主窗口会先确认。
- **项目集合持久化**：打开 / 关闭 / 删除项目后，把当前 `opened_projects` 与 `last_project` 写回 `tablet.toml`；「重新加载」按该集合恢复全部已打开项目，不会只剩 last_project。关闭项目时若选中节点属于该项目，选中会清空（**不会**再通过 select_node 把项目自动重新打开）；项目保持关闭，等用户重新打开。
- **Univer 编辑（Tauri 专属）**：网格 Ribbon「Univer 编辑」为当前组打开独立窗口；**一个窗口 = 一个 Group 工作簿**（sheet = 该组 Table/Constant/Enum），可同时打开多个窗口并排查看 / 比对；sheet 切换条自绘、只允许切换（不提供增删 / 重命名）。可回写 / 重新加载；回写只进内存，主界面点「保存」才落盘。编辑中的组在主界面整块 GridSection 置灰只读，树上的组操作 / 粘贴 / 新建节点 / 删除重命名也被拦截，避免两边同时改。
- **问题面板**：校验错误 / 警告列表，点击跳转到对应节点与单元格；「重新验证」与工具栏「验证」走同一命令——重算全部已打开 Project，并把每条明细（含 `[验证]` / `[警告]` 定位文本）按级别写进 UI 日志与 `tablet.log`；再次验证前先移除上一块验证日志，面板不堆叠重复明细（日志文件保持追加历史）
- **关于**：版本 / 目标平台 / 工作区 / 版本库；检查更新打开 GitHub
- **操作 Toast**：保存、导出、导入、校验结果与错误提示
- 树排序下拉已接通（ID / 名称 / 已打开 / 创建时间 / 手动；创建时间无字段时退回名称）
- 统一输入弹窗与确认弹窗

## 4.2 与 Slint 界面对照（逐界面）

| Slint 界面 / 组件 | Tauri 版状态 |
|---|---|
| Toolbar（撤销/重做/保存/重新加载/当前项目+脏标记） | 已对齐；额外增加 验证 / 导出 / 设置 / 关于 |
| TreeSection（标题 / 新建+排序 / 搜索+过滤+完整组+显示 ID / 节点列表） | 已对齐；搜索支持拼音首字母；排序下拉已接通 |
| GridHeaderBar（标题、行列数、自动列宽） | 已对齐 |
| GridRibbon（还原 / Excel / 校验规则 / 显示引用名称 / 被引用展示列） | 已对齐；额外增加「Univer 编辑」 |
| FormulaBar（折叠单行 / 展开多行 / 只读标记） | 已对齐（textarea 多行，只读显示「只读」） |
| GridArea（选择、框选、键盘、编辑、表头类型·导出下拉、引用选择器、列宽、右键） | 已对齐；单值 / 列表引用都有选择器；类型走「选择类型」对话框，export 是自绘下拉 |
| StatusBar（选区 / 光标 / Excel·Univer 会话 / 单元格错误原因） | 已对齐：单格有校验问题时追加 `⚠ 原因`（即时重算，不受实时校验开关影响）；Univer 编辑中的组显示只读提示 |
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
| 任意 Excel 映射导入 | 已重做：左侧 Sheet 轨道（状态 / 行数） + 节点类型（表 / 常量 / 枚举，可手动改） + 表头行 / 数据起始行 + 可选说明 / 类型 / 导出辅助行 + 源列→目标列映射（逐行实时校验） + 源表 / 导入结果 / 检查三页签 + 导入当前 / 导入全部就绪；自动识别节点类型与辅助行，识别不到的行号为 0 |
| 引用选择器 | 已对齐 Slint 版：id + 辅助展示列的表格（表头两行：说明 + 字段名）；表可切「智能 / 全部」列展示（默认取全局 `[ui.ref_picker]`）；当前值可直接输入；搜索 id / 任意列；单值单击选中、双击或「确定」提交，列表引用多选；列表引用的多值分隔符跟随项目 `[separators]`（`List<@X>` 用 `list`、`Set<@X>` 用 `set`），选择器拼值与引用名展示均按该分隔符拆分 / 连接；「清空 / 取消 / 确定」 |
| 类型选择器 | 已对齐：独立对话框（数据类型 / 引用类型双 tab）；范式列表 + 参数槽 + `结果 / 示例 / Java / Go / Lua` 预览；constant 按全局开关禁用引用 tab |
| 规则编辑器 | 已按 rule-builder-v2 / Slint 版重做：规则卡片树，**每一行就是一个函数**（验证器函数或条件函数 eq/ne/in/match…），行尾 and·or；`＋行` 新增函数行、`＋组` 新增组；底部 inspector 统一「更换函数 + 参数 + 说明/示例」。条件行与同组验证器由保存时编译成 `when(条件, 验证器)`（与 Slint 相同，树里不出现 when 节点） |
| Excel 实时编辑 | 已实现（与 Slint 同一模型）：每次拉起导出**独立副本** `.tbl-cache/<组>-<毫秒>.xlsx` → 用 Excel 打开 → 2s 轮询 mtime → 自动导入回内存（**只改内存，不落盘**）；重拉不受 Excel 对上一份副本的文件锁影响；状态栏显示编辑中，右键可「结束 Excel 编辑」或「放弃 Excel 改动」；会话结束删副本，启动时兜底清理残留 |
| Xlsx 组外部修改 | 已对齐 Slint：2s 轮询工作簿 stamp；干净组自动重载，脏组提示一次并跳过；Univer / Excel 会话中的组不自动重载 |
| Excel 同步面板 | 已实现：读取 `core::excel_sync::plan_group`，逐 (组, 节点) 显示两侧行列数 / desc 差异 / 表头越权提示 / 可用操作（数据→、补列→、强制→、←数据、创建→、←导入）；点按钮前 core 会重算计划，过期操作直接拒绝 |
| 粘贴 Group（冲突命名） | 已对齐：先只读预览，有重名才弹「粘贴 Group」对话框，组名 + 每个子项可改（预填 `_copy`）；无冲突直接粘贴 |
| 克隆 / 关闭 / 删除项目 | 已对齐 |
| 行颜色标记 | 无数据来源：`core` 不解析 `#@color`，Slint 版也渲染为透明（两边一致） |
| 窗口标题（版本 + 分支） | 已对齐（Rust 侧设置原生标题） |

## 4.1 已知限制（非缺失菜单）

- **行颜色标记**：`core` 不把 `#@color` 解析进数据模型（Slint 版同样渲染为透明），没有数据来源可展示；不属于 Tauri 版与 Slint 版的差异。
- **Excel 实时编辑的实现方式**：用 mtime 轮询（2s）而不是 Windows 文件句柄监控；Excel 保存后最多 2 秒回写。Excel 未保存时不会读到半成品。临时副本写在 `.tbl-cache/`（不出现在 `.excel/` 文件列表与同步面板里）；结束会话时删除，Excel 仍持锁导致删不掉时留到下次启动清理，不影响再次拉起（每次都是新文件名）。
- **项目排序**：「创建时间」在 UI 树数据里没有字段，退回按名称；排序方式已持久化到 `tablet.toml`，但「手动」与 Slint 一样没有拖拽重排入口。
- **Univer 编辑**：只用 Apache-2.0 的 `@univerjs/presets` / `preset-sheets-core`，xlsx 依赖通过 SheetJS + 现有 xlsx 桥接往返，不接 Univer Pro；样式 / 批注 / 合并等布局不持久化。一个窗口只放一个 Group（多开窗口 = 多组并排比对）；Univer 自带 sheet bar 关闭，窗口顶部的 sheet 条只允许切换，不提供增删 / 重命名。表头与 sheet 名由 `import_xlsx_into_group` 严格校验，改坏会整次拒绝。回写前比对 Group 指纹，主界面若已改过同组会要求重新加载。
- **Univer 前端资源**：首次引入 Node/Vite 工具链，产物 `ui/univer/` 不提交；`tauri build` 由 `beforeBuildCommand` 自动构建，日常 `cargo build` 前需手动执行
  `cd crates/app-tauri/univer-editor && npm ci && npm run build`（未构建时按钮会提示）。

## 4.3 性能实测（100k 行 × 20 列）

环境：Windows 11，release（`x86_64-pc-windows-msvc`，fat LTO），20 列混合类型（int/str/float/txt/bool/`@Enum`/`List<int>`/`Map<str,int>`），100,000 行。

| 项目 | 优化前 | 优化后 |
|---|---|---|
| 打开表格（选节点 → 首屏可交互） | 36.3 s | **1.21 s**（10k 143 ms / 50k 575 ms） |
| DOM 单元格数 | 2,000,100 | **约 780**（40 个 tr：可视行 + 上下 spacer） |
| JS heap | 128 MB | 46 MB |
| 进程 RSS | ~290 MB | ~290 MB（数据本身在 Rust/JS 内存里） |
| 滚动换窗口 | — | 21–28 ms |
| 单元格编辑提交 | — | **285 ms 后端 + 1 ms 前端**（轻量 EditResult，不含 grid.rows；UI 端到端约 0.5 s） |
| 保存工作区 / 全量校验 | — | 1.58 s / 1.19 s |
| CLI `validate` / `export data --json` | — | 约 1.0 s / 1.2 s |

- 行数 >500 自动进入虚拟滚动；行号栏宽度按位数计算（100k 行显示 6 位数字，62px）。
- 单元格提交走 `commit_cell_fast`：只回状态 / 树 / 错误定位，前端本地已改过单元格，不再重建 10 万行 grid；仅结构操作 / 撤销 / 保存仍走完整 Snapshot。
- 单元格 dirty 计算改成 O(1)（只有改回原值时才退回全表比较），避免每次编辑扫 200 万字符串。

## 4.4 Excel 导入 / 导出

入口位置：

- **Project 根**：`导出 ▸ 导出 Excel…` / `导入 ▸ 导入 Excel（多组）…`，默认范围是整个项目（导出默认全选所有组节点）。
- **Group**：`Excel ▸ 导出 Excel… / 导入 Excel…`；导出默认只预选该组节点，仍可勾选其它组；导入打开同一工作簿对话框。
- **节点**：只有 `导出 Excel…`，默认只预选该节点。导入是「工作簿 → 多个组/节点」的操作，不提供节点级入口。

入口决定模式：从「导出」进入的对话框只显示导出，从「导入」进入只显示导入，不再在弹窗里切换。

- **导出：多组勾选，只输出一个 xlsx**。左侧显示当前项目的全部组与组内 Table/Constant/Enum，支持跨组勾选；右侧预览同一本 `<project>-excel.xlsx` 将包含的 sheet。sheet 名 = 节点名（项目内唯一），所以不同组的 sheet 可以放进同一本工作簿；文件不记录组归属。
- **导入：一本工作簿可以导入多个组**。左侧是**变更预览**（只读，只统计右侧已勾选行）：组行显示 `覆盖 N / 新增 M`，被覆盖节点标「覆盖」，新目标名以虚拟行显示「新增」。右侧每一行源 sheet 自己选择**目标组**（现有组 / `＋ 新建组…`）和**目标节点名**：
  - 同名 → 覆盖现有节点；
  - 新名字 → 作为新节点导入；
  - 清空或取消勾选 → 跳过。
- 目标组默认按 sheet 名自动推断：项目里唯一存在同名节点的组；找不到唯一匹配时保持「未选择组」。源表独有节点默认不勾选。
- 选「新建组…」时使用统一的新组名输入（默认 `imported_group`），新组按 **Tbl 组**创建；组已存在会报错，不会静默覆盖。
- **导入是一个撤销步骤**（`undo_label = Excel 导入`）：所有 sheet 的解析 / 校验先完成，任一行失败则整次不生效；成功后在同一个 `edit_project` 事务里应用，撤销一次即可整体退回。
- 导入只进内存，主界面保存后才落盘。目标组是 **xlsx 组**时按内存模型更新节点（新节点标记未保存），保存时由 xlsx 保存流程写入/新建 sheet；目标组是 **Tbl 组**时写入 `.tbl`。字段按表头回写，新增列按 xlsx 组的现有规则追加。
- 项目根入口只是当前项目范围内的多组导出 / 导入，不涉及 `.tblschema`；任意布局的列映射仍走「导入任意 Excel（映射）…」。不一次导入多个文件。

```text
导出：
左：当前项目全部组 + 组内节点（组复选框全选，可跨组勾选）
右：<project>-excel.xlsx 的 sheet 预览（组 / sheet / 类型）
导入：
左：变更预览（覆盖 N / 新增 M；新增节点为虚拟行）
右：源 sheet → 目标组（现有组 / 新建组）→ 目标节点名（逐行可改）
底部：导出 N 个 sheet / 1 个文件；导入 N 个 sheet → K 个组（覆盖 A，新增 B）
```

## 5. 开发 / 调试 / AI 驱动

Tauri 版把 WebView2 的 CDP 端口作为一等调试入口。相比 Slint 版只能靠 `PrintWindow` 截屏 + `PostMessage` 点击，这套能力可以直接读页面状态、模拟操作、截屏，适合 AI 驱动回归。

### 5.1 启动

首次运行前先构建 Univer 编辑器资源（产物在 `ui/univer/`，编译期内嵌）：

```bash
cd crates/app-tauri/univer-editor
npm ci
npm run build
```

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
- `verify-settings.mjs`：分隔符 34 项 / 导出设置 / 校验策略 / 回填 / 保存落盘
- `verify-header-ref.mjs`：表头 export·type 下拉 / 自定义类型 / 引用显示开关
- `verify-global-export.mjs`：全局设置默认值·回填 / 数据选择器单击 / JSON 导出
- `verify-io.mjs`：Schema 导出·导入 / Excel 导出·文件列表 / 映射导入 / 问题跳转 / 关于 / 条件构建器
- `verify-fixes.mjs`：真实鼠标拖动选区 / 单值·列表引用选择器 / 拼音搜索 / 公式栏多行
- `verify-ui-polish.mjs`：新建项目来源切换复位 / 多列横向滚动与列字母 / 拖选半透明样式
- `verify-ref-picker.mjs`：枚举 / 表引用的表格展示（智能·全部列）/ 搜索 / 单击双击提交 / 列表多选
- `verify-ref-sep.mjs`：项目列表分隔符改为 `#` 后的选择器拼值 / 拆分 / 引用名展示 / 还原
- `verify-paste.mjs`：多行单元格（含 HTML 标签 / 引号）粘贴不拆行、按纯文本渲染、复制往返一致
- `verify-validate-log.mjs`：主动验证明细进 UI 日志 / 重复验证先清旧块 / 日志文件追加（`WORKDIR` 可选）
- `verify-excel-sync.mjs`：Excel 会话建立 / mtime 轮询 / 自动回写 / 结束会话
- `verify-univer.mjs`：Univer 窗口打开 / 每组一个窗口 / 编辑回写内存 / 主界面置灰只读 / 保存落盘（需先构建 `ui/univer/`；用 `WORKDIR=<工作区绝对路径>` 指明测试工作区）
- `verify-audit.mjs`：列宽持久化 / 显示值与原值分离 / 保存失败提示 / 全局设置生效 / 导出预设（需 `WORKDIR` 与含 `hero/HeroBase` 的 full fixture）

每个脚本都会改动工作区数据（编辑单元格、导入节点等），**请各自使用独立的新工作区**，不要在同一个工作区连续跑多个脚本。
`verify-rules.mjs` / `verify-io.mjs` 需要带 `hero/HeroBase`、`building/Building` 的工作区，用 `crates/app-tauri/scripts/fixtures/demo.tblschema` 建：`tablet-cli -w <dir> project new --from-file .../demo.tblschema --id demo`。
`verify-global-export.mjs` / `verify-ui-polish.mjs` / `verify-audit.mjs` 依赖 `hero/Hero`、`global/GameConst` 等节点，用 `crates/app-tauri/scripts/fixtures/full.tblschema` 建工作区。

截图默认写入 `target/tauri-shots/`。浏览器里也可以直接打开 `ui/index.html` 调版式，但此时没有 Rust 侧数据。
