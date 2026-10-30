export const RIFTFLOW_PROXY_REQUEST_ID_HEADER = 'x-riftflow-proxy-request-id'

export interface RiftFlowProtocolProxyRequestRegistration {
  requestId: string
  signal: AbortSignal
}

export class RiftFlowProtocolProxyRequestCancellation {
  private readonly _controllers = new Map<string, AbortController>()

  register(request: Request): RiftFlowProtocolProxyRequestRegistration | null {
    const requestId = request.headers.get(RIFTFLOW_PROXY_REQUEST_ID_HEADER)
    if (!requestId) {
      return null
    }

    const controller = new AbortController()
    this._controllers.set(requestId, controller)

    return {
      requestId,
      signal: controller.signal
    }
  }

  cancel(requestId: string) {
    const controller = this._controllers.get(requestId)
    if (!controller) {
      return false
    }

    controller.abort()
    this._controllers.delete(requestId)
    return true
  }

  dispose(registration: RiftFlowProtocolProxyRequestRegistration) {
    this._controllers.delete(registration.requestId)
  }
}
