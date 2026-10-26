# 下载 sing-box 内核到 kernel/（幂等：已存在则跳过）
# 版本锁定 1.11.15：configbuilder 生成的是 1.11 旧式 dns 格式（address 字段），
# 1.12+ 改 dns type/server 字段会破坏配置 —— 升级内核必须同步升级 configbuilder + 全部测试。
$ErrorActionPreference = 'Stop'
$SB_VERSION = '1.11.15'
$root = Split-Path -Parent $PSScriptRoot
$kernelDir = Join-Path $root 'kernel'
$tmp = Join-Path $kernelDir 'download.zip'
New-Item -ItemType Directory -Force -Path $kernelDir | Out-Null

$exe = Join-Path $kernelDir 'sing-box.exe'
if (Test-Path $exe) {
  Write-Host "kernel already exists: $exe"
  exit 0
}

$url = "https://github.com/SagerNet/sing-box/releases/download/v$SB_VERSION/sing-box-$SB_VERSION-windows-amd64.zip"
Write-Host "downloading $url"
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
Invoke-WebRequest -Uri $url -OutFile $tmp

Expand-Archive -Path $tmp -DestinationPath (Join-Path $kernelDir 'unpacked') -Force
$found = Get-ChildItem -Recurse -Filter 'sing-box.exe' (Join-Path $kernelDir 'unpacked') | Select-Object -First 1
Copy-Item $found.FullName $exe -Force
Remove-Item (Join-Path $kernelDir 'unpacked') -Recurse -Force
Remove-Item $tmp -Force
Write-Host "kernel ready: $exe"
& $exe version
