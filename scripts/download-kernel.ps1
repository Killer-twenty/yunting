# 下载 sing-box 内核到 kernel/（幂等：已存在则跳过）
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$kernelDir = Join-Path $root 'kernel'
$tmp = Join-Path $kernelDir 'download.zip'
New-Item -ItemType Directory -Force -Path $kernelDir | Out-Null

$exe = Join-Path $kernelDir 'sing-box.exe'
if (Test-Path $exe) {
  Write-Host "kernel already exists: $exe"
  exit 0
}

$url = 'https://github.com/SagerNet/sing-box/releases/download/v1.12.9/sing-box-1.12.9-windows-amd64.zip'
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
