# 构建 release，并将可执行文件放到项目根目录，便于直接运行。
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$release = Join-Path $root 'src-tauri\target\release'
$source = Join-Path $release 'yunting.exe'
$destination = Join-Path $root 'yunting.exe'

$downloadScript = Join-Path $PSScriptRoot 'download-kernel.ps1'
& powershell -NoProfile -ExecutionPolicy Bypass -File $downloadScript
if ($LASTEXITCODE -ne 0) { throw 'Kernel preparation failed' }

Push-Location $root
try {
  npm run tauri build -- --no-bundle
  if ($LASTEXITCODE -ne 0) { throw 'Tauri release build failed' }
} finally {
  Pop-Location
}

if (-not (Test-Path -LiteralPath $source)) {
  throw "Release executable not found: $source"
}

Copy-Item -LiteralPath $source -Destination $destination -Force
Write-Host "release ready: $destination"
