// Windows 系统代理开关 —— 注册表 + WM_SETTINGCHANGE 广播
// HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings
// 三层写入策略: reg.exe → PowerShell Set-ItemProperty → UAC 提权；全失败则降级(不阻塞内核连接)
const { exec } = require('child_process')

const IE_KEY = 'HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings'
const IE_KEY_PS = 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings'

// 记住「我们开代理前」的原值，退出时恢复；null = 原本没开
let saved = null
// 最近一次写失败的方式(供 UI 提示)
let lastWriteError = null

function regRun(args) {
  return new Promise((resolve, reject) => {
    exec(`reg ${args}`, { windowsHide: true }, (err, stdout, stderr) => {
      if (err) reject(new Error(`reg ${args}: ${String(stderr || err.message).trim()}`))
      else resolve(stdout)
    })
  })
}

function psRun(command) {
  return new Promise((resolve, reject) => {
    exec(`powershell -NoProfile -Command "${command.replace(/"/g, '\\"')}"`, { windowsHide: true, timeout: 15000 }, (err, stdout, stderr) => {
      if (err) reject(new Error(`powershell: ${String(stderr || err.message).trim()}`))
      else resolve(stdout)
    })
  })
}

/** UAC 提权跑一条命令(用户点"是"后以管理员执行) */
function elevateRun(command) {
  return new Promise((resolve, reject) => {
    const ps = `Start-Process cmd -Verb RunAs -Wait -WindowStyle Hidden -ArgumentList '/c', '${command.replace(/'/g, "''")}'`
    exec(`powershell -NoProfile -Command "${ps.replace(/"/g, '\\"')}"`, { windowsHide: true, timeout: 120000 }, (err, stdout, stderr) => {
      if (err) reject(new Error(`elevate: ${String(stderr || err.message).trim()}`))
      else resolve(stdout)
    })
  })
}

async function readProxy() {
  const out = await regRun(`query "${IE_KEY}" /v ProxyServer`)
  const m = String(out).match(/ProxyServer\s+REG_(?:SZ|EXPAND_SZ)\s+(.+)/i)
  return m ? m[1].trim() : ''
}

async function readEnable() {
  const out = await regRun(`query "${IE_KEY}" /v ProxyEnable`)
  const m = String(out).match(/ProxyEnable\s+REG_DWORD\s+0x(\d+)/i)
  return m ? parseInt(m[1], 16) === 1 : false
}

/**
 * 三层写入: reg.exe → PowerShell → UAC 提权
 * 返回 { ok, via } 或 throw(全部失败)
 */
async function writeRegValues(values) {
  // 层1: reg.exe(多条一次拼)
  try {
    for (const [name, type, data] of values) {
      await regRun(`add "${IE_KEY}" /v ${name} /t ${type} /d "${data}" /f`)
    }
    lastWriteError = null
    return { ok: true, via: 'reg' }
  } catch (e1) {
    lastWriteError = e1.message
  }
  // 层2: PowerShell Set-ItemProperty
  try {
    for (const [name, , data] of values) {
      await psRun(`Set-ItemProperty -Path '${IE_KEY_PS}' -Name ${name} -Value '${data}'`)
    }
    lastWriteError = null
    return { ok: true, via: 'powershell' }
  } catch (e2) {
    lastWriteError = e2.message
  }
  // 层3: UAC 提权(用户确认后管理员执行)
  try {
    const cmd = values.map(([name, type, data]) =>
      `reg add "${IE_KEY}" /v ${name} /t ${type} /d "${data}" /f`).join(' && ')
    await elevateRun(cmd)
    // 验证确实写进去了
    const check = await readEnable().catch(() => null)
    lastWriteError = null
    return { ok: true, via: 'elevate', verified: check }
  } catch (e3) {
    lastWriteError = e3.message
    throw new Error(`系统代理写入失败(已试 reg/PowerShell/管理员三种方式): ${e3.message}`)
  }
}

/** 广播 WM_SETTINGCHANGE，让 IE/WinINET/Chrome 等立即生效（带超时防挂） */
function broadcastSettingChange() {
  const ps = `Add-Type -TypeDefinition "using System; using System.Runtime.InteropServices; public static class W { [DllImport(\\"user32.dll\\", CharSet = CharSet.Auto)] public static extern IntPtr SendMessageTimeout(IntPtr h, uint m, UIntPtr w, string l, uint f, uint t, out UIntPtr r); }" [W]::SendMessageTimeout([IntPtr]0xffff, 0x1A, [UIntPtr]::Zero, "Internet Settings", 2, 3000, [ref]([UIntPtr]::Zero)) | Out-Null`
  return new Promise((resolve) => {
    exec(`powershell -NoProfile -Command "${ps.replace(/"/g, '\\"')}"`, { windowsHide: true, timeout: 10000 }, () => resolve())
  })
}

/**
 * 开系统代理（127.0.0.1:port）。记录原值用于恢复。
 */
async function enable(port) {
  const server = `127.0.0.1:${port}`
  const wasEnabled = await readEnable().catch(() => false)
  const prevServer = await readProxy().catch(() => '')
  saved = { enabled: wasEnabled, server: prevServer }
  const r = await writeRegValues([
    ['ProxyServer', 'REG_SZ', server],
    ['ProxyEnable', 'REG_DWORD', '1'],
  ])
  await broadcastSettingChange()
  return { server, via: r.via }
}

/**
 * 关系统代理。restore=true 且之前记过原值 → 恢复原值而非简单关闭。
 */
async function disable({ restore = false } = {}) {
  if (restore && saved && saved.enabled && saved.server) {
    await writeRegValues([
      ['ProxyServer', 'REG_SZ', saved.server],
      ['ProxyEnable', 'REG_DWORD', '1'],
    ]).catch(() => {})
  } else {
    await writeRegValues([['ProxyEnable', 'REG_DWORD', '0']]).catch(() => {})
  }
  saved = null
  await broadcastSettingChange()
}

/** 当前状态（供 UI 显示） */
async function status() {
  const [enabled, server] = await Promise.all([readEnable().catch(() => false), readProxy().catch(() => '')])
  return { enabled, server, ours: enabled && /^127\.0\.0\.1:\d+$/.test(server), lastWriteError }
}

/**
 * 启动残留检测：系统代理指向 port 但内核未运行 → 自动清掉。
 * 返回是否执行了清理。
 */
async function cleanupStale(port, coreRunning) {
  const s = await status().catch(() => null)
  if (!s || !s.enabled) return false
  if (s.server === `127.0.0.1:${port}` && !coreRunning) {
    await disable()
    return true
  }
  return false
}

/** 测试钩子 */
function _hasSaved() { return saved !== null }

module.exports = { enable, disable, status, cleanupStale, broadcastSettingChange, _hasSaved }