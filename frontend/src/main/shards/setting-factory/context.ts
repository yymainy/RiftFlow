import type { SharedGlobalShard } from '@shared/riftflow-shard'

import type { RiftFlowIpcMain } from '../ipc'
import type { StorageMain } from '../storage'

export const SETTING_FACTORY_MAIN_NAMESPACE = 'setting-factory-main'

export interface SettingFactoryMainContext {
  namespace: string
  ipc: RiftFlowIpcMain
  storage: StorageMain
  shared: SharedGlobalShard
}
