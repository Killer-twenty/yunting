# 让 rustc 在任何新终端里都能找到 link.exe（一次性配置，可重复执行）
#
# 背景: 本机 Build Tools 2022 装在 E:\Tools\BuildTools，属非默认布局，
#       vswhere 未登记该实例（%ProgramData%\Microsoft\VisualStudio\Packages\_Instances 缺失），
#       注册表 VisualStudio\Setup 也为空 → rustc 三条发现路径(vswhere/注册表/环境变量)全断
#       → 报 "linker `link.exe` not found"。
#
# 原理: rustc 的 msvc 工具链发现逻辑里，只要 LIB 环境变量已设置就认定"环境已配好"，
#       直接采用 INCLUDE/LIB/PATH，不再依赖 vswhere。所以把 vcvars64 算出的值
#       持久化到用户级环境变量即可根治。
#
# 只写最小必要集合（不把 Roslyn/MSBuild/.NET/IDE 全塞进 PATH，避免污染）:
#   INCLUDE / LIB          —— 编译器与链接器找头文件/库
#   PATH += MSVC bin(HostX64\x64)  —— link.exe
#   PATH += Windows SDK bin(x64)   —— rc.exe（embed-resource 编 .rc 用，tauri-winres 依赖）
#
# Build Tools 升级后（MSVC/SDK 版本号变化）重新执行本脚本即可刷新。
$ErrorActionPreference = 'Stop'

# 1) 定位 vcvars64.bat：优先 vswhere，其次已知自定义路径
$vcCandidates = @()
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
if (Test-Path $vswhere) {
  $inst = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>$null
  if ($inst) { $vcCandidates += (Join-Path $inst 'VC\Auxiliary\Build\vcvars64.bat') }
}
$vcCandidates += @(
  'E:\Tools\BuildTools\VC\Auxiliary\Build\vcvars64.bat'
  'C:\BuildTools\VC\Auxiliary\Build\vcvars64.bat'
  'C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat'
)
$vc = $vcCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $vc) { throw "vcvars64.bat not found. tried:`n$($vcCandidates -join "`n")" }
Write-Host "vcvars64: $vc"

# 2) source vcvars64 并抓取导出的环境
$dump = & cmd /c "call `"$vc`" >nul 2>&1 && set"
$env1 = @{}
foreach ($line in $dump) {
  if ($line -match '^([^=]+)=(.*)$') { $env1[$matches[1]] = $matches[2] }
}
foreach ($need in @('INCLUDE', 'LIB', 'PATH')) {
  if (-not $env1[$need]) { throw "vcvars64 did not export $need" }
}

# 规范化：vcvars 输出含 `E:\Windows Kits\10\\bin\\x64` 这类重复分隔符
function Norm([string]$p) {
  ($p -split ';' | Where-Object { $_ } | ForEach-Object {
    ($_ -replace '\\+', '\').TrimEnd('\')
  }) -join ';'
}
$include = Norm $env1['INCLUDE']
$lib = Norm $env1['LIB']

# 3) 从 vcvars 的 PATH 里挑出 MSVC bin 与 Windows SDK bin（只要这两类）
$vcPaths = (Norm $env1['PATH']) -split ';'
$msvcBin = $vcPaths | Where-Object { $_ -match '\\VC\\Tools\\MSVC\\[^\\]+\\bin\\HostX64\\x64$' } | Select-Object -First 1
$sdkBin = $vcPaths | Where-Object { $_ -match '\\Windows Kits\\10\\bin\\[0-9.]+\\x64$' } | Select-Object -First 1
if (-not $msvcBin) { $msvcBin = (Norm $env1['VCToolsInstallDir']) + '\bin\HostX64\x64' }
if (-not $sdkBin) {
  $kits = (Get-ItemProperty 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows Kits\Installed Roots' -ErrorAction SilentlyContinue).KitsRoot10
  if ($kits) {
    $ver = (Get-ChildItem (Join-Path $kits 'Lib') -Directory -ErrorAction SilentlyContinue | Sort-Object Name -Descending | Select-Object -First 1).Name
    if ($ver) { $sdkBin = (Join-Path $kits "bin\$ver\x64") }
  }
}
if (-not (Test-Path $msvcBin)) { throw "MSVC bin dir missing: $msvcBin" }
Write-Host "link.exe dir : $msvcBin"
Write-Host "rc.exe dir   : $sdkBin"

# 4) 写用户级 INCLUDE / LIB
[Environment]::SetEnvironmentVariable('INCLUDE', $include, 'User')
[Environment]::SetEnvironmentVariable('LIB', $lib, 'User')

# 5) 追加 PATH（去重，保留原有用户 PATH；先备份）
$userPath = (Get-ItemProperty 'HKCU:\Environment' -ErrorAction SilentlyContinue).PATH
if ($null -eq $userPath) { $userPath = '' }
$backup = Join-Path $PSScriptRoot '..\_backup_20261029\user-PATH.before-msvc.txt'
New-Item -ItemType Directory -Force -Path (Split-Path $backup) | Out-Null
Set-Content -LiteralPath $backup -Value $userPath -Encoding UTF8
Write-Host "user PATH backed up -> $backup"

$parts = @()
foreach ($seg in ($userPath -split ';')) { if ($seg) { $parts += $seg } }
foreach ($add in @($msvcBin, $sdkBin)) {
  if ($add -and ($parts -notcontains $add)) { $parts = @($add) + $parts }
}
$newPath = ($parts -join ';')
[Environment]::SetEnvironmentVariable('PATH', $newPath, 'User')
Write-Host "user PATH updated (added $($parts.Count - (@($userPath -split ';' | Where-Object { $_ }).Count)) entries)"

# 6) 广播 WM_SETTINGCHANGE，让已运行的 Explorer 派生的新进程也能拿到
$sig = @'
[DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Auto)]
public static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint Msg, IntPtr wParam, string lParam, uint fuFlags, uint uTimeout, out IntPtr lpdwResult);
'@
try {
  if (-not ('Win32.Msg' -as [type])) { Add-Type -MemberDefinition $sig -Name Msg -Namespace Win32 -ErrorAction Stop }
  $HWND_BROADCAST = [IntPtr]0xffff; $WM_SETTINGCHANGE = 0x1A; $SMTO_ABORTIFHUNG = 0x0002
  $res = [IntPtr]::Zero
  [void][Win32.Msg]::SendMessageTimeout($HWND_BROADCAST, $WM_SETTINGCHANGE, [IntPtr]::Zero, 'Environment', $SMTO_ABORTIFHUNG, 5000, [ref]$res)
  Write-Host 'broadcast WM_SETTINGCHANGE sent'
} catch {
  Write-Warning "broadcast skipped ($_); 新开的终端仍会生效，已开终端请重开"
}

Write-Host "`nDONE. 新开一个终端执行 'where.exe link.exe' 应能看到 $msvcBin\link.exe"
