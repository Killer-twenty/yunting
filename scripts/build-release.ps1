# 构建 release，并将可执行文件放到项目根目录，便于直接运行。
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$release = Join-Path $root 'src-tauri\target\release'
$source = Join-Path $release 'yunting.exe'
$destination = Join-Path $root 'yunting.exe'

# ---- MSVC 环境注入 ----------------------------------------------------
# 本机 Build Tools 装在 E:\Tools\BuildTools，vswhere 未登记该实例（_Instances 缺失），
# rustc 发现不了 link.exe → 报 "linker `link.exe` not found"。
# 这里主动 source vcvars64 把 INCLUDE/LIB/PATH 注入当前进程；
# 若环境已就绪（Developer Prompt 或用户级变量已配）则跳过。
if (-not (Get-Command link.exe -ErrorAction SilentlyContinue) -or -not $env:LIB) {
  $vcCandidates = @(
    'E:\Tools\BuildTools\VC\Auxiliary\Build\vcvars64.bat'
  )
  $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
  if (Test-Path $vswhere) {
    $inst = (& $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>$null)
    if ($inst) { $vcCandidates = @((Join-Path $inst 'VC\Auxiliary\Build\vcvars64.bat')) + $vcCandidates }
  }
  $vc = $vcCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1
  if ($vc) {
    Write-Host "injecting MSVC env from $vc"
    & cmd /c "call `"$vc`" >nul 2>&1 && set" | ForEach-Object {
      if ($_ -match '^([^=]+)=(.*)$') { Set-Item -Path ("Env:" + $matches[1]) -Value $matches[2] }
    }
  } else {
    Write-Warning 'vcvars64.bat not found; build may fail with "linker link.exe not found"'
  }
}
# ----------------------------------------------------------------------

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
