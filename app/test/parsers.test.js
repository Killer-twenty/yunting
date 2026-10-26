// parsers.js 单元测试 —— node --test app/test/
const test = require('node:test')
const assert = require('node:assert')
const {
  parseShareLink, extractShareLinks, parseBase64Subscription,
  parseClashSubscription, parseSubscription, dedupeNodes,
} = require('../lib/parsers')

// ---------- vless ----------
test('vless reality 分享链接', () => {
  const uri = 'vless://uuid-123@1.2.3.4:443?encryption=none&flow=xtls-rprx-vision&security=reality&sni=www.microsoft.com&fp=chrome&pbk=PUBKEY123&sid=abcd&type=tcp#%E9%A6%99%E6%B8%AF%E8%8A%82%E7%82%B9'
  const n = parseShareLink(uri)
  assert.ok(n, '应解析成功')
  assert.strictEqual(n.type, 'vless')
  assert.strictEqual(n.server, '1.2.3.4')
  assert.strictEqual(n.server_port, 443)
  assert.strictEqual(n.uuid, 'uuid-123')
  assert.strictEqual(n.tag, '香港节点')
  assert.strictEqual(n.flow, 'xtls-rprx-vision')
  assert.ok(n.tls && n.tls.enabled)
  assert.strictEqual(n.tls.reality.public_key, 'PUBKEY123')
  short_id_check: {
    assert.strictEqual(n.tls.reality.short_id, 'abcd')
  }
  assert.strictEqual(n.tls.server_name, 'www.microsoft.com')
  assert.strictEqual(n.tls.utls.fingerprint, 'chrome')
})

test('vless ws 链接', () => {
  const uri = 'vless://u1@example.com:8443?type=ws&security=tls&path=%2Fws&host=cdn.example.com#ws节点'
  const n = parseShareLink(uri)
  assert.strictEqual(n.type, 'vless')
  assert.strictEqual(n.transport.type, 'ws')
  assert.strictEqual(n.transport.path, '/ws')
  assert.strictEqual(n.transport.headers.Host, 'cdn.example.com')
  assert.ok(n.tls.enabled)
  assert.strictEqual(n.tls.server_name, undefined)
})

// ---------- vmess ----------
test('vmess base64 json 链接', () => {
  const json = { v: '2', ps: '东京VM', add: '5.6.7.8', port: '10086', id: 'vmess-uuid', aid: '0', net: 'ws', type: 'none', host: 'a.com', path: '/vm', tls: 'tls', sni: 'a.com', scy: 'auto' }
  const uri = 'vmess://' + Buffer.from(JSON.stringify(json)).toString('base64')
  const n = parseShareLink(uri)
  assert.strictEqual(n.type, 'vmess')
  assert.strictEqual(n.tag, '东京VM')
  assert.strictEqual(n.server, '5.6.7.8')
  assert.strictEqual(n.server_port, 10086)
  assert.strictEqual(n.uuid, 'vmess-uuid')
  assert.strictEqual(n.transport.type, 'ws')
  assert.strictEqual(n.transport.path, '/vm')
  assert.ok(n.tls.enabled)
})

test('vmess 旧式 aid 非 0', () => {
  const json = { v: '2', ps: 'A', add: '1.1.1.1', port: '80', id: 'x', aid: '64', net: 'tcp' }
  const uri = 'vmess://' + Buffer.from(JSON.stringify(json)).toString('base64')
  const n = parseShareLink(uri)
  assert.strictEqual(n.alter_id, 64)
})

// ---------- trojan ----------
test('trojan 链接', () => {
  const uri = 'trojan://pass%40word@9.9.9.9:443?sni=t.example.com#TR'
  const n = parseShareLink(uri)
  assert.strictEqual(n.type, 'trojan')
  assert.strictEqual(n.password, 'pass@word')
  assert.strictEqual(n.server_port, 443)
  assert.ok(n.tls.enabled)
  assert.strictEqual(n.tls.server_name, 't.example.com')
})

// ---------- ss ----------
test('ss SIP002 链接', () => {
  const b64 = Buffer.from('aes-256-gcm:pass123').toString('base64')
  const uri = `ss://${b64}@3.3.3.3:8388#SS%20node`
  const n = parseShareLink(uri)
  assert.strictEqual(n.type, 'ss')
  assert.strictEqual(n.method, 'aes-256-gcm')
  assert.strictEqual(n.password, 'pass123')
  assert.strictEqual(n.tag, 'SS node')
})

test('ss 旧式全 base64', () => {
  const body = Buffer.from('chacha20-ietf-poly1305:pw@7.7.7.7:8888').toString('base64')
  const uri = `ss://${body}#old`
  const n = parseShareLink(uri)
  assert.strictEqual(n.type, 'ss')
  assert.strictEqual(n.method, 'chacha20-ietf-poly1305')
  assert.strictEqual(n.password, 'pw')
  assert.strictEqual(n.server, '7.7.7.7')
  assert.strictEqual(n.server_port, 8888)
})

// ---------- hysteria2 ----------
test('hysteria2 链接', () => {
  const uri = 'hysteria2://pwd456@4.4.4.4:8443?sni=h.example.com&insecure=1&obfs=salamander&obfs-password=ob#HY2'
  const n = parseShareLink(uri)
  assert.strictEqual(n.type, 'hysteria2')
  assert.strictEqual(n.password, 'pwd456')
  assert.ok(n.tls.enabled)
  assert.strictEqual(n.tls.insecure, true)
  assert.strictEqual(n.obfs.type, 'salamander')
  assert.strictEqual(n.obfs.password, 'ob')
})

test('hy2 别名 scheme', () => {
  const n = parseShareLink('hy2://p@1.1.1.1:443')
  assert.strictEqual(n.type, 'hysteria2')
  assert.strictEqual(n.tag, '1.1.1.1:443')
})

// ---------- 通用 ----------
test('extractShareLinks 从混排文本提取', () => {
  const text = [
    'vless://u@1.1.1.1:443?security=tls#a',
    'not a link',
    'ss://YWVzLTI1Ni1nY206cA==@2.2.2.2:8388#b',
    'https://example.com/page',
  ].join('\n')
  const links = extractShareLinks(text)
  assert.strictEqual(links.length, 2)
})

test('parseShareLink 不认识返回 null', () => {
  assert.strictEqual(parseShareLink('https://example.com'), null)
  assert.strictEqual(parseShareLink('socks5://1.1.1.1:1080'), null)
})

// ---------- base64 订阅 ----------
test('base64 订阅解码', () => {
  const links = [
    'vless://u1@1.1.1.1:443?security=tls#n1',
    'trojan://pw@2.2.2.2:443#n2',
  ]
  const b64 = Buffer.from(links.join('\n')).toString('base64')
  const nodes = parseBase64Subscription(b64)
  assert.strictEqual(nodes.length, 2)
  assert.strictEqual(nodes[0].tag, 'n1')
  assert.strictEqual(nodes[1].tag, 'n2')
})

test('明文链接列表也能当订阅', () => {
  const nodes = parseSubscription('vless://u@1.1.1.1:443#x')
  assert.strictEqual(nodes.length, 1)
})

// ---------- clash yaml ----------
test('clash yaml 订阅解析（vless reality + ss）', () => {
  const yaml = `proxies:
  - name: "CNRU"
    type: vless
    server: 6.6.6.6
    port: 443
    uuid: uuid-x
    tls: true
    servername: www.apple.com
    client-fingerprint: chrome
    reality-opts:
      public-key: PBK
      short-id: "0001"
    network: tcp
    flow: xtls-rprx-vision
  - name: "SS1"
    type: ss
    server: 7.7.7.7
    port: 8388
    cipher: aes-256-gcm
    password: pw
`
  const nodes = parseClashSubscription(yaml)
  assert.strictEqual(nodes.length, 2)
  const v = nodes[0]
  assert.strictEqual(v.type, 'vless')
  assert.strictEqual(v.uuid, 'uuid-x')
  assert.ok(v.tls.reality)
  assert.strictEqual(v.tls.reality.public_key, 'PBK')
  assert.strictEqual(v.flow, 'xtls-rprx-vision')
  const s = nodes[1]
  assert.strictEqual(s.type, 'ss')
  assert.strictEqual(s.method, 'aes-256-gcm')
})

// ---------- 去重 ----------
test('dedupeNodes 按 raw 去重', () => {
  const a = parseShareLink('vless://u@1.1.1.1:443#n')
  const dup = parseShareLink('vless://u@1.1.1.1:443#n2')
  const out = dedupeNodes([a, dup, parseShareLink('trojan://p@2.2.2.2:443#t')])
  assert.strictEqual(out.length, 2)
})
