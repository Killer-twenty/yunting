// 云亭渲染进程 —— 只经 window.yunting IPC 调用，无业务逻辑
const api = window.yunting

const $ = (id) => document.getElementById(id)
const state = {
  nodes: [],
  selectedTag: null,
  delays: {},     // tag -> ms | 'fail'
  settings: {},
  core: null,
  proxy: null,
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

// ---------- 日志 ----------
function log(line) {
  const box = $('logBox')
  box.textContent += (box.textContent ? '\n' : '') + line
  box.scrollTop = box.scrollHeight
}

// ---------- 节点列表 ----------
function renderNodes() {
  const list = $('nodeList')
  $('nodeCount').textContent = state.nodes.length ? `${state.nodes.length} 个节点` : ''
  if (state.nodes.length === 0) {
    list.innerHTML = '<div class="empty">暂无节点 —— 点「导入节点」粘贴分享链接，或在右侧添加订阅</div>'
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
      await api.removeNode(n.tag)
      await refreshNodes()
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
}

async function refreshNodes() {
  state.nodes = await api.getNodes()
  if (state.selectedTag && !state.nodes.find(n => n.tag === state.selectedTag)) state.selectedTag = null
  renderNodes()
}

// ---------- 订阅列表 ----------
function renderSubs(subs) {
  const box = $('subList')
  box.innerHTML = ''
  if (!subs || subs.length === 0) { box.innerHTML = '<div style="color:var(--fg-dim);font-size:12px;padding:4px 0">无订阅</div>'; return }
  for (const s of subs) {
    const row = document.createElement('div')
    row.className = 'sub-row'
    const name = document.createElement('span')
    name.className = 'name'
    name.textContent = `${s.name} (${s.nodeCount})`
    name.title = s.url
    const upd = document.createElement('button')
    upd.textContent = '更新'
    upd.onclick = async () => {
      upd.disabled = true
      try { await api.updateSub(s.id); await refreshNodes(); toast('订阅已更新') }
      catch (e) { toast(e.message, true) }
      finally { upd.disabled = false; await refreshSubs() }
    }
    const del = document.createElement('button')
    del.textContent = '✕'
    del.onclick = async () => {
      await api.removeSub(s.id)
      await Promise.all([refreshSubs(), refreshNodes()])
    }
    row.append(name, upd, del)
    box.appendChild(row)
  }
}

async function refreshSubs() { renderSubs(await api.getSubs()) }

// ---------- 状态 ----------
function renderState(s) {
  if (s.core) state.core = s.core
  if (s.proxy) state.proxy = s.proxy
  const running = state.core && state.core.running
  $('pillCore').textContent = running ? `已连接 · ${state.core.pid}` : '未连接'
  $('pillCore').className = 'pill' + (running ? ' on' : '')
  $('pillProxy').textContent = `系统代理：${state.proxy && state.proxy.enabled ? '开' : '关'}`
  $('pillProxy').className = 'pill' + (state.proxy && state.proxy.enabled ? ' on' : '')
  $('btnConnect').style.display = running ? 'none' : ''
  $('btnDisconnect').style.display = running ? '' : 'none'
  $('btnSysProxy').textContent = state.proxy && state.proxy.enabled ? '关系统代理' : '开系统代理'
}

// ---------- 连接 ----------
async function connect() {
  const btn = $('btnConnect')
  btn.disabled = true
  btn.textContent = '连接中…'
  try {
    await api.connect({ selectedTag: state.selectedTag })
    toast('已连接')
  } catch (e) {
    toast(e.message, true)
    log(`[gui] 连接失败: ${e.message}`)
  } finally {
    btn.disabled = false
    btn.textContent = '一键连接'
    renderState(await api.getState())
  }
}

// ---------- 延迟 ----------
async function testDelay(tag) {
  const d = await api.testDelay(tag)
  state.delays[tag] = d === null ? 'fail' : d
  renderNodes()
}

async function testAll() {
  const tags = state.nodes.map(n => n.tag)
  if (tags.length === 0) return toast('没有节点', true)
  if (!(state.core && state.core.running)) return toast('请先连接内核（延迟测试走 clash api）', true)
  toast(`测 ${tags.length} 个节点…`)
  await Promise.all(tags.map(testDelay))
}

// ---------- 设置 ----------
async function loadSettings() {
  state.settings = await api.getSettings()
  $('setPort').value = state.settings.mixedPort
  $('setApiPort').value = state.settings.clashApiPort
  $('chkAutoProxy').checked = !!state.settings.autoSystemProxy
  $('setRouteMode').value = state.settings.routeMode || 'rule'
  updateRouteModeHint()
}

function updateRouteModeHint() {
  const hints = {
    rule: '国内白名单直连，其余走代理',
    global: '所有流量走代理节点',
  }
  $('routeModeHint').textContent = hints[$('setRouteMode').value] || ''
}

async function saveSettings(extra = {}) {
  const mixedPort = parseInt($('setPort').value, 10)
  const clashApiPort = parseInt($('setApiPort').value, 10)
  if (!(mixedPort > 0 && mixedPort < 65536)) return toast('本地端口无效', true)
  if (!(clashApiPort > 0 && clashApiPort < 65536)) return toast('API 端口无效', true)
  state.settings = await api.saveSettings({
    mixedPort, clashApiPort,
    autoSystemProxy: $('chkAutoProxy').checked,
    routeMode: $('setRouteMode').value,
    ...extra,
  })
  toast('设置已保存（重连后生效）')
}

// ---------- 事件绑定 ----------
$('btnConnect').onclick = connect

$('btnDisconnect').onclick = async () => {
  $('btnDisconnect').disabled = true
  try { await api.disconnect(); toast('已断开') }
  catch (e) { toast(e.message, true) }
  finally { $('btnDisconnect').disabled = false; renderState(await api.getState()) }
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
    toast(`导入成功：新增 ${r.added}，共 ${r.total}`)
  } catch (e) { toast(e.message, true) }
}

$('btnTestAll').onclick = testAll
$('btnRefresh').onclick = async () => { await Promise.all([refreshNodes(), refreshSubs(), renderState(await api.getState())]) }

$('btnSubAdd').onclick = async () => {
  const url = $('subUrl').value.trim()
  if (!url) return toast('请输入订阅链接', true)
  try {
    await api.addSub(url)
    $('subUrl').value = ''
    await Promise.all([refreshSubs(), refreshNodes()])
    toast('订阅添加成功')
  } catch (e) { toast(e.message, true) }
}

$('btnSysProxy').onclick = async () => {
  const on = !(state.proxy && state.proxy.enabled)
  try { renderState({ proxy: await api.setSystemProxy(on) }) }
  catch (e) { toast(e.message, true) }
}

$('setPort').onchange = () => saveSettings()
$('setApiPort').onchange = () => saveSettings()
$('chkAutoProxy').onchange = () => saveSettings()
$('setRouteMode').onchange = () => { updateRouteModeHint(); saveSettings() }

// ---------- 主进程事件 ----------
api.onLog(log)
api.onState(renderState)

// ---------- 功能导航(占位 tab) ----------
document.querySelectorAll('.nav-tab').forEach(btn => {
  btn.addEventListener('click', () => {
    if (btn.disabled) return
    document.querySelectorAll('.nav-tab').forEach(b => b.classList.remove('active'))
    btn.classList.add('active')
    const tab = btn.dataset.tab
    document.getElementById('tab-vpn').style.display = tab === 'vpn' ? '' : 'none'
    document.getElementById('tab-game').style.display = tab === 'game' ? '' : 'none'
    document.getElementById('tab-online').style.display = tab === 'online' ? '' : 'none'
  })
})

// ---------- 初始化 ----------
;(async function init() {
  log('[gui] 云亭启动')
  await loadSettings()
  await Promise.all([refreshNodes(), refreshSubs(), renderState(await api.getState())])
})()
