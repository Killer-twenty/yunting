// sing-box config 生成器 —— 纯函数：输入 profile 对象，输出完整 config
// 官方文档: https://sing-box.sagernet.org/configuration/
//
// profile 形态（当前只支持 'proxy' 类型，将来游戏模式加 'tun' 分支）:
// {
//   type: 'proxy',
//   nodes: [node...],          // parsers 输出的节点对象
//   selectedTag: 'node-tag',   // 当前选中节点（selector 默认值）
//   mixedPort: 20808,
//   clashApiPort: 9090,
// }

const path = require('path')
const fs = require('fs')
const { kernelDir } = require('./paths')

// local rule-set 相对 kernelDir；sing-box 以 -D dataDir 运行，需要绝对路径或正确相对路径
function ruleSetFile(name) {
  return path.join(kernelDir(), `${name}.srs`)
}

function buildConfig(profile) {
  if (!profile || !Array.isArray(profile.nodes) || profile.nodes.length === 0) {
    throw new Error('profile.nodes 不能为空')
  }
  const mixedPort = profile.mixedPort || 20808
  const clashApiPort = profile.clashApiPort || 9090
  const mode = profile.mode || 'rule'

  const outbounds = []
  const proxyTags = []

  for (const node of profile.nodes) {
    const ob = nodeToOutbound(node)
    if (!ob) continue
    outbounds.push(ob)
    proxyTags.push(ob.tag)
  }
  if (proxyTags.length === 0) throw new Error('没有可用的出站节点')

  // 选中节点；不指定或不在列表中则取第一个
  const selected = proxyTags.includes(profile.selectedTag) ? profile.selectedTag : proxyTags[0]

  // selector（手动选择）+ urltest（自动测速）
  outbounds.push({
    type: 'selector',
    tag: 'proxy',
    outbounds: ['auto', ...proxyTags],
    default: selected,
    interrupt_exist_connections: false,
  })
  outbounds.push({
    type: 'urltest',
    tag: 'auto',
    outbounds: proxyTags,
    url: 'https://www.gstatic.com/generate_204',
    interval: '5m',
    tolerance: 50,
    idle_timeout: '30m',
  })
  outbounds.push({ type: 'direct', tag: 'direct' })
  outbounds.push({ type: 'block', tag: 'block' })

  const config = {
    log: { level: 'info', timestamp: true },
    dns: buildDns(mode),
    inbounds: buildInbounds(profile, mixedPort),
    outbounds,
    route: buildRoute(mode, profile.directList || []),
    experimental: {
      clash_api: {
        external_controller: `127.0.0.1:${clashApiPort}`,
        default_mode: mode === 'global' ? 'global' : 'rule',
      },
      cache_file: { enabled: true },
    },
  }
  return config
}

// ---------- inbounds：默认只生成 mixed；将来 TUN 模式在此加分支 ----------
function buildInbounds(profile, mixedPort) {
  const inbounds = [
    {
      type: 'mixed',
      tag: 'mixed-in',
      listen: '127.0.0.1',
      listen_port: mixedPort,
    },
  ]
  if (profile.type === 'tun') {
    // M6+ 游戏模式预留：TUN 入站（需要管理员权限 + wintun）
    inbounds.push({
      type: 'tun',
      tag: 'tun-in',
      interface_name: 'yunting',
      address: ['172.19.0.1/30', 'fdfe:dcba:9876::1/126'],
      mtu: 9000,
      auto_route: true,
      strict_route: true,
      stack: 'mixed',
    })
  }
  return inbounds
}

// ---------- dns：按 mode 分流解析 ----------
function buildDns(mode = 'rule') {
  if (mode === 'global') {
    // 全局：所有解析走代理 DNS，防止国内 DNS 污染国外域名
    return {
      servers: [
        { tag: 'local-dns', address: '223.5.5.5', detour: 'direct' },
        { tag: 'proxy-dns', address: 'https://1.1.1.1/dns-query', detour: 'proxy' },
      ],
      final: 'proxy-dns',
      strategy: 'prefer_ipv4',
    }
  }
  return {
    servers: [
      { tag: 'local-dns', address: '223.5.5.5', detour: 'direct' },
      { tag: 'proxy-dns', address: 'https://1.1.1.1/dns-query', detour: 'proxy' },
    ],
    rules: [
      { rule_set: 'geosite-cn', server: 'local-dns' },
      { clash_mode: 'Global', server: 'proxy-dns' },
    ],
    final: 'proxy-dns',
    strategy: 'prefer_ipv4',
  }
}

// ---------- route：按 mode 生成分流 ----------
// mode: 'rule'(默认, 国内直连+白名单直连+国外代理) | 'global'(全部走代理)
// directList: 用户自定义直连域名列表（后缀匹配，如 "openai.com" 覆盖所有子域）
function buildRoute(mode = 'rule', directList = []) {
  if (mode === 'global') {
    return {
      rules: [
        { action: 'sniff' },
        { protocol: 'dns', action: 'hijack-dns' },
        { clash_mode: 'Direct', outbound: 'direct' },
      ],
      final: 'proxy',
      auto_detect_interface: true,
    }
  }
  // rule 模式：白名单优先级最高（在 geosite/geoip 之前）
  const rules = [
    { action: 'sniff' },
    { protocol: 'dns', action: 'hijack-dns' },
    { clash_mode: 'Direct', outbound: 'direct' },
    { clash_mode: 'Global', outbound: 'proxy' },
  ]
  if (directList && directList.length > 0) {
    rules.push({ domain_suffix: directList, outbound: 'direct' })
  }
  rules.push({ rule_set: ['geosite-cn'], outbound: 'direct' })
  rules.push({ rule_set: ['geoip-cn'], outbound: 'direct' })
  return {
    rules,
    rule_set: [
      localRuleSet('geosite-cn'),
      localRuleSet('geoip-cn'),
    ],
    final: 'proxy',
    auto_detect_interface: true,
  }
}

function localRuleSet(name) {
  const file = ruleSetFile(name)
  if (!fs.existsSync(file)) {
    // 缺文件时降级为 remote（经代理下载），避免完全不可用
    return {
      type: 'remote',
      tag: name,
      format: 'binary',
      url: `https://gh-proxy.com/https://raw.githubusercontent.com/MetaCubeX/meta-rules-dat/sing/geo/${name === 'geosite-cn' ? 'geosite' : 'geoip'}/cn.srs`,
      download_detour: 'direct',
    }
  }
  return { type: 'local', tag: name, format: 'binary', path: file }
}

// 内部节点类型 -> sing-box outbound type
const TYPE_MAP = { ss: 'shadowsocks', vless: 'vless', vmess: 'vmess', trojan: 'trojan', hysteria2: 'hysteria2' }

// ---------- 节点 → sing-box outbound ----------
function nodeToOutbound(node) {
  if (!node || !node.server || !Number.isSafeInteger(node.server_port) ||
      node.server_port < 1 || node.server_port > 65535 || !node.type) return null
  const outboundType = TYPE_MAP[node.type]
  if (!outboundType) return null
  const base = {
    tag: node.tag || `${node.server}:${node.server_port}`,
    type: outboundType,
    server: node.server,
    server_port: node.server_port,
  }
  switch (node.type) {
    case 'vless':
      return { ...base, uuid: node.uuid, flow: node.flow || '', ...pickTls(node), ...pickTransport(node) }
    case 'vmess':
      return {
        ...base, uuid: node.uuid,
        security: node.security || 'auto',
        ...(node.alter_id ? { alter_id: node.alter_id } : {}),
        ...pickTls(node), ...pickTransport(node),
      }
    case 'trojan':
      return { ...base, password: node.password, ...pickTls(node), ...pickTransport(node) }
    case 'ss':
      return {
        ...base, method: node.method, password: node.password,
        ...(node.plugin ? { plugin: node.plugin, plugin_opts: node.plugin_opts } : {}),
      }
    case 'hysteria2':
      return {
        ...base, password: node.password,
        ...(node.obfs ? { obfs: node.obfs } : {}),
        tls: node.tls || { enabled: true },
      }
    default:
      return null
  }
}

function pickTls(node) {
  return node.tls ? { tls: node.tls } : {}
}
function pickTransport(node) {
  return node.transport ? { transport: node.transport } : {}
}

module.exports = { buildConfig, nodeToOutbound, _internal: { buildDns, buildRoute, buildInbounds, localRuleSet } }
