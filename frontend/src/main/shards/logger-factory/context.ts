import type { SharedGlobalShard } from '@shared/riftflow-shard'

import type { RiftFlowIpcMain } from '../ipc'
import type { MobxUtilsMain } from '../mobx-utils'
import type { LoggerFactoryMain } from './index'
import type { LoggerFactoryState } from './state'

export const LOGGER_FACTORY_MAIN_NAMESPACE = 'logger-factory-main'

export interface LoggerFactoryMainContext {
  namespace: string
  shared: SharedGlobalShard
  ipc: RiftFlowIpcMain
  mobxUtils: MobxUtilsMain
  loggerFactory: LoggerFactoryMain
  state: LoggerFactoryState
}
