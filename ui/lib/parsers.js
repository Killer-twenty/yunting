// 节点与订阅解析 —— 纯函数，无副作用
// 支持: vless:// vmess:// trojan:// ss:// hysteria2://(hy2://) 分享链接
//       base64 订阅（链接列表）、clash yaml 订阅

/**
 * 统一节点对象:
 * {
 *   tag: string,          // 唯一标识（默认 host:port 或 name）
 *   type: 'vless'|'vmess'|'trojan'|'ss'|'hysteria2',
 *   server, server_port,
 *   ...协议特定字段
 *   raw: string           // 原始链接（订阅更新去重用）
 * }
 */

function safeDecodeURIComponent(s) {
  try { return decodeURIComponent(s) } catch { return s }
}

function tryJsonParse(s) {
  try { return JSON.parse(s) } catch { return null }
}

// ---------- URL 解析基座：把 uri 解析成 { host, port, userinfo, params, name } ----------
function parseUri(uri) {
  // 自定义解析，避免 URL 类对非标准 scheme 的兼容问题
  const m = uri.match(/^([a-z0-9+.-]+):\/\/(.*)$/is)
  if (!m) return null
  const scheme = m[1].toLowerCase()
  let rest = m[2]
  let name = ''
  const hashIdx = rest.indexOf('#')
  if (hashIdx >= 0) {
    name = safeDecodeURIComponent(rest.slice(hashIdx + 1))
    rest = rest.slice(0, hashIdx)
  }
  const params = {}
  const qIdx = rest.indexOf('?')
  if (qIdx >= 0) {
    const qs = rest.slice(qIdx + 1)
    rest = rest.slice(0, qIdx)
    for (const kv of qs.split('&')) {
      if (!kv) continue
      const eq = kv.indexOf('=')
      if (eq < 0) params[kv] = ''
      else params[kv.slice(0, eq)] = safeDecodeURIComponent(kv.slice(eq + 1))
    }
  }
  // rest = userinfo@host:port
  let userinfo = ''
  const atIdx = rest.lastIndexOf('@')
  if (atIdx >= 0) {
    userinfo = rest.slice(0, atIdx)
    rest = rest.slice(atIdx + 1)
  }
  // 处理 [IPv6]:port
  let host = ''
  let port = 0
  const v6 = rest.match(/^\[(.+)\]:(\d+)$/)
  if (v6) { host = v6[1]; port = parseInt(v6[2], 10) }
  else {
    const cIdx = rest.lastIndexOf(':')
    if (cIdx < 0) return null
    host = rest.slice(0, cIdx)
    port = parseInt(rest.slice(cIdx + 1), 10)
  }
  if (!host || !port) return null
  return { scheme, userinfo, host, port, params, name }
}

// TLS/Reality 公共字段提取
function tlsFields(params, defaults = {}) {
  const security = (params.security || defaults.security || '').toLowerCase()
  const out = {}
  if (security === 'tls' || security === 'reality' || params.sni || params.pbk) {
    out.tls = { enabled: true }
    if (security === 'reality') {
      out.tls.utls = { enabled: true, fingerprint: params.fp || 'chrome' }
      out.tls.reality = {
        enabled: true,
        public_key: params.pbk || '',
        short_id: params.sid || '',
      }
    } else {
      if (params.fp) out.tls.utls = { enabled: true, fingerprint: params.fp }
    }
    if (params.sni) out.tls.server_name = params.sni
    if (params.alpn) out.tls.alpn = params.alpn.split(',').filter(Boolean)
    if (params.insecure === '1' || params.allowInsecure === 'true' || params.allowInsecure === '1') {
      out.tls.insecure = true
    }
  }
  return out
}

// 传输层公共字段（ws/grpc/vision-tcp）
function transportFields(params) {
  const type = (params.type || 'tcp').toLowerCase()
  if (type === 'ws') {
    const t = { type: 'ws' }
    if (params.path) t.path = params.path
    const hostHeader = params.host
    if (hostHeader) t.headers = { Host: hostHeader }
    if (params.ed || (params.path && params.path.includes('ed=256'))) t['early_data_header_name'] = 'Sec-WebSocket-Protocol'
    return { transport: t }
  }
  if (type === 'grpc') {
    return { transport: { type: 'grpc', service_name: params.serviceName || params['servicename'] || '' } }
  }
  return {} // tcp / none
}

// ---------- 各协议 ----------
function parseVless(uri) {
  const u = parseUri(uri)
  if (!u) return null
  const node = {
    tag: u.name || `${u.host}:${u.port}`,
    type: 'vless',
    server: u.host,
    server_port: u.port,
    uuid: u.userinfo,
  }
  if (u.params.flow) node.flow = u.params.flow
  Object.assign(node, tlsFields(u.params))
  Object.assign(node, transportFields(u.params))
  node.raw = uri
  return node.uuid ? node : null
}

function parseVmess(uri) {
  // vmess://base64(json)
  let raw = uri.slice('vmess://'.length)
  // 兼容 URL-safe base64 与缺 padding
  raw = raw.replace(/-/g, '+').replace(/_/g, '/')
  while (raw.length % 4) raw += '='
  let json
  try { json = JSON.parse(Buffer.from(raw, 'base64').toString('utf-8')) } catch { return null }
  if (!json || !json.add || !json.id) return null
  const port = parseInt(json.port, 10)
  if (!port) return null
  const node = {
    tag: (json.ps && String(json.ps)) || `${json.add}:${port}`,
    type: 'vmess',
    server: String(json.add),
    server_port: port,
    uuid: String(json.id),
    security: json.scy || 'auto',
  }
  if (String(json.aid ?? '0') !== '0') node.alter_id = parseInt(json.aid, 10)
  const params = {
    type: json.net || 'tcp',
    security: json.tls === 'tls' ? 'tls' : '',
    sni: json.sni || json.host || '',
    host: json.host || '',
    path: json.path || '',
    serviceName: json.path || '', // grpc 时 path 即 serviceName
    fp: json.fp || '',
    alpn: json.alpn || '',
  }
  if (json.tls === 'tls' || json.sni) Object.assign(node, tlsFields(params))
  Object.assign(node, transportFields(params))
  node.raw = uri
  return node
}

function parseTrojan(uri) {
  const u = parseUri(uri)
  if (!u) return null
  const node = {
    tag: u.name || `${u.host}:${u.port}`,
    type: 'trojan',
    server: u.host,
    server_port: u.port,
    password: safeDecodeURIComponent(u.userinfo),
  }
  const params = { ...u.params, security: u.params.security || 'tls' } // trojan 默认 tls
  Object.assign(node, tlsFields(params))
  Object.assign(node, transportFields(u.params))
  node.raw = uri
  return node
}

function parseSS(uri) {
  // 两种形态:
  // 1) ss://base64(method:pass)@host:port#name   (SIP002)
  // 2) ss://base64(method:pass@host:port)#name   (旧式)
  let body = uri.slice('ss://'.length)
  let name = ''
  const hashIdx = body.indexOf('#')
  if (hashIdx >= 0) {
    name = safeDecodeURIComponent(body.slice(hashIdx + 1))
    body = body.slice(0, hashIdx)
  }
  let method = '', password = '', host = '', port = 0, pluginStr = ''
  const qIdx = body.indexOf('?')
  if (qIdx >= 0) {
    pluginStr = body.slice(qIdx + 1)
    body = body.slice(0, qIdx)
  }
  const atIdx = body.lastIndexOf('@')
  if (atIdx >= 0) {
    // SIP002: userinfo 是 base64(method:pass)（也可能明文）
    const userinfo = body.slice(0, atIdx)
    const hostport = body.slice(atIdx + 1)
    const cIdx = hostport.lastIndexOf(':')
    if (cIdx < 0) return null
    host = hostport.slice(0, cIdx)
    port = parseInt(hostport.slice(cIdx + 1), 10)
    let decoded = userinfo
    if (!userinfo.includes(':')) {
      try {
        let b = userinfo.replace(/-/g, '+').replace(/_/g, '/')
        while (b.length % 4) b += '='
        decoded = Buffer.from(b, 'base64').toString('utf-8')
      } catch { /* 保持原样 */ }
    }
    const mc = decoded.indexOf(':')
    if (mc < 0) return null
    method = decoded.slice(0, mc)
    password = decoded.slice(mc + 1)
  } else {
    // 旧式整体 base64
    let b = body.replace(/-/g, '+').replace(/_/g, '/')
    while (b.length % 4) b += '='
    let decoded
    try { decoded = Buffer.from(b, 'base64').toString('utf-8') } catch { return null }
    const m = decoded.match(/^(.+?):(.*)@(.+?):(\d+)$/s)
    if (!m) return null
    method = m[1]; password = m[2]; host = m[3]; port = parseInt(m[4], 10)
  }
  if (!host || !port || !method) return null
  const node = {
    tag: name || `${host}:${port}`,
    type: 'ss',
    server: host,
    server_port: port,
    method,
    password: decodePassword(password),
  }
  // SIP003 plugin: obfs-local;obfs=http;obfs-host=xxx / v2ray-plugin
  if (pluginStr.startsWith('plugin=')) {
    const plugin = safeDecodeURIComponent(pluginStr.slice('plugin='.length))
    const semi = plugin.indexOf(';')
    const pname = semi < 0 ? plugin : plugin.slice(0, semi)
    if (pname.includes('obfs')) {
      const opts = {}
      for (const kv of plugin.split(';').slice(1)) {
        const [k, v] = kv.split('=')
        if (k && v) opts[k] = v
      }
      node.plugin = 'obfs-local'
      node.plugin_opts = JSON.stringify({
        mode: opts.obfs || 'http',
        ...(opts['obfs-host'] ? { host: opts['obfs-host'] } : {}),
      })
    }
    // v2ray-plugin: 简化为 ws 传输由内核原生支持不了，先忽略（罕见）
  }
  node.raw = uri
  return node
}

function decodePassword(p) {
  // 密码可能二次 base64（少见），先原样返回
  return p
}

function parseHysteria2(uri) {
  const u = parseUri(uri)
  if (!u) return null
  const node = {
    tag: u.name || `${u.host}:${u.port}`,
    type: 'hysteria2',
    server: u.host,
    server_port: u.port,
    password: safeDecodeURIComponent(u.userinfo),
    tls: { enabled: true },
  }
  if (u.params.sni) node.tls.server_name = u.params.sni
  if (u.params.insecure === '1' || u.params.insecure === 'true') node.tls.insecure = true
  if (u.params.obfs && u.params.obfs !== 'none') {
    node.obfs = { type: u.params.obfs }
    if (u.params['obfs-password']) node.obfs.password = u.params['obfs-password']
  }
  node.raw = uri
  return node
}

/** 解析单条分享链接；不认识返回 null */
function parseShareLink(uri) {
  const s = uri.trim()
  if (/^vless:\/\//i.test(s)) return parseVless(s)
  if (/^vmess:\/\//i.test(s)) return parseVmess(s)
  if (/^trojan:\/\//i.test(s)) return parseTrojan(s)
  if (/^ss:\/\//i.test(s)) return parseSS(s)
  if (/^(hysteria2|hy2):\/\//i.test(s)) return parseHysteria2(s)
  return null
}

/** 从一段文本中提取所有可识别分享链接（按行/空白分） */
function extractShareLinks(text) {
  const out = []
  const lines = String(text || '').split(/[\r\n\s]+/)
  for (const line of lines) {
    const t = line.trim()
    if (!t) continue
    if (/^(vless|vmess|trojan|ss|hysteria2|hy2):\/\//i.test(t)) out.push(t)
  }
  return out
}

// ---------- base64 订阅 ----------
function parseBase64Subscription(text) {
  let t = String(text || '').trim()
  // 尝试整体 base64 解码（可能带 data: 前缀）
  t = t.replace(/^data:[^,]*,/, '')
  let decoded = null
  try {
    let b = t.replace(/-/g, '+').replace(/_/g, '/').replace(/\s+/g, '')
    while (b.length % 4) b += '='
    const attempt = Buffer.from(b, 'base64').toString('utf-8')
    // 解出来应当包含 scheme://
    if (/(vless|vmess|trojan|ss|hysteria2|hy2):\/\//i.test(attempt)) decoded = attempt
  } catch { /* fallthrough */ }
  const src = decoded || t
  return extractShareLinks(src).map(parseShareLink).filter(Boolean)
}

// ---------- clash yaml 订阅 ----------
function parseClashSubscription(yamlText) {
  const yaml = require('js-yaml')
  let doc
  try { doc = yaml.load(yamlText) } catch { return [] }
  if (!doc || !Array.isArray(doc.proxies)) return []
  const out = []
  for (const p of doc.proxies) {
    if (!p || !p.server || !p.port) continue
    const node = clashProxyToNode(p)
    if (node) out.push(node)
  }
  return out
}

function clashProxyToNode(p) {
  const port = parseInt(p.port, 10)
  if (!port) return null
  const tag = p.name || `${p.server}:${port}`
  const base = { tag, server: String(p.server), server_port: port }

  function tlsOf(p) {
    if (!p.tls && !p.sni && p.type !== 'trojan' && p.type !== 'hysteria2') return {}
    const tls = { enabled: true }
    if (p.sni) tls.server_name = p.sni
    if (p['skip-cert-verify']) tls.insecure = true
    if (p.alpn) tls.alpn = Array.isArray(p.alpn) ? p.alpn : String(p.alpn).split(',')
    if (p['client-fingerprint']) tls.utls = { enabled: true, fingerprint: p['client-fingerprint'] }
    if (p['reality-opts']) {
      if (!tls.utls) tls.utls = { enabled: true, fingerprint: 'chrome' }
      tls.reality = {
        enabled: true,
        public_key: p['reality-opts']['public-key'] || '',
        short_id: p['reality-opts']['short-id'] || '',
      }
    }
    return Object.keys(tls).length > 1 ? { tls } : {}
  }

  function wsOf(p) {
    if (p.network === 'ws') {
      const t = { type: 'ws' }
      if (p['ws-opts'] && p['ws-opts'].path) t.path = String(p['ws-opts'].path)
      if (p['ws-opts'] && p['ws-opts'].headers && p['ws-opts'].headers.Host) t.headers = { Host: p['ws-opts'].headers.Host }
      return { transport: t }
    }
    if (p.network === 'grpc') {
      const sn = p['grpc-opts'] && p['grpc-opts']['grpc-service-name']
      return { transport: { type: 'grpc', service_name: sn || '' } }
    }
    return {}
  }

  switch (p.type) {
    case 'ss':
      return { ...base, type: 'ss', method: p.cipher, password: String(p.password ?? '') }
    case 'vmess':
      return {
        ...base, type: 'vmess', uuid: String(p.uuid || ''),
        security: p.cipher || 'auto',
        ...(parseInt(p.alterId ?? p['alter-id'] ?? '0', 10) ? { alter_id: parseInt(p.alterId ?? p['alter-id'], 10) } : {}),
        ...tlsOf(p), ...wsOf(p),
      }
    case 'vless':
      return {
        ...base, type: 'vless', uuid: String(p.uuid || ''),
        ...(p.flow ? { flow: p.flow } : {}),
        ...tlsOf(p), ...wsOf(p),
      }
    case 'trojan':
      return { ...base, type: 'trojan', password: String(p.password ?? ''), ...tlsOf(p), ...wsOf(p) }
    case 'hysteria2':
      return {
        ...base, type: 'hysteria2', password: String(p.password ?? ''),
        tls: {
          enabled: true,
          ...(p.sni ? { server_name: p.sni } : {}),
          ...(p['skip-cert-verify'] ? { insecure: true } : {}),
        },
        ...(p.obfs ? { obfs: { type: p.obfs, ...(p['obfs-password'] ? { password: p['obfs-password'] } : {}) } } : {}),
      }
    default:
      return null // http/socks5/其他暂不支持
  }
}

/** 订阅内容自动识别：clash yaml 优先（proxies: 关键字），否则 base64/明文链接列表 */
function parseSubscription(text) {
  const t = String(text || '').trim()
  if (/^proxies\s*:/m.test(t) && /type\s*:\s*(ss|vmess|vless|trojan|hysteria2)/m.test(t)) {
    return parseClashSubscription(t)
  }
  return parseBase64Subscription(t)
}

/** 节点去重：raw 相同或 type+server+port 相同均视为同一节点 */
function dedupeNodes(nodes) {
  const seenRaw = new Set()
  const seenNode = new Set()
  const out = []
  for (const n of nodes) {
    if (n.raw && seenRaw.has(n.raw)) continue
    const key = `${n.type}|${n.server}|${n.server_port}`
    if (seenNode.has(key)) continue
    if (n.raw) seenRaw.add(n.raw)
    seenNode.add(key)
    out.push(n)
  }
  return out
}

module.exports = {
  parseShareLink,
  extractShareLinks,
  parseBase64Subscription,
  parseClashSubscription,
  parseSubscription,
  dedupeNodes,
  // 内部函数导出供单测
  _internal: { parseUri, tlsFields, transportFields, clashProxyToNode },
}
