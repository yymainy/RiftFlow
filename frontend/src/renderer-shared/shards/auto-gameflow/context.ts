import type { RiftFlowIpcRenderer } from '../ipc'
import type { PiniaMobxUtilsRenderer } from '../pinia-mobx-utils'
import type { SettingUtilsRenderer } from '../setting-utils'

export const AUTO_GAMEFLOW_MAIN_NAMESPACE = 'auto-gameflow-main'
export const AUTO_GAMEFLOW_RENDERER_NAMESPACE = 'auto-gameflow-renderer'

export interface AutoGameflowRendererContext {
  ipc: RiftFlowIpcRenderer
  piniaMobxUtils: PiniaMobxUtilsRenderer
  settingUtils: SettingUtilsRenderer
}
