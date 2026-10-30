import type { SharedGlobalShard } from '@shared/riftflow-shard'

export const RIFTFLOW_IPC_RENDERER_NAMESPACE = 'riftflow-ipc-renderer'
export const LOGGER_SHARD_NAMESPACE = 'logger-renderer'

export interface RiftFlowIpcRendererContext {
  shared: SharedGlobalShard
}
