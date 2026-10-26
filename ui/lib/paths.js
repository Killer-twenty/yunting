// 路径解析（浏览器态）—— Tauri 版
// 数据目录策略(与 Rust paths.rs 一致):
//   打包态: exe 旁 data/          (便携,写数据在 exe 旁)
//   开发态: 项目根 data/          (cargo 调试运行时)
// 真实路径由 Rust 侧 paths.rs 收敛，前端仅通过 IPC 询问
if (typeof module !== 'undefined') {
  // Node 环境(单测): data/ = 项目根 data/
  const path = require('path')
  const fs = require('fs')
  function projectRoot() {
    // ui/lib/paths.js -> 项目根
    return path.join(__dirname, '..', '..')
  }
  module.exports = {
    dataDir: () => path.join(projectRoot(), 'data'),
    kernelDir: () => path.join(projectRoot(), 'kernel'),
    kernelExe: () => path.join(kernelDir(), 'sing-box.exe'),
    isPackaged: () => {
      try { return __dirname.includes('app.asar') } catch { return false }
    },
  }
} else {
  // 浏览器环境: 占位,实际路径在 Rust 侧
  window.__paths = { dataDir: () => null, kernelDir: () => null, kernelExe: () => null }
}
