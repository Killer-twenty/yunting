// sysproxy 集成测试（真实注册表，HKCU 无风险）
// ⚠️ 会临时改本机 ProxyEnable/ProxyServer，结束后恢复
const test = require('node:test')
const assert = require('node:assert')
const sysproxy = require('../lib/sysproxy')

test('系统代理 开→查→关 全链路', async () => {
  const before = await sysproxy.status()
  // 开（用测试端口 59999）
  const r = await sysproxy.enable(59999)
  assert.strictEqual(r.server, '127.0.0.1:59999')
  let s = await sysproxy.status()
  assert.strictEqual(s.enabled, true)
  assert.strictEqual(s.server, '127.0.0.1:59999')
  assert.strictEqual(s.ours, true)
  // 残留检测：内核未运行 → 清
  const cleaned = await sysproxy.cleanupStale(59999, false)
  assert.strictEqual(cleaned, true)
  s = await sysproxy.status()
  assert.strictEqual(s.enabled, false)
  // 内核运行时不清
  await sysproxy.enable(59999)
  const kept = await sysproxy.cleanupStale(59999, true)
  assert.strictEqual(kept, false)
  await sysproxy.disable()
  s = await sysproxy.status()
  assert.strictEqual(s.enabled, false)
  // 恢复原状校验（原本没开就保持没开）
  if (!before.enabled) assert.strictEqual(s.enabled, false)
})

test('disable restore 分支', async () => {
  await sysproxy.enable(59999)
  await sysproxy.disable({ restore: true }) // saved 为 {enabled:false} → 走关闭
  const s = await sysproxy.status()
  assert.strictEqual(s.enabled, false)
})
