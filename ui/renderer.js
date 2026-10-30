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
  drawerTag: null, // 当前抽屉展示的节点 tag
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

// ---------- 日志（只保留记录功能：追加 + 自动跟随最新，无气泡/无位置记忆）----------
// ANSI 色码剥离：sing-box 日志带 [36m 等转义序列，WebView 不解析会显示成乱码
const ANSI_RE = /\x1b\[[0-9;]*m/g
function log(line) {
  const box = $('logBox')
  const div = document.createElement('div')
  div.textContent = line.replace(ANSI_RE, '')
  box.appendChild(div)
  // 超量裁剪（保留最近 500 行）
  while (box.childNodes.length > 500) box.removeChild(box.firstChild)
  if (box.offsetHeight === 0) return // 页面隐藏：只追加，进入时再滚底
  box.scrollTop = box.scrollHeight
}
function applyLogScroll() {
  // 进入日志页：直接到底部显示最新
  const box = $('logBox')
  if (!box) return
  requestAnimationFrame(() => { box.scrollTop = box.scrollHeight })
}

// ---------- 页面导航 ----------
function switchPage(name) {
  document.querySelectorAll('.nav-item').forEach((it) => {
    if (it === accel.root) return // 加速主项高亮由 syncAccelNav 管理
    it.classList.toggle('active', it.dataset.page === name)
  })
  document.querySelectorAll('.page').forEach((p) => p.classList.toggle('active', p.id === `page-${name}`))
  if (name === 'logs') applyLogScroll() // 进入日志页：直接到底部最新
  syncAccelNav(name) // 左栏顶部加速折叠：同步高亮/名称（展开期间不改主名称）
}
document.querySelectorAll('.nav-item').forEach((item) => {
  item.addEventListener('click', () => switchPage(item.dataset.page))
})

// ---------- 加速模式折叠下拉（左栏顶部：内联展开，把下方导航项往下推） ----------
// 交互规格：
//   · 默认只显示主按钮（导航项样式），文字为当前模式，右侧 ▾
//   · 点击主按钮 → 左栏内联折叠展开 3 个选项（非悬浮菜单），滑块背景随悬停垂直移动
//   · 展开期间点选项 → 只切页面，主按钮名称不变；移出选择区才提交名称并收起
const ACCEL_FEATURES = [
  { page: 'vpn', icon: '🛡️', label: 'VPN 加速' },
  { page: 'game', icon: '🎮', label: '游戏加速' },
  { page: 'online', icon: '🌐', label: '联机加速' },
]
const ACCEL_PAGES = new Set(ACCEL_FEATURES.map((f) => f.page))
const accel = {
  root: null, group: null, icon: null, label: null, collapse: null, slider: null,
  opts: {}, committed: 'vpn', pending: 'vpn', open: false,
}
function accelFeature(page) { return ACCEL_FEATURES.find((f) => f.page === page) }
// 指针是否落在组件区（主按钮 + 折叠选项区，8px 容差）
// 用坐标判定而非 mouseleave：折叠动画/布局变化会让 mouseleave 误触发
let accelPointer = { x: -1, y: -1 }
function insideAccel(x, y) {
  if (!accel.root || !accel.root.isConnected) return false
  const pad = 8
  const hit = (r) => x >= r.left - pad && x <= r.right + pad && y >= r.top - pad && y <= r.bottom + pad
  return hit(accel.root.getBoundingClientRect()) || hit(accel.collapse.getBoundingClientRect())
}
function moveAccelSlider(opt) {
  if (!opt) return
  accel.slider.style.transform = `translateY(${opt.offsetTop}px)`
  accel.slider.style.opacity = '1'
}
function paintAccelMain(page) {
  const feat = accelFeature(page)
  accel.icon.textContent = feat.icon
  accel.label.textContent = feat.label
}
function openAccel() {
  accel.open = true
  accel.pending = accel.committed
  accel.group.classList.add('open')
  accel.root.classList.add('open')
  moveAccelSlider(accel.opts[accel.pending])
  document.addEventListener('mousemove', onAccelMouseMove)
}
function closeAccel() {
  if (!accel.open) return
  accel.open = false
  accel.group.classList.remove('open')
  accel.root.classList.remove('open')
  accel.slider.style.opacity = '0'
  document.removeEventListener('mousemove', onAccelMouseMove)
  // 移出选择区 → 提交展开期间选中的功能：只更新主按钮名称。
  // 页面不在此处强切：展开期间每次点选项/点导航都已 switchPage 过，
  // 若这里再切会劫持"展开状态下直接点订阅管理等其它导航项"的行为。
  if (accel.pending !== accel.committed) {
    accel.committed = accel.pending
    paintAccelMain(accel.committed)
  }
  for (const [p, opt] of Object.entries(accel.opts)) opt.classList.toggle('current', p === accel.committed)
}
function onAccelMouseMove(e) {
  accelPointer = { x: e.clientX, y: e.clientY }
  if (accel.open && !insideAccel(e.clientX, e.clientY)) closeAccel()
}
function buildAccel() {
  // 组容器 = 主导航项 + 折叠选项区，插到左栏最上方
  accel.group = document.createElement('div')
  accel.group.className = 'accel-group'
  accel.root = document.createElement('div')
  accel.root.className = 'nav-item accel'
  accel.root.dataset.page = '__accel__'
  accel.icon = document.createElement('span')
  accel.icon.className = 'icon'
  accel.label = document.createElement('span')
  accel.label.className = 'nav-text'
  const caret = document.createElement('span')
  caret.className = 'accel-caret'
  caret.title = '展开功能选择'
  caret.textContent = '▾'
  caret.onclick = (e) => {
    // 只有点箭头才展开/收起折叠区
    e.stopPropagation()
    if (accel.open) { closeAccel(); return }
    openAccel()
  }
  accel.root.append(accel.icon, accel.label, caret)
  accel.root.onclick = () => {
    // 点名字（箭头以外区域）：只切到当前模式页，不展开折叠区
    if (accel.open) { closeAccel(); return }
    switchPage(accel.committed)
  }
  accel.collapse = document.createElement('div')
  accel.collapse.className = 'accel-collapse'
  accel.slider = document.createElement('div')
  accel.slider.className = 'accel-slider'
  accel.collapse.append(accel.slider)
  for (const f of ACCEL_FEATURES) {
    const opt = document.createElement('div')
    opt.className = 'accel-opt'
    const ic = document.createElement('span')
    ic.className = 'icon'
    ic.textContent = f.icon
    const name = document.createElement('span')
    name.className = 'name'
    name.textContent = f.label
    opt.append(ic, name)
    if (f.page !== 'vpn') opt.classList.add('wip') // 未实现功能置灰，不显示"开发中"文字
    opt.addEventListener('mouseenter', () => moveAccelSlider(opt))
    opt.onclick = (e) => {
      e.stopPropagation()
      accel.pending = f.page // 只记待提交选择：切页面，主名称不动，折叠保持展开
      switchPage(f.page)
      for (const [p, o] of Object.entries(accel.opts)) o.classList.toggle('current', p === f.page)
    }
    accel.opts[f.page] = opt
    accel.collapse.append(opt)
  }
  accel.collapse.addEventListener('mouseleave', () => { accel.slider.style.opacity = '0' })
  accel.group.append(accel.root, accel.collapse)
  document.getElementById('navItems').prepend(accel.group)
  // 点击组件外任意处 → 收起并提交（兜底：未移动鼠标直接点别处）
  document.addEventListener('click', () => { if (accel.open) closeAccel() })
}
function syncAccelNav(page) {
  if (!accel.root) buildAccel()
  if (accel.open) {
    // 展开期间：主名称保持不变（pending 由选项点击维护）；切到非加速页 → 收起提交
    if (!ACCEL_PAGES.has(page)) closeAccel()
    accel.root.classList.toggle('active', ACCEL_PAGES.has(page))
    return
  }
  if (ACCEL_PAGES.has(page)) accel.committed = page
  paintAccelMain(accel.committed)
  accel.root.classList.toggle('active', ACCEL_PAGES.has(page))
  for (const [p, opt] of Object.entries(accel.opts)) opt.classList.toggle('current', p === accel.committed)
}

// ---------- 右键菜单（方案 A：屏蔽 WebView2 浏览器菜单，保留文本区复制粘贴） ----------
document.addEventListener('contextmenu', (e) => {
  const t = e.target
  const tag = t && t.tagName ? t.tagName.toLowerCase() : ''
  // 输入框 / 文本域 / 可选中日志区 → 放行（需要粘贴与复制）
  const editable = t && (t.isContentEditable || tag === 'input' || tag === 'textarea' || tag === 'select')
  const selectable = !!(t && t.closest && t.closest('#logBox'))
  if (!editable && !selectable) e.preventDefault()
})

// ---------- 配色方案（CFW 亮色 / 深色 / 跟随系统） ----------
const THEME_KEY = 'yunting-theme'
let themeQuery = null
function resolveTheme(pref) {
  if (pref === 'dark') return 'dark'
  if (pref === 'light') return 'light'
  return themeQuery && themeQuery.matches ? 'dark' : 'light'
}
function applyTheme(pref) {
  const eff = resolveTheme(pref)
  document.documentElement.dataset.theme = eff
  document.documentElement.style.colorScheme = eff
  return eff
}
function initTheme() {
  themeQuery = window.matchMedia('(prefers-color-scheme: dark)')
  const mqChange = () => { if (($('setTheme') && $('setTheme').value) === 'system') applyTheme('system') }
  if (typeof themeQuery.addEventListener === 'function') themeQuery.addEventListener('change', mqChange)
  else if (typeof themeQuery.addListener === 'function') themeQuery.addListener(mqChange)
  let pref = 'light'
  try { pref = localStorage.getItem(THEME_KEY) || 'light' } catch (e) { /* 存储不可用则默认亮色 */ }
  if (!['light', 'dark', 'system'].includes(pref)) pref = 'light'
  $('setTheme').value = pref
  applyTheme(pref)
}
$('setTheme').onchange = () => {
  const pref = $('setTheme').value
  try { localStorage.setItem(THEME_KEY, pref) } catch (e) { /* 忽略 */ }
  applyTheme(pref)
}
initTheme()

// ---------- 连接计时（CFW 底部 00:00:03 样式） ----------
let connSince = 0
const DASH_TIMER = '--<span class="colon">:</span>--<span class="colon">:</span>--'
function renderTimer() {
  const el = $('connTimer')
  if (!el) return
  const running = !!(state.core && state.core.running)
  if (!running) { connSince = 0; el.innerHTML = DASH_TIMER; return }
  if (!connSince) connSince = Date.now()
  const secs = Math.floor((Date.now() - connSince) / 1000)
  const p = (n) => String(n).padStart(2, '0')
  el.innerHTML = `${p(Math.floor(secs / 3600))}<span class="colon">:</span>${p(Math.floor(secs / 60) % 60)}<span class="colon">:</span>${p(secs % 60)}`
}
setInterval(renderTimer, 1000)

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
      try { await deleteNodeWithUndo(n.tag) }
      catch (err) {
        const m = errText(err)
        toast(m, true)
        log(`[gui] 删除节点失败: ${m}`)
      }
    }
    const detail = document.createElement('button')
    detail.className = 'detail' + (state.drawerTag === n.tag ? ' active' : '')
    detail.title = '查看节点详情'
    detail.innerHTML = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7-10-7-10-7Z"/><circle cx="12" cy="12" r="3"/></svg>'
    detail.onclick = (e) => {
      e.stopPropagation()
      openNodeDrawer(n)
    }
    const fill = document.createElement('span')
    fill.className = 'fill'
    row.append(type, tag, detail, fill, delay, del)
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

// ---------- 节点详情抽屉（右侧滑出） ----------
const ICON_COPY = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>'
let copyResetTimer = null
function kvRows(pairs) {
  const grid = document.createElement('div')
  grid.className = 'kv-grid'
  for (const [k, v] of pairs) {
    if (v === undefined || v === null || v === '') continue
    const kk = document.createElement('span'); kk.className = 'k'; kk.textContent = k
    const vv = document.createElement('span'); vv.className = 'v'; vv.textContent = String(v)
    grid.append(kk, vv)
  }
  return grid
}
function buildNodeDetail(n) {
  const rows = [['协议类型', n.type], ['服务器', n.server], ['端口', n.server_port]]
  if (n.uuid) rows.push(['UUID', n.uuid])
  if (n.password) rows.push(['密码', n.password])
  if (n.method) rows.push(['加密算法', n.method])
  if (n.flow) rows.push(['Flow', n.flow])
  if (n.security && n.type === 'vmess') rows.push(['VMess 加密', n.security])
  if (n.alter_id) rows.push(['AlterId', n.alter_id])
  const tls = n.tls || {}
  if (tls.enabled) {
    rows.push(['TLS', '开启'])
    if (tls.server_name) rows.push(['SNI', tls.server_name])
    if (tls.alpn && tls.alpn.length) rows.push(['ALPN', tls.alpn.join(', ')])
    if (tls.insecure) rows.push(['跳过证书校验', '是'])
    if (tls.utls && tls.utls.fingerprint) rows.push(['UTLS 指纹', tls.utls.fingerprint])
    if (tls.reality && tls.reality.enabled) {
      rows.push(['REALITY', '开启'])
      if (tls.reality.public_key) rows.push(['Public Key', tls.reality.public_key])
      if (tls.reality.short_id) rows.push(['Short ID', tls.reality.short_id || '(空)'])
    }
  }
  const tr = n.transport || {}
  if (tr.type && tr.type !== 'tcp') {
    rows.push(['传输层', tr.type])
    if (tr.path) rows.push(['Path', tr.path])
    if (tr.service_name) rows.push(['Service Name', tr.service_name])
    if (tr.headers && tr.headers.Host) rows.push(['Host', tr.headers.Host])
  } else if (tr.type === 'tcp') {
    rows.push(['传输层', 'tcp'])
  }
  if (n.obfs && n.obfs.type) {
    rows.push(['Obfs', n.obfs.type])
    if (n.obfs.password) rows.push(['Obfs 密码', n.obfs.password])
  }
  if (n.subId) rows.push(['来源', '订阅节点'])
  return rows
}
function openNodeDrawer(n) {
  const wasOpen = $('nodeDrawer').classList.contains('show')
  state.drawerTag = n.tag
  renderNodes()
  renderDrawer(n)
  $('nodeDrawer').classList.add('show')
  // 已打开时切换节点 → 内容淡入滑动动效，不关再开；不抢占焦点（不 focus 任何控件）
  if (wasOpen && n.tag !== (state.drawerShown || '')) {
    const body = $('drawerBody')
    body.classList.remove('swap')
    void body.offsetWidth // 重启动画
    body.classList.add('swap')
  }
  state.drawerShown = n.tag
}
function renderDrawer(n) {
  $('drawerTitle').textContent = '节点详情'
  const body = $('drawerBody')
  body.innerHTML = ''
  // --- 显示名称（唯一可编辑项） ---
  const secName = document.createElement('div')
  secName.className = 'drawer-section'
  const tn = document.createElement('div')
  tn.className = 'sec-title'
  tn.textContent = '显示名称'
  const row = document.createElement('div')
  row.className = 'rename-row'
  const nameInput = document.createElement('input')
  nameInput.type = 'text'
  nameInput.value = n.tag
  nameInput.placeholder = '节点显示名称'
  const saveBtn = document.createElement('button')
  saveBtn.className = 'primary'
  saveBtn.textContent = '保存'
  const applyRename = async () => {
    const v = nameInput.value.trim()
    if (!v) return toast('名称不能为空', true)
    if (v === n.tag) return toast('名称未变化')
    saveBtn.disabled = true
    try {
      await api.renameNode(n.tag, v)
      if (state.selectedTag === n.tag) state.selectedTag = v
      delete state.delays[n.tag]
      await refreshNodes()
      await reloadCoreAfterNodeChange()
      const fresh = state.nodes.find((x) => x.tag === v)
      if (fresh) {
        state.drawerTag = v
        renderDrawer(fresh)
        renderNodes()
      }
      toast(`名称已改为「${v}」`)
      log(`[gui] 节点重命名 ${n.tag} → ${v}`)
    } catch (e) {
      const m = errText(e)
      toast(m, true)
      log(`[gui] 节点重命名失败: ${m}`)
    } finally {
      saveBtn.disabled = false
    }
  }
  saveBtn.onclick = applyRename
  nameInput.onkeydown = (e) => { if (e.key === 'Enter') applyRename() }
  row.append(nameInput, saveBtn)
  secName.append(tn, row)
  // --- 配置信息（只读） ---
  const sec1 = document.createElement('div')
  sec1.className = 'drawer-section'
  const t1 = document.createElement('div')
  t1.className = 'sec-title'
  t1.textContent = '配置信息'
  sec1.append(t1, kvRows(buildNodeDetail(n)))
  // --- 分享链接 + 复制 ---
  const link = api.nodeToLink(n) || `${n.server}:${n.server_port}`
  const sec2 = document.createElement('div')
  sec2.className = 'drawer-section'
  const t2 = document.createElement('div')
  t2.className = 'sec-title'
  t2.textContent = '分享链接'
  const box = document.createElement('div')
  box.className = 'link-box'
  box.style.userSelect = 'text'
  box.textContent = link
  const copyBtn = document.createElement('button')
  copyBtn.className = 'copy-btn'
  copyBtn.title = '复制链接'
  copyBtn.innerHTML =
    '<span class="ic-copy">' + ICON_COPY + '</span>' +
    '<span class="fx"><svg viewBox="0 0 24 24">' +
    '<circle class="ring" cx="12" cy="12" r="9" transform="rotate(-90 12 12)"/>' +
    '<path class="chk" d="M7.5 12.5l3 3 6-6.5"/>' +
    '</svg></span>'
  copyBtn.onclick = async (e) => {
    e.stopPropagation()
    if (copyBtn.classList.contains('busy') || copyBtn.classList.contains('done')) return
    let ok = false
    try {
      await navigator.clipboard.writeText(link)
      ok = true
    } catch (err) {
      // WebView2 剪贴板 API 不可用时回退 execCommand
      try {
        const ta = document.createElement('textarea')
        ta.value = link
        ta.style.cssText = 'position:fixed;left:-9999px'
        document.body.append(ta)
        ta.select()
        ok = document.execCommand('copy')
        ta.remove()
      } catch (e2) { ok = false }
    }
    if (!ok) { toast('复制失败', true); return }
    // 漩涡旋转 → 变对勾
    copyBtn.classList.add('busy')
    clearTimeout(copyResetTimer)
    setTimeout(() => {
      copyBtn.classList.remove('busy')
      copyBtn.classList.add('done')
    }, 500)
    copyResetTimer = setTimeout(() => {
      copyBtn.classList.remove('done')
    }, 500 + 2500)
  }
  box.append(copyBtn)
  sec2.append(t2, box)
  body.append(secName, sec1, sec2)
  // --- 底部：靠左红色删除 ---
  const foot = $('drawerFoot')
  foot.innerHTML = ''
  const delBtn = document.createElement('button')
  delBtn.className = 'btn-danger'
  delBtn.textContent = '删除该节点'
  delBtn.onclick = async () => {
    delBtn.disabled = true
    try {
      closeNodeDrawer()
      await deleteNodeWithUndo(n.tag)
      log(`[gui] 抽屉删除节点 ${n.tag}`)
    } catch (e) {
      const m = errText(e)
      toast(m, true)
      log(`[gui] 删除节点失败: ${m}`)
      delBtn.disabled = false
    }
  }
  foot.append(delBtn)
}
function closeNodeDrawer() {
  state.drawerTag = null
  state.drawerShown = null
  $('nodeDrawer').classList.remove('show')
  renderNodes()
}
$('drawerClose').onclick = closeNodeDrawer
document.addEventListener('keydown', (e) => { if (e.key === 'Escape' && state.drawerTag) closeNodeDrawer() })

// ---------- 撤销条（5 秒可撤销；删除节点 / 清空白名单共用） ----------
let undoTimer = null
function showUndoBar(msg, onUndo) {
  const bar = $('undoBar')
  $('undoMsg').textContent = msg
  bar.classList.remove('show')
  void bar.offsetWidth // 重启进度条动画
  bar.classList.add('show')
  clearTimeout(undoTimer)
  const hide = () => { bar.classList.remove('show'); $('undoBtn').onclick = null }
  $('undoBtn').onclick = () => { clearTimeout(undoTimer); hide(); onUndo() }
  undoTimer = setTimeout(hide, 5000)
}

// 删除一个节点并给 5 秒撤销（把被删节点原样写回）
async function deleteNodeWithUndo(tag) {
  const removed = state.nodes.find((x) => x.tag === tag)
  if (!removed) return
  await api.removeNode(tag)
  await refreshNodes()
  await reloadCoreAfterNodeChange()
  showUndoBar(`已删除节点「${tag}」`, async () => {
    try {
      await api.addNode(removed)
      await refreshNodes()
      await reloadCoreAfterNodeChange()
      toast('已撤销删除')
    } catch (e) {
      const m = errText(e)
      toast(`撤销失败: ${m}`, true)
      log(`[gui] 撤销删除节点失败: ${m}`)
    }
  })
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
  const proxyOn = !!(state.proxy && state.proxy.enable)
  // 导航底部状态：绿点与文字只跟随内核（核心）状态，不跟随系统代理连接状态
  // （内核自启动后即可看到 ● 核心已启动，无需等开系统代理）
  $('statusDot').className = 'status-dot' + (running ? ' on' : '')
  $('statusText').textContent = running ? '核心已启动' : '核心未启动'
  // VPN 连接按钮: 内核在跑且系统代理已开 → 显示断开; 否则显示一键连接
  // （内核自启动后点"一键连接"只补开系统代理，不会重启内核）
  const connected = running && proxyOn
  $('btnConnect').style.display = connected ? 'none' : ''
  $('btnDisconnect').style.display = connected ? '' : 'none'
  $('btnTestAll').disabled = !!state.reloading
  $('btnTestAll').title = state.reloading ? '节点列表更新中，请稍候' : ''
  // 设置页系统代理状态：开关反映是否开启，右侧文字显示详情
  if (state.proxy) {
    const chk = $('chkSysProxy')
    if (chk) chk.checked = !!state.proxy.enable
    const t = $('proxyStateText')
    if (t) {
      if (state.proxy.enable) {
        t.textContent = state.proxy.ours ? state.proxy.server : `${state.proxy.server}（他程序设置）`
      } else {
        t.textContent = ''
      }
    }
  }
  renderTimer()
}

async function reloadCoreAfterNodeChange() {
  if (!(state.core && state.core.running)) return
  state.reloading = true
  renderState()
  log('[gui] 节点列表已更新，正在重载内核…')
  try {
    // 走 restart（停旧起新）—— disconnect 现在只撤系统代理，不再停内核
    await api.restart({ selectedTag: state.selectedTag })
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
    const proxyOn = state.proxy && state.proxy.enable
    toast(proxyOn ? '已连接' : '已就绪（内核运行中）')
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
  $('setCloseBehavior').value = state.settings.closeBehavior || 'quit'
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
  const clearRow = $('directClearRow')
  if (clearRow) clearRow.style.display = list.length > 0 ? '' : 'none'
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
// 一键清空（5 秒内可撤销）
$('btnDirectClear').onclick = async () => {
  const removed = [...(state.settings.directList || [])]
  if (removed.length === 0) return
  try {
    await saveSettings({ directList: [] })
    showUndoBar(`已清空直连白名单（${removed.length} 项）`, async () => {
      try {
        const cur = state.settings.directList || []
        await saveSettings({ directList: [...new Set([...cur, ...removed])] })
        toast('已撤销清空')
      } catch (e) {
        toast(`撤销失败: ${errText(e)}`, true)
      }
    })
    log(`[gui] 清空直连白名单: ${removed.join(', ')}`)
  } catch (e) {
    toast(errText(e), true)
  }
}
// 回车直接加入（与点击「加入」等效）
$('directAddInput').onkeydown = (e) => {
  if (e.key === 'Enter') {
    e.preventDefault()
    $('btnDirectAdd').click()
  }
}
// 反浏览器自动填充（Chromium 无视 autocomplete=off，用 readonly-until-focus 阻断"保存的信息"下拉）
;(function guardDirectInput() {
  const el = $('directAddInput')
  el.setAttribute('readonly', 'readonly')
  el.addEventListener('focus', () => el.removeAttribute('readonly'))
  el.addEventListener('blur', () => { if (!el.value) el.setAttribute('readonly', 'readonly') })
})()

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
      closeBehavior: $('setCloseBehavior').value || 'quit',
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

$('chkSysProxy').onchange = async (e) => {
  const on = !!e.target.checked
  try { renderState({ proxy: await api.setSystemProxy(on) }) }
  catch (err) {
    const m = errText(err)
    toast(m, true)
    log(`[gui] 系统代理失败: ${m}`)
    e.target.checked = !on // 失败回滚开关状态
    try { renderState({ proxy: await api.getSystemProxy() }) } catch (e2) { /* ignore */ }
  }
}

$('btnClearLog').onclick = () => { $('logBox').innerHTML = '' }

$('setPort').onchange = () => saveSettings()
$('setApiPort').onchange = () => saveSettings()
$('chkAutoProxy').onchange = () => saveSettings()
$('setRouteMode').onchange = () => { updateRouteModeHint(); saveSettings() }
$('setCloseBehavior').onchange = () => saveSettings()

// ---------- 主进程事件 ----------
api.onLog(log)
api.onState(renderState)

// ---------- 关闭行为询问对话框 ----------
function closeWith(behavior, remember) {
  if (remember) {
    // 记住选择：更新后端设置（closeBehavior 改为 tray/quit，之后点 × 不再询问）
    const settings = state.settings || {}
    saveSettings({ ...settings, closeBehavior: behavior })
      .then(() => {
        if (behavior === 'tray') { $('dlgClose').close(); api.hideWindow(); log('[gui] 已记住：点击 × 最小化到托盘') }
        else { $('dlgClose').close(); api.quitApp() }
      })
      .catch((e) => { toast(errText(e), true); })
  } else {
    $('dlgClose').close()
    if (behavior === 'tray') { api.hideWindow(); log('[gui] 已最小化到托盘（未记住）') }
    else { api.quitApp() }
  }
}
api.onCloseAsked(() => {
  if (!$('dlgClose').open) $('dlgClose').showModal()
})
$('btnCloseTray').onclick = () => closeWith('tray', $('chkRememberClose').checked)
$('btnCloseQuit').onclick = () => closeWith('quit', $('chkRememberClose').checked)
$('dlgClose').addEventListener('cancel', (e) => {
  // Esc 关闭对话框 = 取消操作（保持运行、窗口可见）
  e.preventDefault()
})

// ---------- 初始化 ----------
;(async function init() {
  syncAccelNav('vpn')
  log('[gui] yunting 启动')
  if (!api) {
    log('[gui] ⚠️ bridge 未就绪（window.yunting 缺失）—— 检查 tauri withGlobalTauri')
    return
  }
  await loadSettings()
  await Promise.all([refreshNodes(), refreshSubs(), renderState(await api.getState())])
})()
