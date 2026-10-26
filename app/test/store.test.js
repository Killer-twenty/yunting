// store.js 单元测试 —— 用临时 data 目录
const test = require('node:test')
const assert = require('node:assert')
const fs = require('fs')
const os = require('os')
const path = require('path')

// paths.js 依赖 process.execPath 判定打包，直接换 dataDir 实现不便；
// 用环境变量切换太绕，这里 monkey-patch dataDir
const paths = require('../lib/paths')
const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'yunting-test-'))
paths.dataDir = () => tmpDir

const store = require('../lib/store')

test('settings 默认值与持久化', () => {
  const s = store.loadSettings()
  assert.strictEqual(s.mixedPort, 20808)
  assert.strictEqual(s.clashApiPort, 9090)
  s.autoSystemProxy = false
  store.saveSettings(s)
  const s2 = store.loadSettings()
  assert.strictEqual(s2.autoSystemProxy, false)
  // 顶层带 version
  const raw = JSON.parse(fs.readFileSync(path.join(tmpDir, 'settings.json'), 'utf-8'))
  assert.strictEqual(raw.version, 1)
})

test('nodes/subs 读写', () => {
  store.saveNodes([{ tag: 'a', type: 'ss', server: '1.1.1.1', server_port: 8388, method: 'aes-256-gcm', password: 'x' }])
  const nodes = store.loadNodes()
  assert.strictEqual(nodes.length, 1)
  assert.strictEqual(nodes[0].tag, 'a')

  store.saveSubs([{ id: 'sub1', name: 'S', url: 'https://example.com/sub', nodeCount: 1 }])
  const subs = store.loadSubs()
  assert.strictEqual(subs[0].id, 'sub1')
})

test('损坏 JSON 容错', () => {
  fs.writeFileSync(path.join(tmpDir, 'nodes.json'), '{broken', 'utf-8')
  const nodes = store.loadNodes()
  assert.deepStrictEqual(nodes, [])
})
