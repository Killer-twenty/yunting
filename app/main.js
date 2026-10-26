// 云亭 main process entry
const { app, BrowserWindow } = require('electron')
const path = require('path')
const fs = require('fs')

// ★ userData 指向 exe 旁(英文路径) — Electron C++ 单实例锁对非 ASCII 路径有 bug(Error 3 → 静默退出)
// 数据文件(JS 层)走 %APPDATA%\yunting(dataDir()), 两者分离
const { dataDir, exeDataDir } = require('./lib/paths')
const userDataDir = exeDataDir()
try { fs.mkdirSync(userDataDir, { recursive: true }) } catch { }
app.setPath('userData', userDataDir)

const diagFile = path.join(dataDir(), 'main-diag.log')
function diag(m) { try { fs.appendFileSync(diagFile, new Date().toISOString() + ' ' + m + '\n') } catch { } }
process.on('uncaughtException', (e) => { diag('UNCAUGHT: ' + e.stack); app.exit(1) })
diag('--- launch, cwd=' + process.cwd() + ' exe=' + process.execPath)

const gotLock = app.requestSingleInstanceLock()
diag('singleInstanceLock=' + gotLock)
if (!gotLock) { app.exit(0) } else {
  app.on('second-instance', () => { const w = BrowserWindow.getAllWindows()[0]; if (w) { if (w.isMinimized()) w.restore(); w.focus() } })
}

const SMOKE = !!process.env.YUNTING_SMOKE

let mainWin = null
let ipcApi = null

function createWindow() {
  const win = new BrowserWindow({
    width: 960, height: 680, minWidth: 820, minHeight: 580,
    backgroundColor: '#14161a', autoHideMenuBar: true, show: false,
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true, nodeIntegration: false, sandbox: true,
    },
  })
  win.once('ready-to-show', () => { diag('ready-to-show'); win.show() })
  win.webContents.on('did-finish-load', () => diag('did-finish-load'))
  win.webContents.on('render-process-gone', (_e, d) => diag('renderer-gone: ' + JSON.stringify(d)))
  win.webContents.on('preload-error', (_e, p, err) => diag('preload-error: ' + err))
  win.loadFile(path.join(__dirname, 'ui', 'index.html'))
  return win
}

app.whenReady().then(async () => {
  diag('app ready')
  if (process.env.YUNTING_HEADLESS) {
    try {
      require('./lib/configBuilder'); require('./lib/kernel'); require('./lib/store')
      console.log('[headless] main ready, libs loaded ok'); app.exit(0); return
    } catch (e) { console.error('[headless] lib load failed:', e.message); app.exit(1); return }
  }
  mainWin = createWindow()
  const { createIpc } = require('./lib/ipc')
  ipcApi = createIpc({ getWin: () => mainWin })
  if (SMOKE) {
    mainWin.webContents.on('did-finish-load', () => { console.log('[smoke] window loaded ok'); app.exit(0) })
  } else {
    await ipcApi.startupCheck()
  }
  app.on('activate', () => { if (BrowserWindow.getAllWindows().length === 0) mainWin = createWindow() })
})

async function gracefulShutdown() { if (ipcApi) { try { await ipcApi.shutdown() } catch { } } }
app.on('before-quit', (e) => {
  if (!gracefulShutdown.done) { e.preventDefault(); gracefulShutdown.done = true; gracefulShutdown().finally(() => app.quit()) }
})
app.on('window-all-closed', () => { diag('window-all-closed'); app.quit() })