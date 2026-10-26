# 云亭 · 进度记录

## 2026-10-25 · M0 骨架 ✅

✅ done: git init；目录结构（app/main.js + preload + ui + lib + test + scripts + kernel）；sing-box 1.12.9 下载到 kernel/sing-box.exe；package.json（.npmrc 配 ELECTRON_MIRROR/ELECTRON_BUILDER_BINARIES_MIRROR + npmmirror registry）；Electron 空窗口冒烟通过（YUNTING_SMOKE=1 → window loaded ok → exit 0）。

⚠️ 踩坑（环境）：
- npm v11 的 allowScripts 拦了 electron postinstall → `npm install-scripts approve electron` + 手动装。
- Electron zip 从 npmmirror 下载 OK 但 GitHub 直连被重置；下载后 sha256 校验一致。
- **关键坑**：DSH 宿主会话里 `ELECTRON_RUN_AS_NODE=1` 会传染子进程，导致 electron 以纯 Node 跑（`app` undefined）；且 `%AppData%\Roaming\Electron` 被 ACL 拒绝 → 开窗即 c0000005 崩溃。修复：main.js 把 userData 重定向到 `data/electron/`；受限环境跑 GUI 需 `--no-sandbox`。
- 正常桌面环境（直接双击/终端）无上述问题。

## 2026-10-25 · M1 解析器 + 存储 ✅

✅ done: `app/lib/parsers.js` —— vless/vmess/trojan/ss/hysteria2 五协议分享链接解析（SIP002+旧式base64 ss、vless reality/ws/grpc、vmess base64-json、hy2 别名）、base64 订阅、clash yaml 订阅（js-yaml）、dedupe（raw + type|server|port 双键）。`app/lib/store.js` —— data/ 便携持久化，JSON 顶层 version=1，损坏容错回默认。`app/lib/paths.js` —— 路径收敛（打包/开发双态）。单测 18 个全绿。

⚠️ 踩坑：dedupe 只按 raw 会漏（同节点不同 #name），改双键去重；测试期望值自己写错一处已修正。

## 2026-10-25 · M2 内核管理 ✅

✅ done: `app/lib/configBuilder.js` 纯函数 —— selector(proxy)+urltest(auto)+direct/block；mixed 入站（TUN 分支已预留 type:'tun'）；DNS 分流（geosite-cn→223.5.5.5，其余→1.1.1.1 via proxy）；路由国内直连+国外代理。`app/lib/kernel.js` —— check→spawn→等 clash api 就绪→优雅停止；clash api GET 封装；testDelay；孤儿清理（wmic+taskkill）。`scripts/smoke.js` CLI 冒烟全过：check 通过、内核起、clash api version=sing-box 1.12.9、mode=rule、stop。单测累计 24 全绿，`sing-box check` exit=0 无警告。

⚠️ 踩坑：
- sing-box 1.12 出站类型是 `shadowsocks` 不是 `ss`（TYPE_MAP 映射解决）。
- 1.11+ 弃用 `dns` 出站 → 改 route rule action `hijack-dns`；1.12 弃用旧 DNS servers 格式 → 新格式 type:udp/https + domain_resolver。
- **remote rule-set 初始下载失败会 FATAL**（首次无代理可用时死锁）→ 改 local rule-set：geosite-cn.srs(441KB)+geoip-cn.srs(37KB) 随 kernel/ 分发，离线可用；缺文件时降级 remote（gh-proxy.com 镜像直连下载）。
- 官方 SagerNet/sing-geosite release 只有 .db 没有 .srs；MetaCubeX/meta-rules-dat 的 srs 在 `sing/geo/{geosite,geoip}/cn.srs`，raw.githubusercontent 本机被重置，gh-proxy.com 可用。

## 2026-10-25 · M3 系统代理 ✅

✅ done: `app/lib/sysproxy.js` —— `reg` 命令封装（无原生依赖）；ProxyServer/ProxyEnable 开关；`SendMessageTimeout` 广播 WM_SETTINGCHANGE（PowerShell Add-Type，3s 超时防挂）；开代理前保存原值、退出恢复；`cleanupStale(port, coreRunning)` 启动残留检测。真实注册表集成测试 2 个全过（开→查→内核未运行自动清→内核运行不清→关→恢复原状）。

## 2026-10-25 · M4 GUI ✅

✅ done: `app/lib/ipc.js` 主进程编排（节点/订阅/连接/延迟/系统代理/设置 全部 IPC handler；订阅 fetch 带 UA + 20s 超时；连接前端口占用预检；退出 shutdown 恢复代理+停内核）。UI（深色中文）：节点列表（选中/双击连测/删除/延迟着色）、分享链接导入对话框、订阅管理（增/更新/删）、一键连接/断开、全部测延迟、日志面板、设置（端口/自动系统代理）。单实例锁 `requestSingleInstanceLock`；启动孤儿 sing-box 清理 + 系统代理残留清理；`before-quit` 优雅关停 + `process.on('exit')` 同步兜底关代理。冒烟：GUI 全 UI 加载 exit 0；无 GUI 集成冒烟（导入→连接→clash api proxies 可见→断开恢复）全过。单测 26 全绿。

## 2026-10-25 · M5 打包 ✅

✅ done: 产物三件套（dist/）：
- `云亭-0.1.0-win.zip`（125MB，electron-builder zip target）
- `win-unpacked/`（解压即用目录，云亭.exe + resources/kernel/sing-box.exe + geosite/geoip srs）
- `云亭-portable.exe`（90MB，7z SFX 自解压：双击解压到当前目录并自动启动；因 NSIS 打 100MB+ exe 时 mmap 失败改用此方案，`scripts/build-portable.ps1` 固化）

干净路径验证全过（.verify-clean / .verify-portable）：全新目录无 data/ 无 node_modules → 云亭.exe 启动 → window loaded ok → data/electron 落在 exe 旁；打包目录内核 `sing-box check` exit 0 + run 后 clash api 1.12.9 响应 + 优雅停止。

⚠️ 踩坑（打包）：
- app-builder 自带下载器分块下载写 `%LOCALAPPDATA%\electron\Cache` 被 ACL 拒绝（用户名含非 ASCII）→ `ELECTRON_CACHE`/`ELECTRON_BUILDER_CACHE` 指到项目内 `.ecache`/`.ebcache`；electron zip 提前 curl 下载（sha256 与 checksums.json 一致）塞缓存。
- winCodeSign 解压 darwin symlink 需特权 → `--config.win.signAndEditExecutable=false` 跳过签名（自用无影响）。
- NSIS makensis 打 portable（100MB+ exe）报 `Internal compiler error #12345: error creating mmap`（会话内存限制）→ 换 7z SFX：`7z.sfx + sfx-config.txt + app.7z` 二进制拼接，RunProgram 直启。
- **打包版首启即崩（0xFFFF7003 + crashpad not connected）**：`paths.js` 开发态逻辑把 userData 指到 `app.asar` 只读路径内 mkdir 失败 → 回落 %AppData%（本机 ACL 拒绝）→ 崩。修复：`__dirname.includes('app.asar')` 判定打包态，data/ 一律落 exe 旁边。
- 7z SFX 模块来自 npm `7zip` 包（7zip-lite 自带 7z.sfx），npmmirror 可直下；gh-proxy.com 可代理 GitHub raw/release。

## 总结

M0–M5 全部完成。单测 26 个全绿；内核 check/启动/clash api/停止冒烟全过；真实注册表系统代理测试全过；打包产物在干净目录验证通过。

⏳ next（后续会话）: M6 VPS 服务端脚本（VLESS-Reality 部署，输出 vless:// 链接）；游戏联机加速（TUN 入站 + process_name 分支已预留）；NSIS 安装包；真实节点出墙实测（需 YG 的 VPS）。

## 2026-10-25 · 启动排查续（沙箱问题深挖）⚠️

✅ done: launcher.exe 方案（scripts/launcher.cs，C# 4KB，相对定位迁移即用：自动设 ELECTRON_DISABLE_SANDBOX=1 后启动同目录 win-unpacked\云亭.exe）。dist 下交付：云亭.exe(launcher) / 云亭.cmd / 云亭.vbs / 云亭-portable.exe / 云亭-0.1.0-win.zip。

⚠️ 深挖结论：
- `app.commandLine.appendSwitch('no-sandbox')` **不生效**（沙箱判断在 C++ 层先于 JS）——已从 main.js 移除。
- DSH 无头会话内打包 exe 行为不稳定（间歇 exit 0/崩溃），**无法在会话内复现用户桌面会话的条件**（window station 不同、宿主 ELECTRON_* 环境污染），最终验证必须由用户在桌面完成。
- Electron 45.0.0 同样崩 → 与版本无关，是 24H2 系统级 + 无头会话组合问题。
- 单实例锁排查：无残留进程握锁；exit=0 的"优雅退出"实为 main.js 未执行（asar 时间戳证据两次拆穿）。
- npm `7zip` 包（7zip-lite）自带 7z.sfx/7z.exe/7zCon.sfx —— 是可靠的 7z 工具源（npmmirror 可下）。

⏳ 用户桌面验证路径：双击 dist\云亭.exe（launcher）→ 窗口应出现。若仍崩：尝试 云亭.vbs / 云亭.cmd；把报错反馈回会话。

## 2026-10-25 · 用户桌面真机测试 · 客户端侧全通，隧道待服务端排查 ⚠️

✅ 已打通（用户桌面日志为证）：launcher 启动 ✅ 节点导入 ✅ 内核起(0.77s) ✅ clash api 就绪 ✅ 系统代理三层写入生效（via elevate，UAC 放行）✅ 分流正确（bilibili 三连 → direct 直连）
❌ 隧道不通：vless[yt-azure-hk]（104.208.108.150:443，Reality，sni=www.microsoft.com）测延迟全超时

🔍 客户端侧证据链（DSH 会话复测）：
- TCP 104.208.108.150:443 握手成功（有东西 LISTEN）
- 经隧道 CONNECT 200 后发 TLS ClientHello → 立即 Connection reset
- **绕过客户端、直接对 VPS:443 发 TLS ClientHello（SNI=www.microsoft.com）→ 同样 reset**——Reality 正确行为应是回落转发到真微软站点返回真证书
- 明文 HTTP 探测 → 0 字节静默关

→ 判定：**问题在服务端**，443 端口后面没有真正工作的 sing-box Reality 服务。

⏳ 服务端自查清单（VPS 上执行）：
1. `ss -tlnp | grep 443` —— sing-box 是否真的 LISTEN 443（还是别的进程占着）
2. `systemctl status sing-box`；`journalctl -u sing-box -n 50 --no-pager` —— 服务端有无 config error
3. Azure 门户 → NSG 入站规则确认 443 Allow；`iptables -L -n | grep 443`
4. 比对服务端 config.json 的 `users[].uuid` / `reality.private_key` / `short_id` 与客户端 vless 链接的 uuid/pbk/sid 逐项一致（私钥对应公钥 mVnH2NAAcFCIf-TJfjNTo0yAdN5qcZs4hONUcg_EPGY）
5. `systemctl restart sing-box` 后客户端重测延迟


## 2026-10-25 · VLESS-Reality 隧道终获打通 ✅

根因(两坑叠加):
1. 服务端原 1.12.9 二进制为精简编译版, 缺 with_reality_server 标签(对比 1.11.15 Tags 可证), Reality 服务端功能根本不存在 — 所有认证注定失败
2. SNI=1.1.1.1(IP 作 SNI)在 uTLS chrome 指纹下握手匹配异常; www.microsoft.com 回落在该机上有 DNS/v6 拖累
修复: 服务端换官方 1.11.15 + 回落/SNI 用 www.cloudflare.com + 全新凭据
验证: VPS 本机自连 ✅ → 本机经真实链路(家宽→Azure)端到端 ✅ 出口 104.208.108.150 / google 204 / youtube 200
云亭: kernel/ 与 dist/win-unpacked 内核均换 1.11.15(与服务端同版)
最终凭据: uuid=94f0c54e-... pbk=ZGjfhu... sid=f553330e sni=www.cloudflare.com
排查工具沉淀: .ebcache/{sb1115,mihomo} 双客户端交叉验证; X25519 私钥推公钥数学验证法; VPS 本机自连剥离法

## 2026-10-26 · 启动失败终局修复 ✅ userData 与数据目录分离

根因: Electron C++ 层单实例锁(process_singleton_win)对含非 ASCII 字符的 userData 路径(%APPDATA%\Killer丶二十\...)创建锁文件失败(Error 3) → 静默 exit 0。此前 08:33 把 userData 迁到 %APPDATA% 时引入。
修复: 双路径策略 — userData(仅 Electron C++ 用: 锁/GPU缓存)指 exe 旁 data\electron(英文路径); 业务数据(JS 层: nodes/subs/settings)走 %APPDATA%\yunting(中文路径对 JS 无影响)。两目录各司其职, 均不随打包丢失。
验证: 分离后 exit 0 → 窗口存活(标题"云亭") + diag 正常落盘。
