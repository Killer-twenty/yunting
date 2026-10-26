# 获取打包工具链（7z + sfx + 7zr），幂等
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$ecache = Join-Path $root '.ecache'
$ebcache = Join-Path $root '.ebcache'
New-Item -ItemType Directory -Force $ecache, $ebcache | Out-Null

$sevenDir = Join-Path $ecache '7zip-pkg\package\7zip-lite'
if (-not (Test-Path "$sevenDir\7z.sfx")) {
  Write-Host '[tools] 下载 7zip-lite (npmmirror)...'
  curl.exe -k -sL --max-time 120 -o "$ecache\7zip.tgz" 'https://registry.npmmirror.com/7zip/-/7zip-0.0.6.tgz'
  New-Item -ItemType Directory -Force (Join-Path $ecache '7zip-pkg') | Out-Null
  tar -xzf "$ecache\7zip.tgz" -C (Join-Path $ecache '7zip-pkg')
}
Write-Host "[tools] 7z.sfx: $(Test-Path "$sevenDir\7z.sfx")"

if (-not (Test-Path "$ebcache\7zr.exe")) {
  Write-Host '[tools] 下载 7zr (gh-proxy)...'
  curl.exe -k -sL --retry 3 --max-time 120 -o "$ebcache\7zr.exe" 'https://gh-proxy.com/https://github.com/ip7z/7zip/releases/download/26.05/7zr.exe'
}
Write-Host "[tools] 7zr: $((Get-Item "$ebcache\7zr.exe" -ErrorAction SilentlyContinue).Length) bytes"
