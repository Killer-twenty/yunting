# Runtime kernel

This directory is populated by `scripts/download-kernel.ps1` during a local
build. The sing-box executable and rule files are intentionally not committed
to the source repository.

The build pins sing-box to version `1.11.15` and verifies the downloaded
executable reports that version before compiling the application.
