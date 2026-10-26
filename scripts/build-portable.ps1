# 云亭 portable 打包脚本（7z SFX 方案，绕过 NSIS 大文件 mmap 限制）
# 前置: npx electron-builder --win zip 已产出 dist/win-unpacked
# 产物: dist/云亭-portable.exe（双击自解压到当前目录并启动）
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$ecache = Join-Path $root '.ecache'
$ebcache = Join-Path $root '.ebcache'
$sevenZip = Join-Path $ecache '7zip-pkg\package\7zip-lite'
$sfx = Join-Path $sevenZip '7z.sfx'
$sevenExe = Join-Path $sevenZip '7z.exe'

if (-not (Test-Path $sfx)) { throw "7z.sfx 不存在: $sfx（先跑 scripts/fetch-tools.ps1）" }
if (-not (Test-Path "$sevenExe")) { $sevenExe = Join-Path $ebcache '7zr.exe' }
if (-not (Test-Path "$root\dist\win-unpacked")) { throw 'dist/win-unpacked 不存在，先跑 npx electron-builder --win zip' }

Write-Host '[portable] 1/3 压缩 win-unpacked ...'
$arcPath = Join-Path $ebcache 'app.7z'
if (Test-Path $arcPath) { Remove-Item $arcPath -Force }
& $sevenExe a -t7z -mx=7 $arcPath "$root\dist\win-unpacked\*" | Out-Null
if ($LASTEXITCODE -ne 0) { throw '7z 压缩失败' }

Write-Host '[portable] 2/3 组装 SFX ...'
$out = Join-Path $root 'dist\云亭-portable.exe'
$fs = [IO.File]::Create($out)
foreach ($f in @($sfx, "$root\scripts\sfx-config.txt", $arcPath)) {
  $bytes = [IO.File]::ReadAllBytes($f)
  $fs.Write($bytes, 0, $bytes.Length)
}
$fs.Close()

Write-Host "[portable] 3/3 完成: $out ($((Get-Item $out).Length) bytes)"
