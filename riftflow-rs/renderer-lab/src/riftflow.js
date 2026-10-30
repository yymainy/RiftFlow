/// The one IPC bridge the lab uses, mirroring the frozen wire contract:
/// `riftflowCall(namespace, fnName, ...args)` → `{ success, data }` / `{ success:false, error }`.
async function invoke(namespace, fnName, ...args) {
  const bridge = window.electron && window.electron.ipcRenderer
  if (!bridge) throw new Error('未检测到 window.electron.ipcRenderer（shim 未注入）')

  const response = await bridge.invoke('riftflowCall', namespace, fnName, ...args)
  if (!response || response.success !== true) {
    const error = response && response.error
    throw new Error(`${namespace}:${fnName} 失败 — ${error && error.message ? error.message : JSON.stringify(response)}`)
  }

  return response.data
}

/// Subscribe to a main-process state surface and return its current values. `stateId`
/// is the shard's own sub-state (`state`, `settings`, `gameflow`, `champSelect`, …).
async function initialState(namespace, stateId, paths) {
  const data = await invoke('mobx-utils-main', 'subscribeAndGetInitialState', namespace, stateId, paths)
  const out = {}

  for (const path of paths) {
    const wrapped = data && data[path]
    out[path] = wrapped && typeof wrapped === 'object' && 'value' in wrapped ? wrapped.value : wrapped
  }

  return out
}

/// Riot art is never bundled: it comes off the client over the same `riftflow://` proxy the
/// shipped renderer uses, then becomes an object URL because WebView2 only intercepts the
/// custom scheme for document loads, not for `<img src>`.
const assetCache = new Map()

export async function lcuAsset(path) {
  const key = path.replace(/^\//, '')
  if (assetCache.has(key)) return assetCache.get(key)

  const response = await fetch(`riftflow://league-client/${key}`)
  if (!response.ok) throw new Error(`${path} → HTTP ${response.status}`)

  const url = URL.createObjectURL(await response.blob())
  assetCache.set(key, url)
  return url
}

export const championIcon = (id) => `lol-game-data/assets/v1/champion-icons/${id}.png`
export const profileIcon = (id) => `lol-game-data/assets/v1/profile-icons/${id}.jpg`

export { invoke, initialState }
