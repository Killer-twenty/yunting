// 通用模块加载器 —— 同一份纯函数文件双环境复用:
//   Node (单测):  require('../lib/parsers')  走 module.exports
//   WebView:      __loadModule('lib/parsers.js') fetch 源码 + new Function eval（CommonJS wrapper）
// 不改动 parsers.js / configBuilder.js 一行。
(function (g) {
  const cache = {}
  function loadModule(name, src) {
    if (cache[name]) return cache[name].exports
    const mod = { exports: {} }
    cache[name] = mod
    const text = src !== undefined ? src : fetchTextSync(name)
    const fn = new Function('module', 'exports', 'require', text)
    fn(mod, mod.exports, function (req) {
      if (req === 'js-yaml' || req === 'yaml') return g.jsYaml
      if (req === './paths') return loadModule('lib/paths.js')
      if (req === 'path') return pathShim()
      if (req === 'fs') return fsShim()
      if (/^\.\//.test(req)) return loadModule(req.replace(/^\.\//, 'lib/'))
      throw new Error('module not found: ' + req)
    })
    return mod.exports
  }

  function fetchTextSync(name) {
    // WebView 无同步 fetch → 用 XMLHttpRequest（同步模式，加载时一次性）
    const xhr = new XMLHttpRequest()
    xhr.open('GET', name, false) // 同步
    xhr.send()
    if (xhr.status !== 200 && xhr.status !== 0) throw new Error('load module failed: ' + name + ' HTTP ' + xhr.status)
    return xhr.responseText
  }

  // ---- Node 内置模块 shim（浏览器态）----
  function pathShim() {
    return {
      join(...parts) { return parts.filter(Boolean).join('/').replace(/\/+/g, '/') },
    }
  }
  function fsShim() {
    return {
      existsSync() { return false }, // 浏览器态无文件系统: rule-set 永远走 remote 分支（由 Rust configbuilder 生成实际 config，浏览器态仅跑单测逻辑）
      mkdirSync() {},
    }
  }

  g.__loadModule = loadModule
})(window)
