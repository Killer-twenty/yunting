# yunting — Windows 桌面代理加速器

Tauri 2.x 壳 + sing-box 内核。零 npm 运行时依赖（前端为原生 JS 纯函数模块），Rust 侧负责进程管理、系统代理与配置生成。

## 功能

- 节点导入：支持 `vless://`（含 Reality）/ `vmess://` / `trojan://` / `ss://` / `hysteria2://` 分享链接与 base64 订阅、Clash YAML 订阅
- 一键连接：sing-box 规则分流 / 全局代理，mixed 本地端口 + Clash API
- 系统代理：自动设置 / 恢复 Windows 用户级代理（异常退出自动清理残留）
- 内核管理：配置预检（`sing-box check`）、启动自愈、孤儿进程清理

## 目录结构

```text
ui/                前端（原生 JS：parsers / configBuilder / bridge / renderer）
src-tauri/         Rust 后端（paths / store / kernel / proxy_util / clashapi / configbuilder）
kernel/            sing-box 内核（构建时下载，不入库；仅 README 入库）
scripts/           构建与部署脚本
```

## 开发

```powershell
npm install          # .npmrc 已配国内镜像，仅装 @tauri-apps/cli
npm test             # 前端纯函数单测（node --test，25 例）
cargo test           # Rust 集成测试（smoke / config_compare / packaged_flow）
npm run dev          # 启动 Tauri 开发窗口
```

内核锁定 sing-box `1.11.15`（configbuilder 生成 1.11 旧式 DNS 格式，升内核须同步升级 configbuilder 与全部测试）。如果 `kernel/` 为空，先运行：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/download-kernel.ps1
```

## 构建与交付

```powershell
npm run build                                        # release 构建 → 根目录 yunting.exe
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build-portable.ps1   # 便携目录 dist/portable/yunting/
```

- `npm run build`：下载/校验内核 → `tauri build --no-bundle` → 拷贝 `yunting.exe` 到项目根目录
- 便携包：`yunting.exe + kernel/ + data/` 解压即用，产物在 `dist/portable/yunting/`
- `src-tauri/target/` 是内部构建缓存，不要分发其中的中间产物；最终交付以 GitHub Release 附件发布，不入源码仓库

## 数据目录

优先级：`YUNTING_DATA_DIR` 环境变量 → `%APPDATA%\yunting`（不可写时自动回退）→ exe 旁 `data/`（便携模式）。

## 服务端

`scripts/server-deploy.sh`：Ubuntu/Debian VPS 一键部署 VLESS-Reality 服务端（sing-box 1.11.15 官方完整版），产出可直接导入 yunting 的 `vless://` 链接。`scripts/server-config-reference.json` 为对应配置参考（占位凭据）。
