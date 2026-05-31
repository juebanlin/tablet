# Excel 桥接

UI 层包装 xlsx 编辑流的工作流文档。**xlsx 文件本身的读 / 写规则属于核心层** —— 详见 @02 "生成 Excel 文件" / "导入 Excel 文件"，本篇不重复。

桥接的本质：调起策划机器上的 Excel / WPS 编辑工具核心生成的 xlsx，关闭后把策划改动回写到 .tbl。CLI 不需要这条流程（要批量出 xlsx 直接 `tbl-cli export --xlsx`，要回写直接靠脚本调 import）。

## 1. 触发方式

UI 树节点右键「用 Excel 打开」时，把当前节点所属的**整个 Group** 生成为一个 xlsx（每个 Table / Constant / Enum 一个 sheet，规则见 @02）。

## 2. 流程

```
点击「用Excel打开」
  │
  ├─ 1. 调用 core::export_group_xlsx(group) → .tbl-cache/<group>.xlsx
  ├─ 2. open::that() 调起系统默认程序（Excel / WPS）
  ├─ 3. UI 标记该 Group 为「编辑中」，禁用其它写操作
  │
  └─ 后台线程检测 Excel 关闭：
       loop { sleep(1s); 尝试独占打开文件 → 成功说明 Excel 已关闭 }
       │
       ├─ 4. 调用 core::import_xlsx(path) 解析回内存模型
       ├─ 5. 写入临时文件（各 .tbl.tmp）
       └─ 6. 进入验证 → 保存流程（@04.4、@01.8）
```

第 1、4 步是核心层调用；2、3、5、6 步是 UI 层职责。

## 3. 容错机制

| 场景 | 处理 |
|------|------|
| Excel 长时间未关闭 | 超时（4 小时）后提示策划手动确认 |
| Excel/WPS 崩溃 | 提供「强制解析」按钮，手动触发从缓存 xlsx 读取 |
| 工具启动时有残留 | 检查 `.tbl-cache/` 是否有 xlsx 或 .tbl.tmp，提示恢复或丢弃 |
| 策划放弃修改 | 丢弃临时文件，恢复节点正常状态 |
| 退出工具 | 自动删除所有临时文件，不保存 |
| 解析报错 | core 返回 `ProjectErrors`，UI 在日志窗口列出位置 + 错误码（同 @01 附录 A） |

## 4. 剪贴板兼容（TSV/CSV）

GridArea 单元格 / 选区支持 Excel 风格的复制粘贴：

| 操作 | UI 端处理 |
|------|----------|
| Ctrl+C 复制选区 | 单元格用 `\t` 拼，行用 `\n` 分；调用平台剪贴板（windows: `clipboard-win`，slint: `slint::SharedString` + 平台桥接） |
| Ctrl+V 粘贴 | 读剪贴板字符串，按 `\t` / `\n` 切回 2D；按当前选区起点逐格写入，超出选区自动扩展（与 Excel 一致） |
| 剪贴板含逗号 | 仅切 `\t`，不切 `,`（避免误把 `Tuple<int,int>` 这种值拆开） |

剪贴板格式与 xlsx 是**两条独立的桥**：剪贴板走文本，xlsx 走二进制；它们的字段类型解析都委托回 core 的 `parse_*` 函数，保证 UI / 核心一致。

实现位置：`crates/app-egui/src/ui/grid.rs::handle_clipboard_*`、`crates/app-slint/src/main.rs` 中相同位置（slint 端待补）。

## 5. 状态

| 子项 | 状态 |
|------|------|
| 核心 xlsx 导出 / 导入 | ⚠️ 待开发，见 @02 |
| UI 桥接流程（调起 / 监控 / 临时文件） | ⚠️ 待开发，留到核心 xlsx 完成后做 |
| 剪贴板 TSV 复制粘贴 | egui 已实现 / slint 待补 |
