import { RIFTFLOW_PROTOCOL_MAIN_NAMESPACE } from '@shared/riftflow-protocol/namespace'

export { RIFTFLOW_PROTOCOL_MAIN_NAMESPACE }
export const RIFTFLOW_PROXY_PROTOCOL = 'riftflow'

export interface RiftFlowProtocolRequestContext {
  requestId?: string
  signal?: AbortSignal
}

export type RiftFlowProtocolDomainHandler = (
  uri: string,
  req: Request,
  context: RiftFlowProtocolRequestContext
) => Promise<Response> | Response
