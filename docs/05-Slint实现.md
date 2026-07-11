# Slint 实现

桌面 GUI（产物 `tablet`）的 slint 实现细节：架构 / 主题 / 编辑态 / 失焦保存 / 滚动同步 / 闪退坑。CLI 实现见 @05；UI 设计与交互见 @06；tbl 系统硬性规则见 @01。

## 1. 架构

项目采用 core + 前端架构：

```
tablet/
├── crates/
│   ├── core/         ← tablet-core：零 UI 依赖的核心库
│   ├── app-slint/    ← Slint GUI（产物 tablet）
│   └── app-cli/      ← 命令行工具（产物 tablet-cli）
```

`tablet-core` 持有模型、类型系统、验证、数据操作、文件 I/O；`tablet` 与 `tablet-cli` 都只是它的薄前端。UI 设计和布局见 @06，本篇专注 slint 实现细节。源码组织见 @03.1；启动分流见 @03.12。

> 历史：项目早期为对比 immediate-mode / retained / 声明式三套 GUI 范式，曾并存 `app-egui` / `app-fltk` 两个实验性实现。结论是 slint 在主题完成度、声明式响应式布局、Live Preview 调试体验上综合最好，已向正式版本靠拢；其余两份实验实现已从仓库移除。

## 2. 大区拉伸 splitter

slint 没有内置 splitter，手写 4px `Rectangle` + `TouchArea(mouse-cursor: ew-resize/ns-resize)` + `pressed-x/y` 累加 delta：

```slint
// 横向 splitter：tree ↔ grid
Rectangle {
    width: 4px;
    background: Theme.border-light;
    TouchArea {
        mouse-cursor: ew-resize;
        moved => {
            if (self.pressed) {
                // self.mouse-x 是 splitter 内坐标，每帧累加 delta
                root.tree-w = max(220px, min(root.tree-w + (self.mouse-x - self.pressed-x), 800px));
            }
        }
    }
}
```

要点：

- `mouse-x / mouse-y` 是 splitter 自身内坐标，光标移动时 splitter 也跟着跑——靠 `pressed-x / pressed-y` 取按下瞬间的相对位置，每帧累加 `mouse-x - pressed-x` 才是稳定 delta；不能直接把 `mouse-x` 当绝对坐标用
- splitter 视觉宽度 4px：3px 太窄不好抓，6px 视觉粗壮；mouse-cursor 设 `ew-resize / ns-resize` 让用户知道可拖
- 上下/左右 clamp 必须显式写：min 防止挤没搜索框，max 防止把另一侧拖到 0
- TreeSection 内层有 border + padding 双层堆叠，下限 220px 才能给搜索框留同样视觉留白

## 3. 主题（widget style）

slint 的 widget style 决定 `LineEdit` `ComboBox` `Button` `CheckBox` `ScrollView` `Slider` `SpinBox` `TabWidget` `StandardListView` 等 std-widgets 的视觉风格。

| style | 模仿 | 何时选 |
|-------|------|-------|
| `fluent` | Windows 11 Fluent | Windows 桌面 |
| `cupertino` | macOS / iOS HIG | macOS 桌面 |
| `cosmic` | System76 COSMIC | Linux 桌面 |
| `material` | Google Material Design | Android 风、跨平台桌面 |
| `native` | 委托 Qt 渲染（需要 `backend-qt` feature + 用户机器装 Qt） | 不推荐发行 |

亮/暗变体：`fluent-light` / `fluent-dark` / `material-light` / `material-dark`，其它跟随系统。

**关键约束：style 是编译期常量，运行时不能切。** 想要"运行时换主题"只能通过项目自定义 `theme.slint` 单例的颜色属性 — 仅对自绘组件有效，std-widgets 的内部颜色由编译期固定的 style 决定。

slint-build 默认按"编译机器"的 OS 推断 style。跨平台编译时（如在 Windows 上编 Linux/macOS 包）会选错，必须按 **目标平台** 显式指定。本项目 `crates/app-slint/build.rs` 读取 `CARGO_CFG_TARGET_OS` 映射到匹配 style：

```rust
let style = std::env::var("SLINT_STYLE").ok().unwrap_or_else(|| {
    match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("windows") => "fluent".to_string(),
        Ok("macos") | Ok("ios") => "cupertino".to_string(),
        _ => "cosmic".to_string(),
    }
});
slint_build::compile_with_config(
    "ui/app.slint",
    slint_build::CompilerConfiguration::new().with_style(style),
)?;
```

环境变量 `SLINT_STYLE=xxx cargo build ...` 仍可显式覆盖。

## 4. retained model 推送策略

slint 是 retained 渲染：UI 不会跟着 Rust 状态自动重画，需要 Rust 主动调 `set_*` 写属性、`set_*` 写 model 才会刷新。

为避免每次操作都重建整个表格 model（开销大、还会断开 slint 内部的 click sequence），分两个粒度推送：

| 函数 | 重建项 | 适用场景 |
|------|--------|---------|
| `push_grid` | header_rows / data_rows model + 全部选区 / 编辑 / 公式栏属性 | 切换节点、写回单元格、进入/退出编辑、view 开关 |
| `push_selection_only` | 仅选区 / coord / formula_display / formula_editable / status / hover 属性 | cell-clicked / row-num / col-letter — 不动 model，避免重建 cell 元素打断 click sequence |

⚠️ **不要在回调里同步 push_grid 后又依赖触发回调的 TouchArea 继续存在** — push_grid 重建 model 会销毁触发当前回调的元素。但实测 cell 的 if 切换 LineEdit / TouchArea 在 slint 1.16 上是稳定的（cell 双击进编辑就是这个流程），原因是回调链结束后 slint 才会真正释放旧元素。

## 5. 编辑态：cell 与 公式栏 共享 buffer

仿 Excel 编辑体验：cell 双击 / 公式栏点击都进入编辑，状态共享一份 `editing-buffer`（slint 端 `in-out property <string>`，与两处 LineEdit 双向绑定）。Rust 端 `AppState` 持有：

```rust
pub editing: Option<(usize, usize)>,
pub editing_buffer: String,        // 进入编辑时的初始 raw 值
pub editing_in_formula: bool,      // 当前编辑器在公式栏（true）还是单元格内联（false）
```

`editing_in_formula` 让两处 LineEdit 互斥渲染（避免同时存在抢 focus）：cell LineEdit 仅在 `editing-row==r && editing-col==c && !editing_in_formula` 时渲染；公式栏 LineEdit 仅在 `editing && editing_in_formula` 时渲染。

`editing_buffer` 在 Rust 端只是初始 raw，**用户输入只通过双向绑定写到 slint 端 property**，没回流到 Rust。所以 commit 时必须读 slint 端的 `ui.get_editing_buffer()`，而不是 Rust 端的 `st.editing_buffer.clone()`。

进入编辑时调 `LineEdit.select-all()` 让文本全选 — 跟 Excel 双击进入编辑后直接输入覆盖原值的体验一致。

## 6. 全局失焦保存退出（Excel 风格）

slint 的 `LineEdit` 焦点不会被 `TouchArea.clicked` 自动剥夺，必须显式调 `FocusScope.focus()` 或调另一个可获焦元素的 `focus()` 把焦点拉走。

实现方式：AppWindow 根部声明一个隐藏 `focus-sink := FocusScope { width:0; height:0 }`，配合两个 callback 联动：

```slint
callback drop-focus();
callback commit-pending-edit();   // Rust 监听
drop-focus => {
    focus-sink.focus();            // 抢走 LineEdit 焦点
    root.commit-pending-edit();    // 通知 Rust 把 buffer 写回
}
```

凡是用户"应该退出编辑"的点击（toolbar 按钮、tree-node、cell-clicked、row-num、col-letter、grid blank、tree-section blank、ribbon 等），都在 callback 入口先调 `root.drop-focus()`。Rust 端在 `wire_focus()` 里订阅 `commit-pending-edit`，调 `commit_editing` + `push_grid`。

⚠️ **未挂 drop-focus 的容器是 bug**：之前 tree_section 的"空白区右键 TouchArea"只处理 `pointer-event` 右键，左键 click 被它吃掉但没冒泡，导致点击树结构下半空白时编辑不退出。修法是给所有空白容器的 TouchArea 都加 `clicked => root.blank-clicked()` 链回 AppWindow 触发 `drop-focus`。**不要依赖 AppWindow 根部的"全屏 blank-catcher TouchArea"** — 它会被前景的 VerticalLayout 等同尺寸节点遮挡，永远不会触发。

## 7. 输入组件选型

| 组件 | 出处 | 用法 |
|------|------|------|
| `TextInput` | 原生基础元素，无装饰 | 想完全自画输入框 / 严格按父尺寸渲染（如公式栏 24px 高度撑不下 LineEdit）|
| `LineEdit` | std-widgets 复合组件，自带 padding / min-height / 焦点态边框 / placeholder / X 清除图标 | 默认选 — 跨 style 视觉一致，开箱即用 |

LineEdit 在 fluent style 下自带的 X 清除图标和焦点蓝色下划线**就是搜索框该有的 affordance**，不需要绕开。

slint 标准库**没有专门的 SearchInput 组件**，"带 X 清除"的搜索风格需要自己用 `TextInput + Rectangle + Text(图标) + TouchArea` 拼装。

## 8. 闪退坑总结

slint 1.16 在重建动态可见元素（`if` 切换、model 重建）时，**正在执行回调的元素若被销毁，会触发 use-after-free 闪退**。已踩到的两个具体场景：

1. **公式栏 idle 态嵌套 TouchArea 在 `if !editing` 里**：clicked 调 request-edit → push_grid 设 editing=true → 外层 if 销毁，连同正在执行的 TouchArea 一起 drop。**修法：把 TouchArea 提到顶层永久存在，靠 `enabled` 切换响应 — 不嵌套在动态 `if` 内**。
2. **LineEdit `changed has-focus` 同步 commit + push_grid**：失焦瞬间 LineEdit 销毁路径上的事件再触发模型重建，反复重入借用冲突。**之前的修法是去掉 `changed has-focus`，靠点击路径上的显式 commit 触发**。

## 9. profile 策略

agent / 日常开发走 dev，最终交付 / 性能验证走 release。两套 profile 目标完全相反：

```toml
[profile.dev]
# 编译速度优先（agent 默认走 dev，频繁 cargo check / build）
opt-level = 0
debug = "line-tables-only"   # 只留行号信息，砍掉变量/类型 debuginfo，链接显著变快
incremental = true
codegen-units = 256

[profile.release]
# 性能优先，体积次之（最终交付 / 性能验证用）
opt-level = 3
lto = "fat"
codegen-units = 1
strip = true
```

权衡：

- **dev**：opt-0 + 行号 debuginfo + 大 codegen-units + 增量编译 → 增量改一行 .rs/.slint 通常秒级；运行慢可接受（本项目主要瓶颈是用户操作而非渲染热路径）
- **release**：opt-3 + fat LTO + 单 codegen-unit → 跨 crate 内联，最强优化；一次完整编译 2-4 分钟，不开 incremental（fat LTO 跟增量不兼容）

slint-build 每次编译会通过 build.rs 重新生成 Rust 代码，这是 release 慢的主要原因，没法绕开。

## 10. ContextMenu 宽度自适应

slint 不像 Qt `QMenu` / Win32 `TrackPopupMenu` / WinUI `MenuFlyout` 那样默认按 item 文本自适应宽度——`Rectangle` 必须显式给 `width` 否则塌缩。早期 `width: 160px` 硬编码，"复制"/"删除"等短标签右侧大段空白，"新建Constant"等长标签又会被截断。

修法：去掉 `width`，靠 `VerticalLayout` 自然撑开，叠加 `min-width / max-width` 兜底；每个 item 的文字外包一层 `HorizontalLayout { padding-left/right }`，让宽度由内容驱动而不是父容器：

```slint
Rectangle {
    min-width: 140px;          // 防止单字标签太窄
    max-width: 280px;          // 防止超长标签把菜单撑满屏幕
    // 不写 width — VerticalLayout 自然撑开
    VerticalLayout {
        for item[i] in root.items: Rectangle {
            height: item.is-separator ? 7px : 28px;
            if !item.is-separator: HorizontalLayout {
                padding-left: 14px;
                padding-right: 14px;
                Text { text: item.label; ... }
            }
        }
    }
}
```

⚠️ slint 1.16 的 `padding` 简写**不支持** `padding: 4px 0px` 这种 CSS 风格双值写法，只接受 `padding: 4px`（四边相同）；要分别设置必须用 `padding-top / padding-bottom / padding-left / padding-right`。这一点和大多数 web/native UI 框架不一样，编译报错信息是 `expected ';'`，看不出来是 padding 写法的问题。

## 11. 表格滚动同步（GridArea / RefPicker）

slint 的 `ScrollView` 只能整体滚动，没法把"某列固定 / 某行固定"。表头钉住垂直方向 + 行号钉住水平方向，做法是把单一数据 ScrollView 当作真值源，header / 行号用 `clip: true` 容器 + 内层手动平移：

```slint
in-out property <length> scroll-x;
in-out property <length> scroll-y;

// header：clip + 内层 x 跟随
Rectangle {
    clip: true;
    Rectangle {
        x: root.scroll-x;
        width: total-col-w;
        // header rows...
    }
}

// 行号列：同样模式但绑 y
Rectangle {
    clip: true;
    Rectangle {
        y: root.scroll-y;
        height: total-rows-h;
        // row numbers...
    }
}

// 数据 ScrollView 持真值
ScrollView {
    viewport-x <=> root.scroll-x;
    viewport-y <=> root.scroll-y;
    // ...
}
```

要点：

- `viewport-x` 在向右滚时本身就是负值，内层 `x` 直接绑 `scroll-x` 即可同方向；**多取一次反就反了**（这条踩过两次）
- 不要试图嵌套两个 ScrollView 互相同步 `viewport-x` —— 1.16 版本下事件不会冒泡，互相绑也不工作
- 行号"水平不动"不能在 row 的 `HorizontalLayout` 里给 row-num 元素加 `x:` 抵消；layout 子元素禁止显式 `x/y`，必须把行号拆出 layout 单独 clip 容器
- 表头双行 / 单行的高度切换：用 property 计算 `has-desc ? row-h*2 : row-h`，clip 容器跟着变高即可，内部 layout 自适应

实现：`crates/app-slint/ui/components/grid_area.slint` + `crates/app-slint/ui/dialogs/ref_picker.slint`。

## 12. 跨平台启动约定

GUI（slint）与 CLI 都遵循同一套启动壳，平台差异通过 `cfg_attr` / `cfg!` 包起来，**不直接绑死任何平台**。

### 12.1 隐藏控制台窗口

```rust
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
```

- `windows_subsystem = "windows"` 是 Windows 专用属性：把可执行文件 PE header 的 subsystem 字段设为 GUI，双击 / `cargo run` 不再带终端窗口。
- 同时抑制部分图形栈在 stderr 输出的初始化日志（如 Mesa `vm3dgl: Failed to initialize svga driver. / Falling back to LLVMPipe.`）——这些日志在 GUI subsystem 下没有 stderr 句柄。
- Linux/macOS 没有 subsystem 概念，`cfg_attr(target_os = "windows", ...)` 让属性只在 Windows 编译时启用，其它平台无副作用。

### 12.2 进程存活检测（lock 文件防双开）

启动时写 `tablet.lock` 记录 PID；下一次启动若发现 lock 文件，验证 PID 是否还活着：

| 平台 | 实现 |
|------|------|
| Windows | `tasklist /FI "PID eq <pid>" /NH` 解析输出 |
| Unix-like (Linux/macOS) | `kill -0 <pid>` 不发信号仅检查存在性 |

代码用 `#[cfg(target_os = "windows")]` 分支隔离两套实现，对调用方透明。

### 12.3 工作目录与日志

- 默认 `--workdir` 为可执行文件所在目录；可显式覆盖
- 当前 Project 由 `tablet.toml` 的 `[project] last_project` 决定；CLI 也支持 `--project <id>` 覆盖（@02 Project / @03.4）
- 文件日志写入 `<workdir>/tablet.log`，等级由 `[ui] log_level` 配置项决定
- 不写 stdout/stderr（subsystem=windows 后两者无效）

## 13. RefPicker 数据通道

弹窗的 search / manual / strategy 三个 in-out 属性都需要"slint 端写 + Rust 端立刻重 push"才能让筛选 / 列表跟着刷新。仅靠 in-out 双向绑定，slint 写完不会触发 Rust 重新计算 `rows`，列表就不动。所以三处都额外加了 callback：

```slint
edited(s) => { root.set-search(s); }   // 写值 + 通知 Rust
```

```rust
ui.on_rp_search_edited(move |q| {
    s.borrow_mut().ref_picker.search = q.to_string();
    if let Some(ui) = weak.upgrade() { push_ref_picker(&ui, &s); }
});
```

参考实现 `crates/app-slint/src/main.rs::wire_ref_picker`。

## 14. 其它 slint 语义陷阱

### 14.1 layout 子元素禁止设置 x/y

`HorizontalLayout` / `VerticalLayout` 的直接子元素如果显式设置 `x:` 或 `y:`，编译报：

```
The property 'x' cannot be set for elements placed in this layout, because the layout is already setting it
```

所以"行号列固定不水平滚动"不能在数据 ScrollView 内的 row HorizontalLayout 里给 row-num 元素加 `x: -scroll-x` 抵消，必须把行号拆出 layout（如 §11 的独立 clip 容器）。

### 14.2 input property 不能在 callback 内赋值

```slint
in property <int> rp-strategy-index;
// set-strategy(i) => { root.rp-strategy-index = i; }   ← 编译报错
```

编译报 `Assignment on a input property`。需要把声明改成 `in-out property`，或用单独的 callback 让 Rust 端回写。
