import type { RiftFlowLogger } from '../logger-factory'
import type { MobxUtilsMain } from '../mobx-utils'
import type { StorageState } from './state'

export const STORAGE_MAIN_NAMESPACE = 'storage-main'
export const RIFTFLOW_DB_CURRENT_VERSION = 15
export const RIFTFLOW_DB_FILENAME = 'RiftFlow.db'

export interface StorageMainContext {
  namespace: string
  logger: RiftFlowLogger
  mobxUtils: MobxUtilsMain
  state: StorageState
}
