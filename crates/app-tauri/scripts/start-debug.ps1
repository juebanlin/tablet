# SPDX-License-Identifier: Apache-2.0
# 启动带 CDP 调试端口的 Tauri 版，供截图 / 模拟操作 / AI 驱动测试。
#
# 用法：
#   powershell -File crates/app-tauri/scripts/start-debug.ps1 -Workdir <工作区>
# 之后：
#   node crates/app-tauri/scripts/dev.mjs snap
#   node crates/app-tauri/scripts/dev.mjs shot
param(
  [string]$Workdir = ".",
  [string]$Exe = "target\x86_64-pc-windows-msvc\debug\tablet-tauri.exe",
  [int]$Port = 9222
)

$ErrorActionPreference = "Stop"
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = Resolve-Path (Join-Path $here "..\..\..")
$exePath = if ([System.IO.Path]::IsPathRooted($Exe)) { $Exe } else { Join-Path $root $Exe }
$workdirPath = if ([System.IO.Path]::IsPathRooted($Workdir)) { $Workdir } else { Join-Path $root $Workdir }

$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$Port"
$proc = Start-Process -FilePath $exePath -ArgumentList "--gui", "--workdir=$workdirPath" -PassThru

for ($i = 0; $i -lt 80; $i++) {
  Start-Sleep -Milliseconds 250
  try {
    Invoke-RestMethod "http://127.0.0.1:$Port/json/list" -TimeoutSec 2 | Out-Null
    Write-Output "CDP ready: http://127.0.0.1:$Port (pid=$($proc.Id))"
    exit 0
  } catch {}
}

Write-Error "CDP 端口未就绪"
if (-not $proc.HasExited) { Stop-Process -Id $proc.Id -Force }
exit 1
