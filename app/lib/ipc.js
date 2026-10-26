// 主进程编排层：IPC handler —— 组合 lib（store/kernel/sysproxy/parsers）
// 渲染进程只经 preload 白名单调用这里
const crypto = require('crypto')
const store = require('./store')
const kernel = require('./kernel')
const sysproxy = require('./sysproxy')
const { parseShareLink, extractShareLinks, parseSubscription, dedupeNodes } = require('./parsers')

function log(win, line) {
  const ts = new Date().toLocaleTimeString('zh-CN', { hour12: false })
  win.webContents.send('log', `[${ts}] ${line}`)
}

function createIpc({ getWin }) {
  // ---------- 节点 ----------
  ipc('nodes:get', async () => store.loadNodes())

  ipc('nodes:addLinks', async (text) => {
    const links = extractShareLinks(text)
    const parsed = links.map(parseShareLink).filter(Boolean)
    if (parsed.length === 0) throw new Error('文本中没有可识别的分享链接（支持 vless/vmess/trojan/ss/hysteria2）')
    const existing = store.loadNodes()
    const merged = dedupeNodes([...existing, ...parsed])
    const added = merged.length - existing.length
    store.saveNodes(merged)
    log(getWin(), `导入 ${added} 个节点（识别 ${parsed.length}，去重后新增 ${added}）`)
    return { added, total: merged.length }
  })

  ipc('nodes:remove', async (tag) => {
    const nodes = store.loadNodes()
    const next = nodes.filter(n => n.tag !== tag)
    store.saveNodes(next)
    log(getWin(), `删除节点 ${tag}`)
    return next.length
  })

  // ---------- 订阅 ----------
  ipc('subs:get', async () => store.loadSubs())

  ipc('subs:add', async (url, name) => {
    const win = getWin()
    log(win, `拉取订阅 ${url}`)
    const text = await fetchText(url)
    const nodes = parseSubscription(text)
    if (nodes.length === 0) throw new Error('订阅内容解析不出节点')
    const sub = { id: crypto.randomUUID(), name: name || hostOf(url), url, addedAt: Date.now(), updatedAt: Date.now(), nodeCount: nodes.length }
    const subs = store.loadSubs()
    subs.push(sub)
    store.saveSubs(subs)
    // 节点带 subId，更新时替换该订阅的节点
    const existing = store.loadNodes().filter(n => n.subId !== sub.id)
    const merged = dedupeNodes([...existing, ...nodes.map(n => ({ ...n, subId: sub.id }))])
    store.saveNodes(merged)
    log(win, `订阅 ${sub.name} 拉取成功：${nodes.length} 节点`)
    return sub
  })

  ipc('subs:update', async (id) => {
    const subs = store.loadSubs()
    const sub = subs.find(s => s.id === id)
    if (!sub) throw new Error('订阅不存在')
    const win = getWin()
    log(win, `更新订阅 ${sub.name} ...`)
    const text = await fetchText(sub.url)
    const nodes = parseSubscription(text)
    if (nodes.length === 0) throw new Error('订阅更新后解析不出节点')
    sub.updatedAt = Date.now()
    sub.nodeCount = nodes.length
    store.saveSubs(subs)
    const existing = store.loadNodes().filter(n => n.subId !== id)
    store.saveNodes(dedupeNodes([...existing, ...nodes.map(n => ({ ...n, subId: id }))]))
    log(win, `订阅 ${sub.name} 更新成功：${nodes.length} 节点`)
    return sub
  })

  ipc('subs:remove', async (id) => {
    const subs = store.loadSubs().filter(s => s.id !== id)
    store.saveSubs(subs)
    store.saveNodes(store.loadNodes().filter(n => n.subId !== id))
    return subs.length
  })

  // ---------- 连接 ----------
  let currentProfile = null

  ipc('core:connect', async (profile) => {
    const win = getWin()
    const nodes = store.loadNodes()
    if (nodes.length === 0) throw new Error('没有可用节点，请先导入')
    const settings = store.loadSettings()
    const mode = settings.routeMode === 'global' ? 'global' : 'rule'

    currentProfile = {
      type: 'proxy',
      nodes,
      selectedTag: profile && profile.selectedTag,
      mixedPort: settings.mixedPort,
      clashApiPort: settings.clashApiPort,
      mode,
    }
    // 端口占用预检
    const busy = await checkPortBusy(settings.mixedPort)
    if (busy) throw new Error(`端口 ${settings.mixedPort} 被占用（可能已有内核或其他代理在跑），请在设置中改端口`)
    await kernel.start(currentProfile)
    log(win, `内核已启动（mixed :${settings.mixedPort} / clash api :${settings.clashApiPort} / 模式 ${mode === 'global' ? '全局代理' : '规则分流'}）`)
    // 自动开系统代理 —— 失败不阻塞连接（内核照常跑，UI 提示手动设置）
    if (settings.autoSystemProxy) {
      try {
        const r = await sysproxy.enable(settings.mixedPort)
        log(win, `系统代理已开启 → ${r.server}（via ${r.via}）`)
      } catch (e) {
        log(win, `[系统代理] 自动开启失败: ${e.message}`)
        log(win, `[系统代理] 请手动设置：Windows 设置 → 网络和 Internet → 代理 → 手动设置代理 → 127.0.0.1:${settings.mixedPort}`)
      }
    }
    broadcastState(getWin())
    return kernel.getState()
  })

  ipc('core:disconnect', async () => {
    const win = getWin()
    await sysproxy.disable({ restore: true }).catch(e => log(win, `系统代理恢复失败: ${e.message}`))
    await kernel.stop()
    log(win, '已断开，系统代理已恢复')
    broadcastState(getWin())
    return kernel.getState()
  })

  ipc('core:state', async () => ({
    core: kernel.getState(),
    proxy: await sysproxy.status().catch(() => ({ enabled: false })),
  }))

  ipc('core:delay', async (tag) => {
    const settings = store.loadSettings()
    const d = await kernel.testDelay(tag, settings.clashApiPort)
    log(getWin(), `延迟 ${tag}: ${d === null ? '超时' : d + 'ms'}`)
    return d
  })

  // ---------- 系统代理 ----------
  ipc('sysproxy:set', async (on) => {
    const settings = store.loadSettings()
    if (on) {
      await sysproxy.enable(settings.mixedPort)
      log(getWin(), '系统代理已开启')
    } else {
      await sysproxy.disable()
      log(getWin(), '系统代理已关闭')
    }
    broadcastState(getWin())
    return sysproxy.status()
  })

  ipc('sysproxy:get', async () => sysproxy.status())

  // ---------- 设置 ----------
  ipc('settings:get', async () => store.loadSettings())
  ipc('settings:save', async (s) => {
    const merged = { ...store.loadSettings(), ...s }
    if (merged.routeMode && !['rule', 'global'].includes(merged.routeMode)) {
      throw new Error(`未知分流模式: ${merged.routeMode}`)
    }
    store.saveSettings(merged)
    return merged
  })

  // kernel 状态/日志事件 → 渲染进程
  kernel.onState((s) => broadcastState(getWin()))
  kernel.onLog((l) => {
    const win = getWin()
    if (win) win.webContents.send('log', l)
  })

  return {
    // 退出清理：关系统代理（恢复）+ 停内核
    async shutdown() {
      try { await sysproxy.disable({ restore: true }) } catch { /* 尽力 */ }
      try { await kernel.stop() } catch { /* 尽力 */ }
    },
    // 启动检查：孤儿内核 + 系统代理残留
    async startupCheck() {
      const settings = store.loadSettings()
      const orphan = await kernel.cleanupOrphans().catch(() => 0)
      if (orphan > 0) log(getWin(), `清理了 ${orphan} 个残留 sing-box 进程`)
      const cleaned = await sysproxy.cleanupStale(settings.mixedPort, false).catch(() => false)
      if (cleaned) log(getWin(), '检测到系统代理残留（上次异常退出），已自动清除')
    },
  }
}

function ipc(channel, handler) {
  const { ipcMain } = require('electron')
  ipcMain.handle(channel, (_e, ...args) => handler(...args))
}

function broadcastState(win) {
  if (!win) return
  Promise.all([
    kernel.getState(),
    sysproxy.status().catch(() => ({ enabled: false, server: '', ours: false })),
  ]).then(([core, proxy]) => {
    win.webContents.send('state', { core, proxy })
  })
}

function hostOf(url) {
  try { return new URL(url).host } catch { return url.slice(0, 30) }
}

async function fetchText(url) {
  // Node 24 全局 fetch
  const r = await fetch(url, {
    headers: { 'User-Agent': 'clash-verge/1.7.7' }, // 订阅后端常见 UA 要求
    signal: AbortSignal.timeout(20000),
  })
  if (!r.ok) throw new Error(`订阅拉取失败 HTTP ${r.status}`)
  return r.text()
}

async function checkPortBusy(port) {
  const net = require('net')
  return new Promise((resolve) => {
    const s = net.createServer()
    s.once('error', () => resolve(true))
    s.once('listening', () => s.close(() => resolve(false)))
    s.listen(port, '127.0.0.1')
  })
}

module.exports = { createIpc, checkPortBusy }
