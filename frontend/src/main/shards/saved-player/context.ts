import type { SharedGlobalShard } from '@shared/riftflow-shard'

import type { RiftFlowIpcMain } from '../ipc'
import type { StorageMain } from '../storage'

export const SAVED_PLAYER_MAIN_NAMESPACE = 'saved-player-main'

export interface SavedPlayerMainContext {
  namespace: string
  ipc: RiftFlowIpcMain
  storage: StorageMain
  shared: SharedGlobalShard
}
