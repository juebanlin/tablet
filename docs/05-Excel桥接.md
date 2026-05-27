# Excel 桥接

## 触发方式

点击"用 Excel 打开"时，将当前节点所属的**整个 Group** 生成为一个 xlsx（每个 Table/Constant 一个 tab）。

## xlsx 生成规则

| 模式 | tab 布局 |
|------|----------|
| Table | 第1行：中文描述（desc）<br>第2行：类型（type）<br>第3行：导出标记（export）<br>第4行：字段名（field）<br>第5行起：数据 |
| Constant | 第1行：固定表头 name / type / value / export / desc<br>第2行起：数据 |

## 流程

```
点击"用Excel打开"
  │
  ├─ 1. 读取 Group 下所有 .tbl，生成 .tbl-cache/GroupName.xlsx
  ├─ 2. open::that() 调起系统默认程序
  ├─ 3. UI 标记该 Group 为"编辑中"
  │
  └─ 后台线程检测 Excel 关闭：
       loop { sleep(1s); 尝试独占打开文件 → 成功说明 Excel 已关闭 }
       │
       ├─ 4. 读取 xlsx，按 tab 解析回各 Table/Constant 数据
       ├─ 5. 写入临时文件（各 .tbl.tmp）
       └─ 6. 进入验证 → 保存流程（见 UI 功能文档）
```

Excel 关闭后的验证、标红、保存流程与 UI 内编辑一致，详见 [04-UI功能 · 编辑模式](04-UI功能.md#编辑模式)。

## 容错机制

| 场景 | 处理 |
|------|------|
| Excel 长时间未关闭 | 超时（4小时）后提示策划手动确认 |
| Excel/WPS 崩溃 | 提供"强制解析"按钮，手动触发从缓存 xlsx 读取 |
| 工具启动时有残留 | 检查 .tbl-cache/ 是否有 xlsx 或 .tbl.tmp，提示恢复或丢弃 |
| 策划放弃修改 | 丢弃临时文件，恢复节点正常状态 |
| 退出工具 | 自动删除所有临时文件，不保存 |
