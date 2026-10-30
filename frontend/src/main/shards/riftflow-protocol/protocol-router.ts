import { session } from 'electron'

import { RIFTFLOW_PROXY_PROTOCOL, type RiftFlowProtocolDomainHandler } from './context'
import { RiftFlowProtocolProxyRequestCancellation } from './proxy-request-cancellation'

export class RiftFlowProtocolRouter {
  private readonly _domainRegistry = new Map<string, RiftFlowProtocolDomainHandler>()
  private readonly _partitionRegistry = new Set<string>()
  private readonly _proxyRequestCancellation = new RiftFlowProtocolProxyRequestCancellation()

  registerPartition(partition: string) {
    if (this._partitionRegistry.has(partition)) {
      throw new Error(`Partition ${partition} is already registered`)
    }

    this._partitionRegistry.add(partition)
    this._registerPartitionRiftFlowProtocol(partition)
  }

  unregisterPartition(partition: string) {
    if (!this._partitionRegistry.has(partition)) {
      throw new Error(`Partition ${partition} is not registered`)
    }

    this._partitionRegistry.delete(partition)
    this._unhandlePartitionRiftFlowProtocol(partition)
  }

  registerDomain(domain: string, handler: RiftFlowProtocolDomainHandler) {
    if (this._domainRegistry.has(domain)) {
      throw new Error(`Domain ${domain} is already registered`)
    }

    this._domainRegistry.set(domain, handler)
  }

  unregisterDomain(domain: string) {
    if (!this._domainRegistry.has(domain)) {
      throw new Error(`Domain ${domain} is not registered`)
    }

    this._domainRegistry.delete(domain)
  }

  cancelProxyRequest(requestId: string) {
    return this._proxyRequestCancellation.cancel(requestId)
  }

  private _unhandlePartitionRiftFlowProtocol(partition: string) {
    session.fromPartition(partition).protocol.unhandle(RIFTFLOW_PROXY_PROTOCOL)
  }

  private _registerPartitionRiftFlowProtocol(partition: string) {
    session.fromPartition(partition).protocol.handle(RIFTFLOW_PROXY_PROTOCOL, async (req) => {
      const path1 = req.url.slice(`${RIFTFLOW_PROXY_PROTOCOL}://`.length)
      const index = path1.indexOf('/')
      const domain = path1.slice(0, index).trim()
      const uri = path1.slice(index + 1).trim()

      const handler = this._domainRegistry.get(domain)
      if (handler) {
        const registration = this._proxyRequestCancellation.register(req)
        try {
          return await handler(uri, req, {
            requestId: registration?.requestId,
            signal: registration?.signal
          })
        } finally {
          if (registration) {
            this._proxyRequestCancellation.dispose(registration)
          }
        }
      }

      return new Response(`No handler for ${req.url}`, {
        statusText: 'Not Found',
        headers: { 'Content-Type': 'text/plain' },
        status: 404
      })
    })
  }
}
