# 下载固定版本 sing-box 内核到 kernel/（幂等：已存在则校验版本）
# 版本锁定 1.11.15：configbuilder 生成的是 1.11 旧式 dns 格式（address 字段），
# 1.12+ 改 dns type/server 字段会破坏配置 —— 升级内核必须同步升级 configbuilder + 全部测试。
$ErrorActionPreference = 'Stop'
$SB_VERSION = '1.11.15'
$root = Split-Path -Parent $PSScriptRoot
$kernelDir = Join-Path $root 'kernel'
$cacheDir = Join-Path $root '.cache'
$tmp = Join-Path $cacheDir "sing-box-$SB_VERSION-windows-amd64.zip"
New-Item -ItemType Directory -Force -Path $kernelDir | Out-Null
New-Item -ItemType Directory -Force -Path $cacheDir | Out-Null

$exe = Join-Path $kernelDir 'sing-box.exe'
if (Test-Path $exe) {
  $current = (& $exe version | Out-String)
  if ($current -match [regex]::Escape($SB_VERSION)) {
    Write-Host "kernel already exists and is valid: $exe"
    exit 0
  }
  Remove-Item $exe -Force
}

$url = "https://github.com/SagerNet/sing-box/releases/download/v$SB_VERSION/sing-box-$SB_VERSION-windows-amd64.zip"
Write-Host "downloading $url"
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
if (-not (Test-Path $tmp)) { Invoke-WebRequest -Uri $url -OutFile $tmp }

 $unpacked = Join-Path $cacheDir 'sing-box-unpacked'
if (Test-Path $unpacked) { Remove-Item $unpacked -Recurse -Force }
Expand-Archive -Path $tmp -DestinationPath $unpacked -Force
$found = Get-ChildItem -Recurse -Filter 'sing-box.exe' $unpacked | Select-Object -First 1
if (-not $found) { throw 'sing-box.exe not found in downloaded archive' }
Copy-Item $found.FullName $exe -Force
Remove-Item $unpacked -Recurse -Force
Write-Host "kernel ready: $exe"
$versionOutput = (& $exe version | Out-String)
if ($versionOutput -notmatch [regex]::Escape($SB_VERSION)) {
  throw "Downloaded sing-box version does not match $SB_VERSION"
}
