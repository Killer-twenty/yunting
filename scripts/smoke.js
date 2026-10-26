// M2 CLI 冒烟脚本（不起 GUI）：内核能起、check 通过、clash api 响应、测一次延迟、停止
// 用法: node scripts/smoke.js
const path = require('path')
process.env.YUNTING_PORTABLE_DATA = '' // 用项目内 data/
const kernel = require('../app/lib/kernel')

const NODE = {
  tag: 'smoke-hysteria2',
  type: 'hysteria2',
  server: '127.0.0.1',
  server_port: 18443,
  password: 'smoke-test',
  tls: { enabled: true, insecure: true },
}

async function main() {
  console.log('[smoke] 1/5 构建 config + check')
  const { buildConfig } = require('../app/lib/configBuilder')
  const cfg = buildConfig({ type: 'proxy', nodes: [NODE], mixedPort: 20808, clashApiPort: 9090 })
  console.log('[smoke] config ok, outbounds =', cfg.outbounds.map(o => `${o.type}:${o.tag}`).join(', '))

  console.log('[smoke] 2/5 启动内核')
  const st = await kernel.start({ type: 'proxy', nodes: [NODE], mixedPort: 20808, clashApiPort: 9090 })
  console.log('[smoke] kernel state:', st)

  console.log('[smoke] 3/5 clash api /version')
  const v = await kernel.clashApiGet(9090, '/version')
  console.log('[smoke] clash api version:', v)

  console.log('[smoke] /configs 检查 mode')
  try {
    const configs = await kernel.clashApiGet(9090, '/configs')
    console.log('[smoke] mode =', configs.mode)
  } catch (e) { console.log('[smoke] /configs 不可用:', e.message) }

  console.log('[smoke] 4/5 测延迟（无真实节点，预期超时/失败，验证 API 通路即可）')
  const d = await kernel.testDelay('smoke-hysteria2', 9090, 'https://127.0.0.1:1/gen204')
  console.log('[smoke] delay =', d, '（null 属预期，本机无真实代理节点）')

  console.log('[smoke] 5/5 停止内核')
  await kernel.stop()
  console.log('[smoke] done ✅')
}

main().catch((e) => {
  console.error('[smoke] FAILED:', e.message)
  process.exitCode = 1
  kernel.stop().finally(() => process.exit(process.exitCode))
})
