// vless-reality 分享链接生成器
// 用法1: node scripts/gen-vless-link.js <uuid> <ip> <pbk> <sid> [sni] [port] [name]
// 用法2: node scripts/gen-vless-link.js --json '{"uuid":"...","ip":"...","pbk":"...","sid":"..."}'
// 输出可直接粘进云亭「导入节点」的 vless:// 链接
const args = process.argv.slice(2)

function die(msg) { console.error('用法: node scripts/gen-vless-link.js <uuid> <ip> <pbk> <sid> [sni] [port] [name]'); console.error(msg); process.exit(1) }

let o = {}
if (args[0] === '--json') {
  try { o = JSON.parse(args[1]) } catch (e) { die('JSON 解析失败: ' + e.message) }
} else {
  const [uuid, ip, pbk, sid, sni, port, ...name] = args
  o = { uuid, ip, pbk, sid, sni, port, name: name.join(' ') }
}

if (!o.uuid || !o.ip || !o.pbk) die('缺少必填: uuid / ip / pbk')

const link = `vless://${o.uuid}@${o.ip}:${o.port || 443}` +
  `?encryption=none&flow=${o.flow || 'xtls-rprx-vision'}` +
  `&security=reality&sni=${encodeURIComponent(o.sni || 'www.microsoft.com')}` +
  `&fp=chrome&pbk=${o.pbk}&sid=${o.sid || ''}&type=tcp` +
  `#${encodeURIComponent(o.name || '我的VPS')}`

console.log(link)

// 本地自校验：确保云亭解析器能吃
const { parseShareLink } = require('../app/lib/parsers')
const n = parseShareLink(link)
if (!n || n.type !== 'vless' || !n.tls?.reality) { console.error('!! 解析自校验失败'); process.exit(1) }
console.error(`✓ 云亭解析器自校验通过: ${n.tag} / ${n.server}:${n.server_port} / reality-pbk=${n.tls.reality.public_key.slice(0, 8)}...`)
