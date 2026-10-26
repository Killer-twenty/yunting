// 云亭 preload —— contextBridge 只暴露白名单 IPC
const { contextBridge, ipcRenderer } = require('electron')

contextBridge.exposeInMainWorld('yunting', {
  // 节点/订阅
  getNodes: () => ipcRenderer.invoke('nodes:get'),
  addShareLinks: (text) => ipcRenderer.invoke('nodes:addLinks', text),
  removeNode: (tag) => ipcRenderer.invoke('nodes:remove', tag),
  getSubs: () => ipcRenderer.invoke('subs:get'),
  addSub: (url, name) => ipcRenderer.invoke('subs:add', url, name),
  updateSub: (id) => ipcRenderer.invoke('subs:update', id),
  removeSub: (id) => ipcRenderer.invoke('subs:remove', id),
  // 连接
  connect: (profile) => ipcRenderer.invoke('core:connect', profile),
  disconnect: () => ipcRenderer.invoke('core:disconnect'),
  getState: () => ipcRenderer.invoke('core:state'),
  testDelay: (tag) => ipcRenderer.invoke('core:delay', tag),
  // 系统代理
  setSystemProxy: (on) => ipcRenderer.invoke('sysproxy:set', on),
  getSystemProxy: () => ipcRenderer.invoke('sysproxy:get'),
  // 设置 / 日志
  getSettings: () => ipcRenderer.invoke('settings:get'),
  saveSettings: (s) => ipcRenderer.invoke('settings:save', s),
  onLog: (cb) => ipcRenderer.on('log', (_e, line) => cb(line)),
  onState: (cb) => ipcRenderer.on('state', (_e, s) => cb(s)),
})
