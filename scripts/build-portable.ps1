# 云亭便携绿色目录打包（替代 SFX 方案，用户已改为解压即用交付形态）
# 产出: dist/portable/云亭/ （云亭.exe + kernel/sing-box.exe + 双 srs + 空 data/）
# 用法: pwsh -File scripts/build-portable.ps1
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$release = Join-Path $root 'src-tauri\target\release'
$out = Join-Path $root 'dist\portable\云亭'

Write-Host '[1/4] cargo build --release'
Push-Location (Join-Path $root 'src-tauri')
cargo build --release
if ($LASTEXITCODE -ne 0) { throw 'cargo build --release failed' }
Pop-Location

Write-Host '[2/4] 组装目录'
if (Test-Path $out) {
  Remove-Item $out -Recurse -Force
}
New-Item -ItemType Directory -Force (Join-Path $out 'data') | Out-Null
Copy-Item (Join-Path $release 'yt-tool.exe') (Join-Path $out '云亭.exe') -Force
Copy-Item (Join-Path $root 'kernel') (Join-Path $out 'kernel') -Recurse -Force

Write-Host '[3/4] 清理交付物垃圾（运行数据/调试残渣不随包）'
Get-ChildItem (Join-Path $out 'data') -Force -ErrorAction SilentlyContinue | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue

Write-Host '[4/4] 验证'
$exe = Join-Path $out 'kernel\sing-box.exe'
if (-not (Test-Path $exe)) { throw 'kernel missing in portable dir' }
& $exe version | Select-Object -First 1
$size = (Get-ChildItem $out -Recurse -File | Measure-Object -Property Length -Sum).Sum
Write-Host "portable ready: $out ($([math]::Round($size/1MB,2)) MB)"
