# yunting — Windows 桌面代理加速器

## 构建与运行

运行 `npm run build` 后，可执行文件会直接生成在项目根目录：

```text
yunting.exe
```

本地构建时，`scripts/download-kernel.ps1` 会下载固定版本的 sing-box 到 `kernel/`；内核二进制和规则文件被 Git 忽略，不上传到源码仓库。`src-tauri/target/` 仅用于 Rust 构建缓存和中间产物。

`src-tauri/target/release/` 是内部构建目录，不是用户交付目录。用户只需运行根目录的 `yunting.exe`；不要手动运行或分发 `target/release` 下的中间文件。

Tauri 2.x 壳 + sing-box 内核。

## 开发

```powershell
npm install          # .npmrc 已配国内镜像
npm test             # Node 原生单测
npm run dev          # 启动 Tauri 开发窗口
npm run build        # 构建 release，并将 yunting.exe 放到项目根目录
```

如果 `kernel/` 为空，先运行：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/download-kernel.ps1
```

然后再运行 `npm run build`。最终用户分发目录不应上传到源码仓库，而应作为 GitHub Release 附件发布。
