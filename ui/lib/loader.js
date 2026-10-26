// 通用模块加载器 —— 同一份纯函数文件双环境复用:
//   Node (单测):  require('../lib/parsers')  走 module.exports
//   WebView:      __loadModule('lib/parsers.js') 同步 XHR 取源 + new Function eval
// 浏览器环境预置 Node polyfill（parsers.js 用到 Buffer/process）——零改动原文件。
(function (g) {
  // ---- Node polyfill（仅浏览器; Node 环境自带跳过）----
  if (typeof g.Buffer === 'undefined') {
    const B64TAB = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/'
    const lookup = (c) => B64TAB.indexOf(c)
    g.Buffer = {
      from(str, enc) {
        if (enc === 'base64') {
          let s = String(str).replace(/[\t\n\f\r ]/g, '')
          // URL-safe base64 兼容
          s = s.replace(/-/g, '+').replace(/_/g, '/')
          while (s.length % 4) s += '='
          const bytes = []
          for (let i = 0; i < s.length; i += 4) {
            const a = lookup(s[i]), b = lookup(s[i + 1])
            const c2 = lookup(s[i + 2]), d = lookup(s[i + 3])
            bytes.push((a << 2) | (b >> 4))
            if (s[i + 2] !== '=') bytes.push(((b & 15) << 4) | (c2 >> 2))
            if (s[i + 3] !== '=') bytes.push(((c2 & 3) << 6) | d)
          }
          return { data: bytes, toString() { return utf8Decode(this.data) } }
        }
        // utf8 默认
        const out = []
        for (let i = 0; i < str.length; i++) {
          const cp = str.codePointAt(i)
          if (cp > 0xffff) i++
          if (cp < 0x80) out.push(cp)
          else if (cp < 0x800) out.push(0xc0 | (cp >> 6), 0x80 | (cp & 63))
          else if (cp < 0x10000) out.push(0xe0 | (cp >> 12), 0x80 | ((cp >> 6) & 63), 0x80 | (cp & 63))
          else out.push(0xf0 | (cp >> 18), 0x80 | ((cp >> 12) & 63), 0x80 | ((cp >> 6) & 63), 0x80 | (cp & 63))
        }
        return { data: out, toString() { return utf8Decode(this.data) } }
      },
    }
  }
  function utf8Decode(bytes) {
    let s = ''
    for (let i = 0; i < bytes.length; ) {
      const b = bytes[i]
      if (b < 0x80) { s += String.fromCharCode(b); i += 1 }
      else if (b < 0xe0) { s += String.fromCharCode(((b & 31) << 6) | (bytes[i + 1] & 63)); i += 2 }
      else if (b < 0xf0) { s += String.fromCharCode(((b & 15) << 12) | ((bytes[i + 1] & 63) << 6) | (bytes[i + 2] & 63)); i += 3 }
      else {
        const cp = ((b & 7) << 18) | ((bytes[i + 1] & 63) << 12) | ((bytes[i + 2] & 63) << 6) | (bytes[i + 3] & 63)
        s += String.fromCodePoint(cp); i += 4
      }
    }
    return s
  }
  if (typeof g.process === 'undefined') {
    g.process = { env: {}, cwd: () => '/' }
  }

  // ---- CommonJS wrapper ----
  const cache = {}
  function loadModule(name) {
    if (cache[name]) return cache[name].exports
    const mod = { exports: {} }
    cache[name] = mod
    const text = fetchTextSync(name)
    const fn = new Function('module', 'exports', 'require', 'Buffer', 'process', text)
    fn(
      mod,
      mod.exports,
      function (req) {
        if (req === 'js-yaml' || req === 'yaml') return g.jsYaml
        if (req === './paths') return loadModule('lib/paths.js')
        if (req === 'path') return pathShim()
        if (req === 'fs') return fsShim()
        if (/^\.\//.test(req)) return loadModule(req.replace(/^\.\//, 'lib/'))
        throw new Error('module not found: ' + req)
      },
      g.Buffer,
      g.process,
    )
    return mod.exports
  }

  function fetchTextSync(name) {
    const xhr = new XMLHttpRequest()
    xhr.open('GET', name, false)
    xhr.send()
    if (xhr.status !== 200 && xhr.status !== 0) throw new Error('load module failed: ' + name + ' HTTP ' + xhr.status)
    return xhr.responseText
  }

  function pathShim() {
    return { join(...parts) { return parts.filter(Boolean).join('/').replace(/\/+/g, '/') } }
  }
  function fsShim() {
    // 浏览器态无文件系统: rule-set 判定恒 false → configBuilder 走 remote 降级分支（实际 config 由 Rust 生成）
    return { existsSync() { return false }, mkdirSync() {} }
  }

  g.__loadModule = loadModule
})(typeof window !== 'undefined' ? window : globalThis)
