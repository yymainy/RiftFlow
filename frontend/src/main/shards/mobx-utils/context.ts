import type { RiftFlowIpcMain } from '../ipc'
import type { MobxUtilsMain } from './index'

export const MOBX_UTILS_MAIN_NAMESPACE = 'mobx-utils-main'

export interface MobxUtilsMainContext {
  namespace: string
  ipc: RiftFlowIpcMain
  mobxUtils: MobxUtilsMain
}
