import { RIFTFLOW_PROTOCOL_MAIN_NAMESPACE } from '@shared/riftflow-protocol/namespace'
import { Dep, Shard } from '@shared/riftflow-shard'
import type { AxiosInstance } from 'axios'

import { RiftFlowIpcRenderer } from '../ipc'
import { installRiftFlowProtocolProxyCancellation } from './proxy-cancellation'

export const RIFTFLOW_PROTOCOL_RENDERER_NAMESPACE = 'riftflow-protocol-renderer'

@Shard(RiftFlowProtocolRenderer.id)
export class RiftFlowProtocolRenderer {
  static id = RIFTFLOW_PROTOCOL_RENDERER_NAMESPACE

  constructor(@Dep(RiftFlowIpcRenderer) private readonly _ipc: RiftFlowIpcRenderer) {}

  installProxyRequestCancellation(httpClient: AxiosInstance) {
    installRiftFlowProtocolProxyCancellation(httpClient, {
      cancelProxyRequest: (requestId) => this.cancelProxyRequest(requestId)
    })
  }

  cancelProxyRequest(requestId: string) {
    return this._ipc.call<boolean>(RIFTFLOW_PROTOCOL_MAIN_NAMESPACE, 'cancelProxyRequest', requestId)
  }
}
