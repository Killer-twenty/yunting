# 云亭 (Yunting) — Windows 桌面代理加速器

Electron 壳 + sing-box 内核。项目详情见《加速器任务启动Prompt.md》。

## 开发

```powershell
npm install          # .npmrc 已配国内镜像
npm run download-kernel   # 下载 sing-box 到 kernel/（已随仓库）
npm test             # 单测
npm run smoke        # 内核 CLI 冒烟（不起 GUI）
npm start            # 启动 GUI
npm run dist         # portable exe 打包
```

注意：本机（DSH 宿主会话内）直接 `npm start` 需带 `--no-sandbox` 且清理 `ELECTRON_RUN_AS_NODE`；正常桌面环境直接运行。
