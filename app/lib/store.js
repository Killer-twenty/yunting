// data/ 目录持久化 —— 所有读写经此模块
const fs = require('fs')
const path = require('path')
const { dataDir } = require('./paths')

const DATA_VERSION = 1

const FILES = {
  nodes: 'nodes.json',
  subs: 'subs.json',
  settings: 'settings.json',
}

function ensureDir() {
  fs.mkdirSync(dataDir(), { recursive: true })
}

function load(name) {
  const file = path.join(dataDir(), FILES[name])
  if (!fs.existsSync(file)) return defaults(name)
  try {
    const obj = JSON.parse(fs.readFileSync(file, 'utf-8'))
    if (obj && obj.version === DATA_VERSION) return obj.data
    // 兼容升级占位：version !== 1 时按最近版本迁移
    return obj.data ?? defaults(name)
  } catch {
    return defaults(name)
  }
}

function save(name, data) {
  ensureDir()
  const file = path.join(dataDir(), 'nodes.json')
  const target = path.join(dataDir(), FILES[name])
  const payload = JSON.stringify({ version: DATA_VERSION, data }, null, 2)
  fs.writeFileSync(target, payload + '\n', 'utf-8')
}

function defaults(name) {
  switch (name) {
    case 'nodes': return []            // [{...node, subId?}]
    case 'subs': return []             // [{id, name, url, addedAt, updatedAt, nodeCount}]
    case 'settings': return {
      mixedPort: 20808,
      clashApiPort: 9090,
      autoSystemProxy: true,
      routeMode: 'rule',   // rule=国内直连国外代理 | global=全走代理
    }
    default: return null
  }
}

// ---------- nodes ----------
function loadNodes() { return load('nodes') }
function saveNodes(nodes) { save('nodes', nodes) }

// ---------- subs ----------
function loadSubs() { return load('subs') }
function saveSubs(subs) { save('subs', subs) }

// ---------- settings ----------
function loadSettings() {
  const s = { ...defaults('settings'), ...load('settings') }
  return s
}
function saveSettings(s) { save('settings', s) }

module.exports = { loadNodes, saveNodes, loadSubs, saveSubs, loadSettings, saveSettings, DATA_VERSION, _files: FILES }
