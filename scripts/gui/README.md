# GUI 冒烟测试脚本

Windows 桌面上的半自动 GUI 验收脚手架。把之前「临时写一段 PowerShell + user32 截图」的做法收进来，跟着仓库版本走。

> 边界：**能拆成纯状态的断言必须放 Rust 单测**（`docs/08-测试.md` §6.2）。
> 这里只负责启动窗口、截图、坐标点击、发键；不做业务状态断言，不进 CI。
> 坐标受 DPI / 字体缩放 / Slint 版本影响，换机器后先看 `01-start.png` 再调坐标。

## 文件

| 文件 | 作用 |
|---|---|
| `GuiHarness.ps1` | 公共函数：建临时工作区、启动/移动/截图/点击/发键/关窗 |
| `smoke_gui.ps1` | 示例场景：建工作区 → 启动 → 截图 →（可选）点树节点 |

## 快速开始

```powershell
# 仓库根
cargo build -p tablet-slint -p tablet-cli

# 只截图
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/gui/smoke_gui.ps1

# 顺便点一下 HeroBase（默认 1280x800 窗口下的坐标）
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/gui/smoke_gui.ps1 -ClickHeroBase
```

截图默认在 `%TEMP%\tablet-gui-smoke\`。工作区默认在 `%TEMP%\tablet-gui-<guid>\`，结束后删除；要看现场加 `-KeepWorkspace`。

## 自己写场景

场景脚本统一 dot-source harness：

```powershell
. "$PSScriptRoot\GuiHarness.ps1"

$root = Join-Path $env:TEMP ("tablet-gui-" + [guid]::NewGuid().ToString('N'))
try {
    New-TabletWorkspace -Root $root -Template standard -Id demo | Out-Null
    Start-TabletGui -Workdir $root

    Save-TabletScreenshot -Path (Join-Path $env:TEMP 'before.png') | Out-Null

    # 窗口内坐标（截图坐标），默认已把窗口移到 (0,0)
    Invoke-TabletClick -X 160 -Y 724 -Double
    Start-Sleep -Milliseconds 500

    Save-TabletScreenshot -Path (Join-Path $env:TEMP 'after.png') | Out-Null
}
finally {
    Stop-TabletGui
    Remove-Item -LiteralPath $root -Recurse -Force
}
```

## 函数

| 函数 | 说明 |
|---|---|
| `New-TabletWorkspace -Root <dir> -Template <name> -Id <id>` | 用 `tablet-cli project new` 建临时工作区；目录必须不存在 |
| `Start-TabletGui -Workdir <dir> [-Exe <path>] [-NoMove]` | 启动 GUI，等主窗口，默认移到 `(0,0)` |
| `Set-TabletWindowOrigin -X 0 -Y 0` | 移动窗口，重置相对坐标原点 |
| `Save-TabletScreenshot -Path <png>` | `PrintWindow` 截窗口（被遮挡也能截） |
| `Invoke-TabletClick -X <n> -Y <n> [-Double] [-Absolute] [-Message]` | 窗口内坐标点击；默认先激活窗口；`-Message` 直接给窗口投递鼠标消息（非交互桌面兜底） |
| `Send-TabletKeys -Keys "<SendKeys 串>"` | 通过 `WScript.Shell` 发键；特殊字符按 SendKeys 规则转义 |
| `Stop-TabletGui` | 先 `CloseMainWindow`，不退出再强杀 |

## 注意事项

- 首次点击常常只是激活窗口；`Invoke-TabletClick` 已经包含一次点击，树节点/按钮尽量用 `-Double` 或连续调用两次。
- 不要用全屏 `CopyFromScreen`；`PrintWindow` 才能稳定截被遮挡的窗口。
- 临时工作区不要指向真实仓库；`New-TabletWorkspace` 会拒绝已存在目录，避免误覆盖。
- 自动化点击在部分非交互桌面会话里可能被前台窗口限制；失败时先用截图确认布局，或试 `-Message` 直投消息；仍不稳定就交给人工验收。
- 业务结果优先落 Rust 单测；例如撤销/重做的值、dirty、历史清空、跨项目隔离都在 `crates/core/src/ops.rs` / `history.rs` 的单测里。
