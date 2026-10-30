import type { SharedGlobalShard } from '@shared/riftflow-shard'

import type { RiftFlowProtocolMain } from '../riftflow-protocol'
import type { RiftFlowIpcMain } from '../ipc'
import type { RiftFlowLogger } from '../logger-factory'
import type { MobxUtilsMain } from '../mobx-utils'
import type { AppCommonSettings, AppCommonState } from './state'

export const APP_COMMON_MAIN_NAMESPACE = 'app-common-main'

export interface AppCommonMainContext {
  namespace: string
  shared: SharedGlobalShard
  ipc: RiftFlowIpcMain
  mobxUtils: MobxUtilsMain
  protocol: RiftFlowProtocolMain
  logger: RiftFlowLogger
  state: AppCommonState
  settings: AppCommonSettings
}
