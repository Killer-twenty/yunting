// yunting 渲染进程 —— 只经 window.yunting bridge 调用，无业务逻辑
// Tauri 版 UI: 左侧导航 + VPN(默认)/订阅/日志/设置/游戏(占位)/联机(占位)
const api = window.yunting

const $ = (id) => document.getElementById(id)
const state = {
  nodes: [],
  selectedTag: null,
  delays: {},     // tag -> ms | 'fail'
  settings: {},
  core: null,
  proxy: null,
  reloading: false,
}

// ---------- toast ----------
let toastTimer = null
function toast(msg, isErr = false) {
  const t = $('toast')
  t.textContent = msg
  t.className = 'toast show' + (isErr ? ' err' : '')
  clearTimeout(toastTimer)
  toastTimer = setTimeout(() => { t.className = 'toast' }, isErr ? 4200 : 2200)
}

// Tauri 2 invoke 失败时 reject 的是字符串而非 Error，e.message 为 undefined 会导致红框无文字
function errText(e) {
  if (e == null) return '未知错误'
  if (typeof e === 'string') return e
  if (typeof e.message === 'string' && e.message) return e.message
  return String(e)
}

// ---------- 日志（节点追加，避免 textContent += 全量重排卡顿）----------
function log(line) {
  const box = $('logBox')
  const div = document.createElement('div')
  div.textContent = line
  box.appendChild(div)
  // 超量裁剪（保留最近 500 行）
  while (box.childNodes.length > 500) box.removeChild(box.firstChild)
  box.scrollTop = box.scrollHeight
}

// ---------- 页面导航 ----------
function switchPage(name) {
  document.querySelectorAll('.nav-item').forEach((it) => it.classList.toggle('active', it.dataset.page === name))
  document.querySelectorAll('.page').forEach((p) => p.classList.toggle('active', p.id === `page-${name}`))
}
document.querySelectorAll('.nav-item').forEach((item) => {
  item.addEventListener('click', () => switchPage(item.dataset.page))
})

// ---------- 节点列表 ----------
function renderNodes() {
  const list = $('nodeList')
  $('vpnNodeCount').textContent = state.nodes.length ? `${state.nodes.length} 个节点` : ''
  $('navSubCount').textContent = ''
  if (state.nodes.length === 0) {
    list.innerHTML = '<div class="empty">暂无节点 —— 点「导入节点」粘贴分享链接，或到「订阅管理」添加订阅</div>'
    renderConnectTarget()
    return
  }
  list.innerHTML = ''
  for (const n of state.nodes) {
    const row = document.createElement('div')
    row.className = 'node' + (n.tag === state.selectedTag ? ' selected' : '')
    const type = document.createElement('span')
    type.className = 'type'
    type.textContent = n.type
    const tag = document.createElement('span')
    tag.className = 'tag'
    tag.textContent = n.tag
    tag.title = `${n.server}:${n.server_port}`
    const delay = document.createElement('span')
    const d = state.delays[n.tag]
    if (d === 'fail') { delay.className = 'dl-fail'; delay.textContent = '超时' }
    else if (typeof d === 'number') {
      delay.className = 'delay ' + (d < 200 ? 'good' : d < 500 ? 'mid' : 'bad')
      delay.textContent = d + 'ms'
    } else { delay.className = 'delay'; delay.textContent = '' }
    const del = document.createElement('button')
    del.className = 'del'
    del.textContent = '✕'
    del.title = '删除'
    del.onclick = async (e) => {
      e.stopPropagation()
      try {
        await api.removeNode(n.tag)
        await refreshNodes()
        await reloadCoreAfterNodeChange()
      } catch (err) {
        const m = errText(err)
        toast(m, true)
        log(`[gui] 删除节点失败: ${m}`)
      }
    }
    row.append(type, tag, delay, del)
    row.onclick = () => {
      state.selectedTag = n.tag
      renderNodes()
    }
    row.ondblclick = async () => {
      state.selectedTag = n.tag
      renderNodes()
      if (!state.core || !state.core.running) await connect()
    }
    list.appendChild(row)
  }
  renderConnectTarget()
}

function renderConnectTarget() {
  const n = state.nodes.find((x) => x.tag === state.selectedTag)
  if (n) {
    $('connectTarget').textContent = n.tag
    $('connectDesc').textContent = `${n.type} · ${n.server}:${n.server_port}`
  } else if (state.nodes.length > 0) {
    $('connectTarget').textContent = '未选择节点'
    $('connectDesc').textContent = '点击节点选中，双击直接连接'
  } else {
    $('connectTarget').textContent = '未选择节点'
    $('connectDesc').textContent = '从下方列表选择，或双击节点直接连接'
  }
}

async function refreshNodes() {
  state.nodes = await api.getNodes()
  if (state.selectedTag && !state.nodes.find((n) => n.tag === state.selectedTag)) state.selectedTag = null
  renderNodes()
}

// ---------- 订阅列表 ----------
function renderSubs(subs) {
  const box = $('subList')
  box.innerHTML = ''
  $('navSubCount').textContent = subs && subs.length ? `${subs.length}` : ''
  if (!subs || subs.length === 0) {
    box.innerHTML = '<div class="empty">无订阅 —— 上方粘贴订阅链接添加</div>'
    return
  }
  for (const s of subs) {
    const row = document.createElement('div')
    row.className = 'sub-row'
    const name = document.createElement('div')
    name.className = 'name'
    name.innerHTML = ''
    const t = document.createElement('div')
    t.className = 't'
    t.textContent = s.name
    const u = document.createElement('div')
    u.className = 'u'
    u.textContent = s.url
    name.append(t, u)
    const meta = document.createElement('div')
    meta.className = 'meta'
    meta.textContent = `${s.nodeCount} 节点`
    const upd = document.createElement('button')
    upd.textContent = '更新'
    upd.onclick = async () => {
      upd.disabled = true
      try {
        await api.updateSub(s.id)
        await refreshNodes()
        await reloadCoreAfterNodeChange()
        toast('订阅已更新')
      }
      catch (e) {
        const m = errText(e)
        toast(m, true)
        log(`[gui] 订阅更新失败: ${m}`)
      }
      finally { upd.disabled = false; await refreshSubs() }
    }
    const del = document.createElement('button')
    del.textContent = '✕'
    del.title = '删除订阅'
    del.onclick = async () => {
      try {
        await api.removeSub(s.id)
        await Promise.all([refreshSubs(), refreshNodes()])
        await reloadCoreAfterNodeChange()
        toast('订阅已删除')
      } catch (e) {
        const m = errText(e)
        toast(m, true)
        log(`[gui] 删除订阅失败: ${m}`)
      }
    }
    row.append(name, meta, upd, del)
    box.appendChild(row)
  }
}

async function refreshSubs() { renderSubs(await api.getSubs()) }

// ---------- 状态（连接栏 + 导航底部状态点 + 设置页）----------
function renderState(s) {
  if (s && s.core) state.core = s.core
  if (s && s.proxy) state.proxy = s.proxy
  const running = state.core && state.core.running
  // 导航底部状态
  $('statusDot').className = 'status-dot' + (running ? ' on' : '')
  $('statusText').textContent = running ? `已连接 · ${state.core.pid}` : '未连接'
  // VPN 连接按钮
  $('btnConnect').style.display = running ? 'none' : ''
  $('btnDisconnect').style.display = running ? '' : 'none'
  $('btnTestAll').disabled = !!state.reloading
  $('btnTestAll').title = state.reloading ? '节点列表更新中，请稍候' : ''
  // 设置页系统代理状态
  if (state.proxy) {
    $('proxyStateText').textContent = state.proxy.enable
      ? `已开启 → ${state.proxy.server}${state.proxy.ours ? '' : '（他程序设置）'}`
      : '已关闭'
    $('btnSysProxy').textContent = state.proxy.enable ? '关系统代理' : '开系统代理'
  }
}

async function reloadCoreAfterNodeChange() {
  if (!(state.core && state.core.running)) return
  state.reloading = true
  renderState()
  log('[gui] 节点列表已更新，正在重载内核…')
  try {
    await api.disconnect()
    renderState(await api.getState())
    await api.connect({ selectedTag: state.selectedTag })
    renderState(await api.getState())
    toast('节点已更新，内核重载完成')
    log('[gui] 节点更新生效，内核重载完成')
  } catch (e) {
    const m = errText(e)
    toast(`节点已更新，但内核重载失败：${m}`, true)
    log(`[gui] 节点更新后内核重载失败: ${m}`)
  } finally {
    state.reloading = false
    try { renderState(await api.getState()) } catch (e) { log(`[gui] 状态刷新失败: ${errText(e)}`) }
  }
}

// ---------- 连接 ----------
async function connect() {
  const btn = $('btnConnect')
  if (btn.disabled) return
  btn.disabled = true
  btn.textContent = '连接中…'
  log('[gui] 开始连接：生成配置 → 校验内核 → 等待 clash api 就绪')
  try {
    await api.connect({ selectedTag: state.selectedTag })
    toast('已连接')
    log('[gui] 连接完成')
  } catch (e) {
    const m = errText(e)
    toast(m, true)
    log(`[gui] 连接失败: ${m}`)
  } finally {
    btn.disabled = false
    btn.textContent = '一键连接'
    try { renderState(await api.getState()) } catch (e) { log(`[gui] 状态刷新失败: ${errText(e)}`) }
  }
}

// ---------- 延迟 ----------
async function testDelay(tag) {
  if (state.reloading) return
  try {
    const d = await api.testDelay(tag)
    state.delays[tag] = d === null ? 'fail' : d
    renderNodes()
  } catch (e) {
    const m = errText(e)
    if (m.includes('404')) {
      toast('节点列表已更新，请断开重连后生效', true)
      log(`[gui] 延迟测试失败：节点列表已更新，请断开重连后生效（${tag}）`)
    } else {
      toast(m, true)
      log(`[gui] 延迟 ${tag} 测试失败: ${m}`)
    }
  }
}

async function testAll() {
  if (state.reloading) return toast('节点列表更新中，请稍候', true)
  const tags = state.nodes.map((n) => n.tag)
  if (tags.length === 0) return toast('没有节点', true)
  if (!(state.core && state.core.running)) return toast('请先连接内核（延迟测试走 clash api）', true)
  toast(`测 ${tags.length} 个节点…`)
  // 限并发 5：一次打满 clash api 会导致后端超时+前端卡顿
  const CONCURRENCY = 5
  let i = 0
  const workers = Array.from({ length: Math.min(CONCURRENCY, tags.length) }, async () => {
    while (i < tags.length) {
      const tag = tags[i++]
      await testDelay(tag)
    }
  })
  await Promise.all(workers)
}

// ---------- 设置 ----------
async function loadSettings() {
  state.settings = await api.getSettings()
  $('setPort').value = state.settings.mixedPort
  $('setApiPort').value = state.settings.clashApiPort
  $('chkAutoProxy').checked = !!state.settings.autoSystemProxy
  $('setRouteMode').value = state.settings.routeMode || 'rule'
  updateRouteModeHint()
  renderDirectList()
}

function updateRouteModeHint() {
  const hints = {
    rule: '国内白名单直连，其余走代理',
    global: '所有流量走代理节点',
  }
  $('routeModeHint').textContent = hints[$('setRouteMode').value] || ''
  $('directListPanel').style.display = $('setRouteMode').value === 'rule' ? '' : 'none'
}

// ---------- 直连白名单 ----------
function renderDirectList() {
  const box = $('directList')
  box.innerHTML = ''
  const list = state.settings.directList || []
  if (list.length === 0) {
    box.innerHTML = '<span style="font-size:11px;color:var(--fg-dim)">（空 = 仅内置国内规则直连）</span>'
    return
  }
  for (const d of list) {
    const chip = document.createElement('span')
    chip.style.cssText = 'background:var(--bg3);border:1px solid var(--border);border-radius:10px;padding:2px 8px;font-size:12px;display:inline-flex;align-items:center;gap:4px'
    const name = document.createElement('span')
    name.textContent = d
    const del = document.createElement('span')
    del.textContent = '✕'
    del.style.cssText = 'cursor:pointer;color:var(--fg-dim)'
    del.onmouseenter = () => { del.style.color = 'var(--err)' }
    del.onmouseleave = () => { del.style.color = 'var(--fg-dim)' }
    del.onclick = () => saveSettings({ directList: (state.settings.directList || []).filter((x) => x !== d) })
    chip.append(name, del)
    box.appendChild(chip)
  }
}

$('btnDirectAdd').onclick = () => {
  const v = $('directAddInput').value.trim().toLowerCase()
  if (!v) return
  if (!/^[a-z0-9][a-z0-9.-]*\.[a-z]{2,}$/.test(v)) return toast('域名格式不对，如 openai.com', true)
  const list = state.settings.directList || []
  if (list.includes(v)) return toast('已在白名单中')
  $('directAddInput').value = ''
  saveSettings({ directList: [...list, v] })
}

async function saveSettings(extra = {}) {
  const mixedPort = parseInt($('setPort').value, 10)
  const clashApiPort = parseInt($('setApiPort').value, 10)
  if (!(mixedPort > 0 && mixedPort < 65536)) return toast('本地端口无效', true)
  if (!(clashApiPort > 0 && clashApiPort < 65536)) return toast('API 端口无效', true)
  try {
    state.settings = await api.saveSettings({
      mixedPort, clashApiPort,
      autoSystemProxy: $('chkAutoProxy').checked,
      routeMode: $('setRouteMode').value,
      ...extra,
    })
    renderDirectList()
    toast('设置已保存（重连后生效）')
  } catch (e) {
    const m = errText(e)
    toast(m, true)
    log(`[gui] 设置保存失败: ${m}`)
  }
}

// ---------- 事件绑定 ----------
$('btnConnect').onclick = connect

$('btnDisconnect').onclick = async () => {
  const btn = $('btnDisconnect')
  if (btn.disabled) return
  btn.disabled = true
  const old = btn.textContent
  btn.textContent = '正在断开…'
  toast('正在断开，请稍候…')
  log('[gui] 开始断开：停止内核 + 恢复系统代理')
  try {
    await api.disconnect()
    toast('已断开')
    log('[gui] 断开完成')
  } catch (e) {
    const m = errText(e)
    toast(m, true)
    log(`[gui] 断开失败: ${m}`)
  } finally {
    btn.disabled = false
    btn.textContent = old || '断开'
    try { renderState(await api.getState()) } catch (e) { log(`[gui] 状态刷新失败: ${errText(e)}`) }
  }
}

$('btnImport').onclick = () => { $('importText').value = ''; $('dlgImport').showModal() }
$('btnImportCancel').onclick = () => $('dlgImport').close()
$('btnImportOk').onclick = async () => {
  const text = $('importText').value
  if (!text.trim()) return toast('内容为空', true)
  try {
    const r = await api.addShareLinks(text)
    $('dlgImport').close()
    await refreshNodes()
    await reloadCoreAfterNodeChange()
    toast(`导入成功：新增 ${r.added}，共 ${r.total}`)
  } catch (e) {
    const m = errText(e)
    toast(m, true)
    log(`[gui] 导入失败: ${m}`)
  }
}

$('btnTestAll').onclick = testAll

$('btnSubAdd').onclick = async () => {
  const url = $('subUrl').value.trim()
  if (!url) return toast('请输入订阅链接', true)
  const name = $('subName').value.trim() || undefined
  try {
    await api.addSub(url, name)
    $('subUrl').value = ''
    $('subName').value = ''
    await Promise.all([refreshSubs(), refreshNodes()])
    await reloadCoreAfterNodeChange()
    toast('订阅添加成功')
  } catch (e) {
    const m = errText(e)
    toast(m, true)
    log(`[gui] 订阅添加失败: ${m}`)
  }
}

$('btnSysProxy').onclick = async () => {
  const on = !(state.proxy && state.proxy.enable)
  try { renderState({ proxy: await api.setSystemProxy(on) }) }
  catch (e) {
    const m = errText(e)
    toast(m, true)
    log(`[gui] 系统代理失败: ${m}`)
  }
}

$('btnClearLog').onclick = () => { $('logBox').innerHTML = '' }

$('setPort').onchange = () => saveSettings()
$('setApiPort').onchange = () => saveSettings()
$('chkAutoProxy').onchange = () => saveSettings()
$('setRouteMode').onchange = () => { updateRouteModeHint(); saveSettings() }

// ---------- 主进程事件 ----------
api.onLog(log)
api.onState(renderState)

// ---------- 初始化 ----------
;(async function init() {
  log('[gui] yunting 启动')
  if (!api) {
    log('[gui] ⚠️ bridge 未就绪（window.yunting 缺失）—— 检查 tauri withGlobalTauri')
    return
  }
  await loadSettings()
  await Promise.all([refreshNodes(), refreshSubs(), renderState(await api.getState())])
})()
