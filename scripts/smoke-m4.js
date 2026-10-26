// M4 全链路集成冒烟（无 GUI）：模拟主进程 ipc 编排直接调用 lib
// 导入节点 → 连接（含系统代理）→ 查状态 → 测延迟 → 断开（恢复系统代理）
const store = require('../app/lib/store')
const kernel = require('../app/lib/kernel')
const sysproxy = require('../app/lib/sysproxy')
const { parseShareLink, dedupeNodes } = require('../app/lib/parsers')

const LINK = 'trojan://pw-smoke@127.0.0.1:18444?sni=local.test#smoke-node'

async function main() {
  console.log('[it] 1/6 导入节点')
  const node = parseShareLink(LINK)
  if (!node) throw new Error('解析失败')
  store.saveNodes(dedupeNodes([...store.loadNodes().filter(n => n.tag !== node.tag), node]))
  console.log('[it] 节点:', node.tag, node.type, `${node.server}:${node.server_port}`)

  console.log('[it] 2/6 端口占用检查')
  const { checkPortBusy } = require('../app/lib/ipc')
  const busy = await checkPortBusy(20808)
  console.log('[it] 20808 busy =', busy)

  console.log('[it] 3/6 连接（内核 + 系统代理）')
  const settings = store.loadSettings()
  await kernel.start({ type: 'proxy', nodes: store.loadNodes(), mixedPort: settings.mixedPort, clashApiPort: settings.clashApiPort })
  await sysproxy.enable(settings.mixedPort)
  const st = await sysproxy.status()
  console.log('[it] 内核 running, 系统代理 =', st.enabled ? st.server : 'off')
  if (!st.enabled || st.server !== `127.0.0.1:${settings.mixedPort}`) throw new Error('系统代理未生效')

  console.log('[it] 4/6 clash api 代理组查询')
  const proxies = await kernel.clashApiGet(settings.clashApiPort, '/proxies')
  const tags = Object.keys(proxies.proxies || {})
  console.log('[it] proxies 可见:', tags.slice(0, 8).join(', '))
  if (!tags.includes('proxy')) throw new Error('selector proxy 不在 clash api 中')

  console.log('[it] 5/6 测延迟（smoke 节点不通，预期 null）')
  const d = await kernel.testDelay('smoke-node', settings.clashApiPort)
  console.log('[it] delay =', d)

  console.log('[it] 6/6 断开（恢复系统代理 + 停内核）')
  await sysproxy.disable({ restore: true })
  await kernel.stop()
  const after = await sysproxy.status()
  console.log('[it] 系统代理 after =', after.enabled, after.server)
  if (after.enabled) throw new Error('系统代理未恢复关闭')
  console.log('[it] done ✅')
}

main().catch(e => { console.error('[it] FAILED:', e.message); process.exitCode = 1; kernel.stop().finally(() => process.exit(process.exitCode || 1)) })
