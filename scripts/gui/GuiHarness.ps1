# GUI 冒烟测试公共脚本（Windows / PowerShell）
#
# 用法：场景脚本里 dot-source 本文件：
#   . "$PSScriptRoot\GuiHarness.ps1"
#
# 定位：
# - 只做「启动窗口 / 截图 / 相对窗口点击 / 发键 / 关窗」这些桌面动作；
# - 能拆成纯状态的断言必须放 Rust 单测（docs/08 §6.2）；
# - 本脚本不进 CI：没有桌面会话、DPI / 字体 / Slint 版本都会让坐标漂移。
#
# 约定：
# - 点击坐标是**窗口内坐标**（截图里的坐标），不是屏幕坐标；
# - 启动后默认把窗口移到 (0,0)，减少窗口位置造成的漂移；
# - 每次动作前重新读窗口矩形，不缓存一次坐标用到底。

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

if (-not ('TabletGui.Win32' -as [type])) {
    Add-Type @"
using System;
using System.Runtime.InteropServices;
namespace TabletGui {
  public static class Win32 {
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)]
    public struct POINT { public int X, Y; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(
        IntPtr hWnd, IntPtr hWndInsertAfter, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hWnd, IntPtr hdcBlt, uint nFlags);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(
        uint flags, uint dx, uint dy, uint data, UIntPtr extra);
    [DllImport("user32.dll")] public static extern bool ScreenToClient(IntPtr hWnd, ref POINT pt);
    [DllImport("user32.dll")] public static extern bool PostMessage(
        IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);
  }
}
"@
}

$script:TabletProcess = $null
$script:TabletHwnd = [IntPtr]::Zero
$script:TabletOriginX = 0
$script:TabletOriginY = 0

function Get-RepoRoot {
    return (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
}

function Get-TabletExeDefault {
    return (Join-Path (Get-RepoRoot) 'target\debug\tablet.exe')
}

function Get-TabletCliDefault {
    return (Join-Path (Get-RepoRoot) 'target\debug\tablet-cli.exe')
}

function New-TabletWorkspace {
    param(
        [Parameter(Mandatory = $true)] [string] $Root,
        [string] $Template = 'standard',
        [string] $Id = 'demo',
        [string] $Cli = (Get-TabletCliDefault)
    )
    if (Test-Path -LiteralPath $Root) {
        throw "工作目录已存在，拒绝覆盖: $Root"
    }
    if (-not (Test-Path -LiteralPath $Cli)) {
        throw "找不到 tablet-cli: $Cli（先 cargo build -p tablet-cli）"
    }
    New-Item -ItemType Directory -Path $Root | Out-Null
    & $Cli -w $Root project new --template $Template --id $Id
    if ($LASTEXITCODE -ne 0) {
        throw "创建临时工作区失败，exit=$LASTEXITCODE"
    }
    return $Root
}

function Wait-TabletWindow {
    param([int] $TimeoutSeconds = 15)
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        if ($null -eq $script:TabletProcess) { throw 'GUI 进程尚未启动' }
        $script:TabletProcess.Refresh()
        $h = $script:TabletProcess.MainWindowHandle
        if ($h -ne [IntPtr]::Zero) {
            # 刚拿到 handle 时窗口可能还没 Apply 初始尺寸（实测会截到 4x4）。
            # 等窗口至少到最小尺寸后再交给场景。
            $r = New-Object TabletGui.Win32+RECT
            [TabletGui.Win32]::GetWindowRect($h, [ref]$r) | Out-Null
            $w = $r.Right - $r.Left
            $hh = $r.Bottom - $r.Top
            if ($w -ge 400 -and $hh -ge 300) {
                $script:TabletHwnd = $h
                Update-TabletWindowOrigin
                Start-Sleep -Milliseconds 500
                return
            }
        }
        Start-Sleep -Milliseconds 200
    }
    throw "等待 GUI 主窗口超时（${TimeoutSeconds}s）"
}

function Update-TabletWindowOrigin {
    if ($script:TabletHwnd -eq [IntPtr]::Zero) { throw 'GUI 主窗口尚未就绪' }
    $r = New-Object TabletGui.Win32+RECT
    [TabletGui.Win32]::GetWindowRect($script:TabletHwnd, [ref]$r) | Out-Null
    $script:TabletOriginX = $r.Left
    $script:TabletOriginY = $r.Top
}

function Start-TabletGui {
    param(
        [Parameter(Mandatory = $true)] [string] $Workdir,
        [string] $Exe = (Get-TabletExeDefault),
        [int] $TimeoutSeconds = 15,
        [switch] $NoMove
    )
    if (-not (Test-Path -LiteralPath $Exe)) {
        throw "找不到 GUI 可执行文件: $Exe（先 cargo build -p tablet-slint）"
    }
    if (-not (Test-Path -LiteralPath $Workdir)) {
        throw "工作目录不存在: $Workdir"
    }
    $script:TabletProcess = Start-Process -FilePath $Exe `
        -ArgumentList '--gui', "--workdir=$Workdir" -PassThru
    Wait-TabletWindow -TimeoutSeconds $TimeoutSeconds
    if (-not $NoMove) {
        Set-TabletWindowOrigin -X 0 -Y 0
    }
    [TabletGui.Win32]::SetForegroundWindow($script:TabletHwnd) | Out-Null
    Start-Sleep -Milliseconds 300
}

function Set-TabletWindowOrigin {
    param([int] $X = 0, [int] $Y = 0)
    if ($script:TabletHwnd -eq [IntPtr]::Zero) { throw 'GUI 主窗口尚未就绪' }
    # SWP_NOSIZE=0x0001, SWP_NOZORDER=0x0004
    [TabletGui.Win32]::SetWindowPos(
        $script:TabletHwnd, [IntPtr]::Zero, $X, $Y, 0, 0, 0x0001 -bor 0x0004) | Out-Null
    Start-Sleep -Milliseconds 150
    Update-TabletWindowOrigin
}

function Save-TabletScreenshot {
    param([Parameter(Mandatory = $true)] [string] $Path)
    if ($script:TabletHwnd -eq [IntPtr]::Zero) { throw 'GUI 主窗口尚未就绪' }
    Update-TabletWindowOrigin
    $r = New-Object TabletGui.Win32+RECT
    [TabletGui.Win32]::GetWindowRect($script:TabletHwnd, [ref]$r) | Out-Null
    $w = $r.Right - $r.Left
    $h = $r.Bottom - $r.Top
    if ($w -le 0 -or $h -le 0) { throw "窗口尺寸异常: ${w}x${h}" }

    $dir = Split-Path -Parent $Path
    if ($dir -and -not (Test-Path -LiteralPath $dir)) {
        New-Item -ItemType Directory -Path $dir -Force | Out-Null
    }
    $bmp = New-Object System.Drawing.Bitmap $w, $h
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $hdc = $g.GetHdc()
    [TabletGui.Win32]::PrintWindow($script:TabletHwnd, $hdc, 2) | Out-Null
    $g.ReleaseHdc($hdc)
    $g.Dispose()
    $bmp.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    return $Path
}

function Invoke-TabletClick {
    param(
        [Parameter(Mandatory = $true)] [int] $X,
        [Parameter(Mandatory = $true)] [int] $Y,
        [switch] $Double,
        [switch] $Absolute,
        [switch] $Message
    )
    if ($script:TabletHwnd -eq [IntPtr]::Zero) { throw 'GUI 主窗口尚未就绪' }
    Update-TabletWindowOrigin
    $screenX = $X
    $screenY = $Y
    if (-not $Absolute) {
        $screenX = $script:TabletOriginX + $X
        $screenY = $script:TabletOriginY + $Y
    }
    [TabletGui.Win32]::SetForegroundWindow($script:TabletHwnd) | Out-Null
    Start-Sleep -Milliseconds 150

    if ($Message) {
        # 非交互桌面 / 前台锁受限时，直接给窗口投递鼠标消息。
        $pt = New-Object TabletGui.Win32+POINT
        $pt.X = $screenX
        $pt.Y = $screenY
        [TabletGui.Win32]::ScreenToClient($script:TabletHwnd, [ref]$pt) | Out-Null
        $lp = [IntPtr]::new([int64]((($pt.Y -band 0xFFFF) -shl 16) -bor ($pt.X -band 0xFFFF)))
        [TabletGui.Win32]::PostMessage($script:TabletHwnd, 0x0200, [IntPtr]::Zero, $lp) | Out-Null
        [TabletGui.Win32]::PostMessage($script:TabletHwnd, 0x0201, [IntPtr]::new(1), $lp) | Out-Null
        [TabletGui.Win32]::PostMessage($script:TabletHwnd, 0x0202, [IntPtr]::Zero, $lp) | Out-Null
        if ($Double) {
            Start-Sleep -Milliseconds 80
            [TabletGui.Win32]::PostMessage($script:TabletHwnd, 0x0201, [IntPtr]::new(1), $lp) | Out-Null
            [TabletGui.Win32]::PostMessage($script:TabletHwnd, 0x0202, [IntPtr]::Zero, $lp) | Out-Null
        }
        Start-Sleep -Milliseconds 250
        return
    }

    [TabletGui.Win32]::SetCursorPos($screenX, $screenY) | Out-Null
    Start-Sleep -Milliseconds 120

    [TabletGui.Win32]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 60
    [TabletGui.Win32]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
    if ($Double) {
        Start-Sleep -Milliseconds 80
        [TabletGui.Win32]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 60
        [TabletGui.Win32]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
    }
    Start-Sleep -Milliseconds 250
}

function Send-TabletKeys {
    param([Parameter(Mandatory = $true)] [string] $Keys)
    if ($script:TabletHwnd -eq [IntPtr]::Zero) { throw 'GUI 主窗口尚未就绪' }
    $wshell = New-Object -ComObject WScript.Shell
    $wshell.AppActivate($script:TabletProcess.Id) | Out-Null
    Start-Sleep -Milliseconds 120
    $wshell.SendKeys($Keys)
    Start-Sleep -Milliseconds 250
}

function Stop-TabletGui {
    if ($null -eq $script:TabletProcess) { return }
    try {
        if (-not $script:TabletProcess.HasExited) {
            $script:TabletProcess.CloseMainWindow() | Out-Null
            Start-Sleep -Milliseconds 800
        }
        if (-not $script:TabletProcess.HasExited) {
            Stop-Process -Id $script:TabletProcess.Id -Force
        }
    } finally {
        $script:TabletProcess = $null
        $script:TabletHwnd = [IntPtr]::Zero
    }
}
