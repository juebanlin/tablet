# GUI 冒烟示例场景：建临时工作区 → 启动 GUI → 截图 →（可选）点树节点 → 关窗。
#
# 用法（仓库根）：
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts/gui/smoke_gui.ps1
#
# 先构建：
#   cargo build -p tablet-slint -p tablet-cli
#
# 截图默认写到 %TEMP%\tablet-gui-smoke\；工作区默认在 %TEMP%\tablet-gui-<guid>\，
# 不传 -KeepWorkspace 时结束后删除。

param(
    [string] $OutDir = (Join-Path $env:TEMP 'tablet-gui-smoke'),
    [string] $Exe = '',
    [switch] $ClickHeroBase,
    [switch] $KeepWorkspace
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'GuiHarness.ps1')

if ([string]::IsNullOrWhiteSpace($Exe)) {
    $Exe = Get-TabletExeDefault
}
if (-not (Test-Path -LiteralPath $OutDir)) {
    New-Item -ItemType Directory -Path $OutDir -Force | Out-Null
}

$root = Join-Path $env:TEMP ("tablet-gui-" + [guid]::NewGuid().ToString('N'))
Write-Host "workdir: $root"
Write-Host "outdir : $OutDir"

try {
    New-TabletWorkspace -Root $root -Template standard -Id demo | Out-Null
    Start-TabletGui -Workdir $root -Exe $Exe

    $shot = Join-Path $OutDir '01-start.png'
    Save-TabletScreenshot -Path $shot | Out-Null
    Write-Host "screenshot: $shot"

    if ($ClickHeroBase) {
        # 这个坐标是按默认 1280x800 窗口 + (0,0) 窗口原点测的；换 DPI / 缩放先看 01-start.png。
        Invoke-TabletClick -X 160 -Y 724 -Double -Message
        $shot2 = Join-Path $OutDir '02-hero.png'
        Save-TabletScreenshot -Path $shot2 | Out-Null
        Write-Host "screenshot: $shot2"
    }
}
finally {
    Stop-TabletGui
    if (-not $KeepWorkspace -and (Test-Path -LiteralPath $root)) {
        Remove-Item -LiteralPath $root -Recurse -Force
    }
}
