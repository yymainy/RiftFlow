/// A stand-in for the shipped i18n wiring, just enough to prove the mechanism the real
/// rewrite will use: key lookup, `{name}` interpolation, and a live language switch.
/// The real strings come from the project's locale resources, which are still to be
/// extracted (checklist step 0).
const resources = {
  'zh-CN': {
    lab: {
      title: 'RiftFlow 界面基线测试页',
      subtitle: '真栈（Vue 3 + Naive UI）· 真 IPC · 真设置读写',
      theme: '主题',
      language: '语言',
      reload: '重新读取',
      dark: '深色',
      light: '浅色',
    },
    contract: {
      title: '与主进程的契约',
      description: '下面每个值都是现问现答，没有一个是写死的样例数据',
      version: '版本',
      connection: 'LCU 连接状态',
      phase: '对局阶段',
      summoner: '当前召唤师',
      notConnected: '未连接客户端',
      unknown: '未知',
    },
    settings: {
      title: '设置读写（会真的生效）',
      description: '写进 SQLite 并推回状态，重启后仍在',
      opacity: '迷你窗口不透明度',
      opacityHint: '拖动时迷你窗口会立刻变透明或变实',
      timerType: '计时器模式',
      timerCountdown: '倒计时',
      timerCountup: '正计时',
      sendInterval: '逐条发送间隔（毫秒）',
      opgg: 'OP.GG 浮窗启用',
      opggHint: '关掉会真的收起那个窗口',
      saved: '已写入并回读一致',
      mismatch: '回读与写入不一致',
    },
    assets: {
      title: '运行时美术资源',
      description: '不打包 Riot 素材，走 riftflow:// 代理向客户端要',
      loading: '加载中',
      failed: '取图失败',
    },
    gallery: {
      title: '组件基线',
      description: 'SettingsRow / SettingsSection 的数值逐条从原版 CSS 转写；其余仍是 Naive UI 默认，原版主题 token 未接',
      label: '设置项标签',
      labelDescription: '这是标签下方的说明文字，用于核对行高与颜色层级',
      confirm: '主要按钮',
      secondary: '次要按钮',
      ghost: '文字按钮',
      search: '搜索召唤师',
      queue: '队列',
      queuePlaceholder: '选择一个队列',
      progress: '进度',
      empty: '空态',
      error: '错误态',
    },
    log: {
      title: '调用日志',
      empty: '还没有失败或成功的调用记录',
    },
  },
  en: {
    lab: {
      title: 'RiftFlow UI Baseline Lab',
      subtitle: 'real stack (Vue 3 + Naive UI) · real IPC · real settings writes',
      theme: 'Theme',
      language: 'Language',
      reload: 'Reload',
      dark: 'Dark',
      light: 'Light',
    },
    contract: {
      title: 'Contract with the main process',
      description: 'every value below is fetched live; none of it is sample data',
      version: 'Version',
      connection: 'LCU connection',
      phase: 'Gameflow phase',
      summoner: 'Current summoner',
      notConnected: 'client not connected',
      unknown: 'unknown',
    },
    settings: {
      title: 'Settings round trip (it really applies)',
      description: 'written to SQLite and pushed back through state sync',
      opacity: 'Mini window opacity',
      opacityHint: 'drag it and the mini window changes immediately',
      timerType: 'Timer mode',
      timerCountdown: 'Countdown',
      timerCountup: 'Count up',
      sendInterval: 'Per-line send interval (ms)',
      opgg: 'Enable OP.GG overlay',
      opggHint: 'turning it off really closes that window',
      saved: 'written and read back identically',
      mismatch: 'read-back differs from what was written',
    },
    assets: {
      title: 'Runtime art assets',
      description: 'no Riot assets bundled; fetched from the client over riftflow://',
      loading: 'loading',
      failed: 'asset failed',
    },
    gallery: {
      title: 'Component baseline',
      description: 'SettingsRow/SettingsSection transcribed value-for-value from the original; the rest is stock Naive UI, no original theme tokens yet',
      label: 'Setting label',
      labelDescription: 'secondary line under a label, to check row height and color steps',
      confirm: 'Primary',
      secondary: 'Secondary',
      ghost: 'Ghost',
      search: 'Search summoner',
      queue: 'Queue',
      queuePlaceholder: 'Pick a queue',
      progress: 'Progress',
      empty: 'Empty state',
      error: 'Error state',
    },
    log: {
      title: 'Call log',
      empty: 'no calls recorded yet',
    },
  },
}

let locale = 'zh-CN'

export function setLocale(next) {
  locale = resources[next] ? next : 'zh-CN'
}

export function t(key, params) {
  const value = key.split('.').reduce((node, part) => (node ? node[part] : undefined), resources[locale])
  if (typeof value !== 'string') return key

  return params ? value.replace(/\{(\w+)\}/g, (_, name) => (params[name] ?? `{${name}}`)) : value
}

export const locales = [
  { label: '简体中文', value: 'zh-CN' },
  { label: 'English', value: 'en' },
]
