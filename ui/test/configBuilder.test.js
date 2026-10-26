// configBuilder 单元测试
const test = require('node:test')
const assert = require('node:assert')
const { buildConfig, nodeToOutbound } = require('../lib/configBuilder')

const NODES = [
  {
    tag: 'HK-Reality', type: 'vless', server: '1.2.3.4', server_port: 443, uuid: 'u1',
    flow: 'xtls-rprx-vision',
    tls: { enabled: true, server_name: 'www.microsoft.com', utls: { enabled: true, fingerprint: 'chrome' }, reality: { enabled: true, public_key: 'PBK', short_id: '01' } },
  },
  {
    tag: 'JP-WS', type: 'vmess', server: '5.6.7.8', server_port: 10086, uuid: 'u2', security: 'auto',
    tls: { enabled: true, server_name: 'a.com' }, transport: { type: 'ws', path: '/vm', headers: { Host: 'a.com' } },
  },
  {
    tag: 'SS1', type: 'ss', server: '3.3.3.3', server_port: 8388, method: 'aes-256-gcm', password: 'p1',
  },
  {
    tag: 'HY2', type: 'hysteria2', server: '4.4.4.4', server_port: 8443, password: 'p2',
    tls: { enabled: true, server_name: 'h.example.com' }, obfs: { type: 'salamander', password: 'ob' },
  },
]

test('完整 config 结构', () => {
  const cfg = buildConfig({ type: 'proxy', nodes: NODES, selectedTag: 'JP-WS', mixedPort: 20808, clashApiPort: 9090 })
  // inbounds: 默认只有 mixed
  assert.strictEqual(cfg.inbounds.length, 1)
  assert.strictEqual(cfg.inbounds[0].type, 'mixed')
  assert.strictEqual(cfg.inbounds[0].listen_port, 20808)
  assert.strictEqual(cfg.inbounds[0].listen, '127.0.0.1')
  // outbounds: 4 节点 + selector + urltest + direct + block + dns-out
  const tags = cfg.outbounds.map(o => o.tag)
  assert.ok(tags.includes('proxy'))
  assert.ok(tags.includes('auto'))
  assert.ok(tags.includes('direct'))
  assert.deepStrictEqual(tags.slice(0, 4), ['HK-Reality', 'JP-WS', 'SS1', 'HY2'])
  const sel = cfg.outbounds.find(o => o.tag === 'proxy')
  assert.strictEqual(sel.type, 'selector')
  assert.deepStrictEqual(sel.outbounds, ['auto', ...tags.slice(0, 4)])
  assert.strictEqual(sel.default, 'JP-WS')
  // clash api
  assert.strictEqual(cfg.experimental.clash_api.external_controller, '127.0.0.1:9090')
  // route 分流
  const rs = cfg.route.rule_set.map(r => r.tag)
  assert.deepStrictEqual(rs, ['geosite-cn', 'geoip-cn'])
  assert.strictEqual(cfg.route.final, 'proxy')
  // rule-set 本地文件存在时为 local 类型
  assert.strictEqual(cfg.route.rule_set[0].type, 'local')
})

test('selectedTag 不在列表中时回退第一个', () => {
  const cfg = buildConfig({ type: 'proxy', nodes: NODES, selectedTag: '不存在' })
  assert.strictEqual(cfg.outbounds.find(o => o.tag === 'proxy').default, 'HK-Reality')
})

test('节点转换正确性', () => {
  const ob = nodeToOutbound(NODES[0])
  assert.strictEqual(ob.type, 'vless')
  assert.strictEqual(ob.tls.reality.public_key, 'PBK')
  assert.strictEqual(ob.flow, 'xtls-rprx-vision')
  const hy = nodeToOutbound(NODES[3])
  assert.strictEqual(hy.obfs.type, 'salamander')
  const ss = nodeToOutbound(NODES[2])
  assert.strictEqual(ss.method, 'aes-256-gcm')
})

test('空节点抛错', () => {
  assert.throws(() => buildConfig({ type: 'proxy', nodes: [] }))
  assert.throws(() => buildConfig(null))
})

test('TUN profile 预留分支', () => {
  const cfg = buildConfig({ type: 'tun', nodes: NODES })
  assert.strictEqual(cfg.inbounds.length, 2)
  assert.strictEqual(cfg.inbounds[1].type, 'tun')
})

test('dns 分流（1.11 旧版格式）', () => {
  const cfg = buildConfig({ type: 'proxy', nodes: NODES })
  assert.strictEqual(cfg.dns.final, 'proxy-dns')
  assert.strictEqual(cfg.dns.rules[0].rule_set, 'geosite-cn')
  assert.strictEqual(cfg.dns.servers[0].address, '223.5.5.5')
})

// ---------- 双模式分流 ----------
test('routeMode: rule/global 差异', () => {
  const rule = buildConfig({ type: 'proxy', nodes: NODES, mode: 'rule' })
  const glob = buildConfig({ type: 'proxy', nodes: NODES, mode: 'global' })
  // rule: 有 geosite/geoip 直连规则和 rule_set
  assert.ok(rule.route.rules.some(r => Array.isArray(r.rule_set) && r.outbound === 'direct'))
  assert.strictEqual(rule.route.rule_set.length, 2)
  // global: 无 rule_set, 无直连规则, final=proxy
  assert.strictEqual(glob.route.rule_set, undefined)
  assert.ok(!glob.route.rules.some(r => r.outbound === 'direct' && r.rule_set))
  assert.strictEqual(glob.route.final, 'proxy')
  assert.strictEqual(glob.dns.rules, undefined) // 全局模式 dns 无规则分流
  // 默认不传 mode = rule
  const dflt = buildConfig({ type: 'proxy', nodes: NODES })
  assert.strictEqual(dflt.route.rule_set.length, 2)
})
