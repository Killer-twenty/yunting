// 路径解析 —— 全部收敛到这一个文件
// 数据目录策略(按优先级):
// 1. %APPDATA%\yunting —— 标准位置(安装版/开发版共用, 与打包目录解耦, 重打包/换目录不丢数据)
// 2. exe 旁 data/      —— 仅当 %APPDATA% 不可写时兜底(便携场景)
const path = require('path')
const fs = require('fs')

function exeDir() {
  try { return path.dirname(String(process.execPath())) } catch { return process.cwd() }
}

function isPackaged() {
  try {
    if (__dirname.includes('app.asar')) return true
    const exeName = path.basename(String(process.execPath())).toLowerCase()
    return !process.defaultApp && exeName.startsWith('云亭')
  } catch {
    return false
  }
}

let cachedRoot = null
function dataRoot() {
  if (cachedRoot) return cachedRoot
  const ad = process.env.APPDATA
  if (ad) {
    const dir = path.join(ad, 'yunting')
    try {
      fs.mkdirSync(dir, { recursive: true })
      fs.accessSync(dir, fs.constants.W_OK)
      cachedRoot = dir
      return dir
    } catch { /* 不可写 → 兜底 */ }
  }
  const fallback = path.join(exeDir(), 'data')
  try { fs.mkdirSync(fallback, { recursive: true }) } catch { }
  cachedRoot = fallback
  return fallback
}

function appRoot() {
  return isPackaged() ? exeDir() : path.join(__dirname, '..', '..')
}

function dataDir() {
  return dataRoot()
}

// exe 旁 data(英文路径, 仅供 Electron C++ 层使用: userData/单实例锁)
function exeDataDir() {
  const d = path.join(exeDir(), 'data')
  try { fs.mkdirSync(d, { recursive: true }) } catch { }
  return d
}

function kernelDir() {
  if (isPackaged()) {
    const res = path.join(exeDir(), 'resources', 'kernel')
    if (fs.existsSync(res)) return res
  }
  return path.join(__dirname, '..', '..', 'kernel')
}

function kernelExe() {
  return path.join(kernelDir(), 'sing-box.exe')
}

module.exports = { appRoot, dataDir, exeDataDir, kernelDir, kernelExe, isPackaged }
