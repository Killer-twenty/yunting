// sing-box 内核进程管理
// 职责: 写 config -> spawn -> 健康等待 -> clash api 延迟测试 -> 停止/清孤儿
const { spawn, exec } = require('child_process')
const fs = require('fs')
const path = require('path')
const http = require('http')
const { kernelExe, dataDir } = require('./paths')
const { buildConfig } = require('./configBuilder')

let proc = null
let stateCb = null
let logCb = null

function onState(cb) { stateCb = cb }
function onLog(cb) { logCb = cb }
function emitState(s) { if (stateCb) stateCb(s) }
function emitLog(line) { if (logCb) logCb(line) }

function getState() {
  return {
    running: !!(proc && !proc.killed),
    pid: proc ? proc.pid : null,
    exitCode: proc ? (proc.exitCode ?? null) : null,
  }
}

function configPath() {
  return path.join(dataDir(), 'config.json')
}

/** 启动内核。profile 见 configBuilder。 */
async function start(profile) {
  if (proc) throw new Error('内核已在运行')
  const exe = kernelExe()
  if (!fs.existsSync(exe)) throw new Error(`内核不存在: ${exe}`)

  fs.mkdirSync(dataDir(), { recursive: true })
  const cfg = buildConfig(profile)
  const cfgPath = configPath()
  fs.writeFileSync(cfgPath, JSON.stringify(cfg, null, 2), 'utf-8')
  emitLog(`[core] config 写入 ${cfgPath}`)

  // 先 check 再跑，坏配置不启动
  await new Promise((resolve, reject) => {
    exec(`"${exe}" check -c "${cfgPath}"`, (err, _so, se) => {
      if (err) reject(new Error(`config 校验失败: ${se}`))
      else resolve()
    })
  })
  emitLog('[core] sing-box check 通过')

  proc = spawn(exe, ['run', '-c', cfgPath, '-D', dataDir()], {
    windowsHide: true,
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  emitLog(`[core] 内核已启动 pid=${proc.pid}`)
  emitState({ ...getState(), phase: 'starting' })

  proc.stdout.on('data', (d) => d.toString().split('\n').filter(Boolean).forEach(l => emitLog(`[core] ${l}`)))
  proc.stderr.on('data', (d) => d.toString().split('\n').filter(Boolean).forEach(l => emitLog(`[core] ${l}`)))
  proc.on('exit', (code) => {
    emitLog(`[core] 内核退出 code=${code}`)
    proc = null
    emitState({ running: false, pid: null, exitCode: code })
  })

  // 等待 clash api 就绪
  const clashApiPort = profile.clashApiPort || 9090
  await waitClashApi(clashApiPort, 10000)
  emitState({ ...getState(), phase: 'running' })
  return getState()
}

function waitClashApi(port, timeoutMs) {
  const start = Date.now()
  return new Promise((resolve, reject) => {
    function tick() {
      clashApiGet(port, '/version').then(() => resolve()).catch(() => {
        if (proc === null) return reject(new Error('内核启动过程中退出'))
        if (Date.now() - start > timeoutMs) return reject(new Error('clash api 就绪超时'))
        setTimeout(tick, 300)
      })
    }
    tick()
  })
}

function stop() {
  return new Promise((resolve) => {
    if (!proc) return resolve()
    const p = proc
    const killer = setTimeout(() => {
      try { p.kill('SIGKILL') } catch { /* already dead */ }
    }, 3000)
    p.on('exit', () => {
      clearTimeout(killer)
      resolve()
    })
    try { p.kill() } catch { resolve() }
  })
}

// ---------- clash api ----------
function clashApiGet(port, p) {
  return new Promise((resolve, reject) => {
    const req = http.get({ host: '127.0.0.1', port, path: p, timeout: 2500 }, (res) => {
      let body = ''
      res.on('data', (c) => body += c)
      res.on('end', () => {
        if (res.statusCode !== 200) return reject(new Error(`clash api ${res.statusCode}`))
        try { resolve(JSON.parse(body)) } catch { resolve(body) }
      })
    })
    req.on('error', reject)
    req.on('timeout', () => { req.destroy(); reject(new Error('timeout')) })
  })
}

/** 测某节点延迟（走 clash api）。返回毫秒或 null。tag='proxy' 测当前选中。 */
async function testDelay(tag, clashApiPort = 9090, url = 'https://www.gstatic.com/generate_204') {
  try {
    const q = `?timeout=5000&url=${encodeURIComponent(url)}`
    const r = await clashApiGet(clashApiPort, `/proxies/${encodeURIComponent(tag)}/delay${q}`)
    return typeof r.delay === 'number' ? r.delay : null
  } catch {
    return null
  }
}

// ---------- 孤儿清理 ----------
/** 清理残留 sing-box 进程（按可执行文件路径匹配）。返回清理数量。 */
function cleanupOrphans() {
  return new Promise((resolve) => {
    exec('wmic process where "name=\'sing-box.exe\'" get processid,commandline /format:csv', (err, stdout) => {
      if (err) return resolve(0)
      const myKernel = kernelExe().toLowerCase()
      const lines = String(stdout).split('\n').map(l => l.trim()).filter(Boolean)
      let killed = 0
      const targets = []
      for (const line of lines) {
        const m = line.match(/(\d+)\s*$/)
        if (!m) continue
        const pid = parseInt(m[1], 10)
        if (!pid || pid === (proc && proc.pid)) continue
        if (line.toLowerCase().includes(myKernel)) targets.push(pid)
      }
      let pending = targets.length
      if (pending === 0) return resolve(0)
      for (const pid of targets) {
        exec(`taskkill /PID ${pid} /F`, () => {
          killed++
          if (--pending === 0) resolve(killed)
        })
      }
    })
  })
}

module.exports = { start, stop, getState, testDelay, clashApiGet, cleanupOrphans, onState, onLog, configPath }
