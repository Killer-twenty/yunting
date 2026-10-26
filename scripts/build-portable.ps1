# 云亭 Tauri 版便携包构建 —— 7z SFX 方案（复刻旧工程 scripts/build-portable.ps1 验证过的流程）
# NSIS 打 56MB 包在受限会话会 mmap 失败(#12345)，7z SFX 绕开（旧工程同款结论）
# 产物: dist\云亭-portable.exe —— 双击解压到当前目录并自动启动
param(
    [string]$DistDir = "$PSScriptRoot\..\dist",
    [string]$ReleaseExe = "$PSScriptRoot\..\src-tauri\target\release\yt-tool.exe"
)

$ErrorActionPreference = 'Stop'
$portableDir = Join-Path $DistDir 'portable\云亭'
$outexe = Join-Path $DistDir '云亭-portable.exe'

# 工具（旧工程缓存）
$sevenZr = 'E:\Agents\DSH_Projects\yunting\.ebcache\7zr.exe'
$sevenZip = 'E:\Agents\DSH_Projects\yunting\.ecache\7zip-pkg\package\7zip-lite\7z.exe'
$sfx = 'E:\Agents\DSH_Projects\yunting\.ecache\7zip-pkg\package\7zip-lite\7z.sfx'

foreach ($t in @($sevenZr, $sevenZip, $sfx)) {
    if (-not $t -or -not (Test-Path $t)) { throw "tool missing: $t" }
}

# 1) 组装便携目录（先清空防嵌套）
if (Test-Path $portableDir) { Remove-Item -Recurse -Force $portableDir }
New-Item -ItemType Directory -Force $portableDir | Out-Null
Copy-Item $ReleaseExe (Join-Path $portableDir '云亭.exe') -Force
Copy-Item "$PSScriptRoot\..\kernel" (Join-Path $portableDir 'kernel') -Recurse -Force
# 数据目录放 exe 旁（便携模式识别用）
New-Item -ItemType Directory -Force (Join-Path $portableDir 'data') | Out-Null

# 2) 打 7z 包
$work = Join-Path $DistDir 'portable-work'
New-Item -ItemType Directory -Force $work | Out-Null
$arc = Join-Path $work 'app.7z'
if (Test-Path $arc) { Remove-Item $arc -Force }
& $sevenZr a -mx=7 $arc "$portableDir\*" | Out-Null
"packed: $((Get-Item $arc).Length / 1MB) MB"

# 3) SFX 配置（双击 → 解压到当前目录 → 直接启动）
# ⚠️ 必须 UTF-8 无 BOM —— 7z SFX 要求首字节是 ";!@Install@!UTF-8!"，BOM 会让配置块整体失效（双击无反应）
$sfxCfg = Join-Path $work 'sfx-config.txt'
$cfgText = @"
;!@Install@!UTF-8!
Title="云亭"
BeginPrompt="解压云亭便携版到当前目录并启动？"
RunProgram="云亭.exe"
ExtractTitle="云亭 · 首次解压"
ExtractDialogText="正在解压云亭（仅首次运行需要）..."
GUIFlags="4+8+64+256"
Progress="yes"
;!@InstallEnd@!
"@
[System.IO.File]::WriteAllText($sfxCfg, $cfgText, (New-Object System.Text.UTF8Encoding $false))

# 4) 二进制拼接 sfx + config + 7z → portable.exe
if (Test-Path $outexe) { Remove-Item $outexe -Force }
$buf = New-Object System.IO.MemoryStream
$buf.Write([System.IO.File]::ReadAllBytes($sfx), 0, (Get-Item $sfx).Length)
$cfgBytes = [System.Text.Encoding]::UTF8.GetBytes((Get-Content $sfxCfg -Raw))
$buf.Write($cfgBytes, 0, $cfgBytes.Length)
$buf.Write([System.IO.File]::ReadAllBytes($arc), 0, (Get-Item $arc).Length)
[System.IO.File]::WriteAllBytes($outexe, $buf.ToArray())
$buf.Close()

"portable exe: $outexe ($([math]::Round((Get-Item $outexe).Length/1MB,1)) MB)"
