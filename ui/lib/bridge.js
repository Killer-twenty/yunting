// 云亭 Tauri 版 bridge —— 暴露与旧版 preload.js 完全一致的 window.yunting 接口
// renderer.js 零改动复用；所有调用转发到 Tauri invoke / event。
// 纯函数（解析/config生成）在本文件内本地执行（parsers.js/configBuilder.js 原样加载），
// 拉订阅由 Rust 侧 fetch_text 完成后再回前端解析（订阅文本 → parse → 传节点给 Rust）。
(async function () {
  const { invoke } = window.__TAURI__.core
  const { listen } = window.__TAURI__.event

  // ---- 加载纯函数模块（与旧版同一份文件）----
  const parsers = __loadModule('lib/parsers.js')
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
    disconnect: () => invoke('core_disconnect'),
    getState: () => invoke('core_state'),
    testDelay: (tag) => invoke('core_delay', { tag }),

    // ---------- 系统代理 ----------
    setSystemProxy: (on) => invoke('sysproxy_set', { on }),
    getSystemProxy: () => invoke('sysproxy_get'),

    // ---------- 设置/事件 ----------
    getSettings: () => invoke('settings_get'),
    saveSettings: (s) => invoke('settings_save', { s }),
    onLog: (cb) => listen('log', (e) => cb(e.payload)),
    onState: (cb) => listen('state', (e) => cb(e.payload)),
  }

  window.yunting = bridge
  window.__bridgeReady = true
})().catch((e) => {
  console.error('[bridge] init failed:', e)
  document.title = '云亭 (bridge 错误)'
})
