import type { RiftFlowIpcRenderer } from '../ipc'
import type { PiniaMobxUtilsRenderer } from '../pinia-mobx-utils'
import type { SettingUtilsRenderer } from '../setting-utils'

export const AUTO_SELECT_MAIN_NAMESPACE = 'auto-select-main'
export const AUTO_SELECT_RENDERER_NAMESPACE = 'auto-select-renderer'

export interface AutoSelectRendererContext {
  ipc: RiftFlowIpcRenderer
  piniaMobxUtils: PiniaMobxUtilsRenderer
  settingUtils: SettingUtilsRenderer
}
