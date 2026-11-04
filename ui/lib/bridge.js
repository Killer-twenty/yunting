// yunting Tauri 版 bridge —— 暴露与旧版 preload.js 完全一致的 window.yunting 接口
// renderer.js 零改动复用；所有调用转发到 Tauri invoke / event。
// 纯函数（解析/config生成）在本文件内本地执行（parsers.js/configBuilder.js 原样加载），
// 拉订阅由 Rust 侧 fetch_text 完成后再回前端解析（订阅文本 → parse → 传节点给 Rust）。
(function () {
  // Tauri 2 需开启 withGlobalTauri 才有 window.__TAURI__（tauri.conf.json app.withGlobalTauri: true）
  if (!window.__TAURI__) {
    console.error('[bridge] window.__TAURI__ 缺失 —— tauri.conf.json 未开 withGlobalTauri')
    document.addEventListener('DOMContentLoaded', () => { document.title = 'yunting (bridge 失败: __TAURI__ 缺失)' })
    return
  }
  const { invoke } = window.__TAURI__.core
  const { listen } = window.__TAURI__.event

  // ---- 加载纯函数模块（与旧版同一份文件）----
  let parsers
  try {
    parsers = __loadModule('lib/parsers.js')
  } catch (e) {
    console.error('[bridge] parsers.js 加载失败:', e)
    document.title = 'yunting (parsers 加载失败)'
    throw e
  }
  if (!parsers || typeof parsers.parseShareLink !== 'function') {
    document.title = 'yunting (parsers 接口异常)'
    throw new Error('parsers 接口异常')
  }
  // configBuilder 仅在需要时由 Rust 侧生成 config；前端不需要加载它
  // （configBuilder.js 的 require('./paths') 链在浏览器态可用，但 Rust 已实现等价逻辑）

  const bridge = {
    // ---------- 节点 ----------
    getNodes: () => invoke('nodes_get'),
    addShareLinks: async (text) => {
      const links = parsers.extractShareLinks(text)
      const parsed = links.map(parsers.parseShareLink).filter(Boolean)
      if (!parsed.length) throw new Error('文本中没有可识别的分享链接（支持 vless/vmess/trojan/ss/hysteria2）')
      return invoke('nodes_add', { nodes: parsed })
    },
    removeNode: async (tag) => { await invoke('nodes_remove', { tag }); return },
    // 撤销删除：把原节点对象写回（raw 未变，去重会命中同一节点）
    addNode: (node) => invoke('nodes_add', { nodes: [node] }),
    renameNode: (oldTag, newTag) => invoke('nodes_rename', { tag: oldTag, newTag }),
    getSubs: () => invoke('subs_get'),
    addSub: async (url, name) => {
      // 拉文本（Rust）→ 前端解析（parsers 原逻辑）→ 节点回传 Rust
      const text = await invoke('fetch_text', { url })
      const nodes = parsers.parseSubscription(text)
      if (!nodes.length) throw new Error('订阅内容解析不出节点')
      return invoke('subs_add', { url, name, nodes })
    },
    updateSub: async (id) => {
      const subs = await invoke('subs_get')
      const sub = subs.find((s) => s.id === id)
      if (!sub) throw new Error('订阅不存在')
      const text = await invoke('fetch_text', { url: sub.url })
      const nodes = parsers.parseSubscription(text)
      if (!nodes.length) throw new Error('订阅更新后解析不出节点')
      return invoke('subs_update', { id, nodes })
    },
    removeSub: async (id) => { await invoke('subs_remove', { id }); return },

    // ---------- 连接 ----------
    connect: (profile) => invoke('core_connect', { selectedTag: (profile && profile.selectedTag) || null }),
    // 仅撤销系统代理，内核保持运行（内核生命周期跟随软件）
    disconnect: () => invoke('core_disconnect'),
    // 节点变更后重载内核配置
    restart: (profile) => invoke('core_restart', { selectedTag: (profile && profile.selectedTag) || null }),
    getState: () => invoke('core_state'),
    testDelay: (tag) => invoke('core_delay', { tag }),

    // ---------- 系统代理 ----------
    setSystemProxy: (on) => invoke('sysproxy_set', { on }),
    getSystemProxy: () => invoke('sysproxy_get'),

    // ---------- 设置/事件 ----------
    getSettings: () => invoke('settings_get'),
    saveSettings: (s) => invoke('settings_save', { s }),
    getLogTail: () => invoke('logs_tail'),
    onLog: (cb) => listen('log', (e) => cb(e.payload)),
    onState: (cb) => listen('state', (e) => cb(e.payload)),

    // ---------- 托盘/窗口 ----------
    quitApp: () => invoke('app_quit'),
    showWindow: () => invoke('window_show'),
    hideWindow: () => invoke('window_hide'),
    onCloseAsked: (cb) => listen('close-asked', () => cb()),

    // ---------- 节点详情（纯函数，前端本地执行） ----------
    nodeToLink: (node) => parsers.nodeToShareLink(node),
  }

  window.yunting = bridge
  window.__bridgeReady = true
  console.log('[bridge] window.yunting 就绪')
})()
