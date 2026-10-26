# 云亭 yt-tool-tauri · Tauri 2.x 迁移进度记录

> 目标：用 Tauri 2.x 重写「云亭」，只复用旧版验证过的纯函数，底层（进程/系统代理/IPC）全部 Rust 重写。
> 原则：原 Electron 工程（../yunting 或历史 yt-tool）零改动；本目录独立 git 历史。

## 2026-10-26 · M0 环境隔离与工具链 ✅

✅ done:
- 工程复制：`yunting` → `yt-tool-tauri`（robocopy 全量复制后清理 node_modules/dist/.git/.ecache 等构建产物；`app/` Electron 源码保留作移植参考与单测基准）
- `git init` 全新仓库，首次提交含全部源码+内核资源
- Rust 1.99.1 (x86_64-pc-windows-msvc) + MSVC Build Tools 14.44 + WinSDK 10.0.26100 安装完成，cargo hello-msvc 链接冒烟通过
- WebView2 运行时确认存在（155.0.4283.62）

⏳ next: Tauri 2.x 工程骨架（手写最小工程，UI 复用旧版原生 JS）→ `cargo build` 通过 → `tauri dev` 冒烟
