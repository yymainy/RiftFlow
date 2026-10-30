import { RIFTFLOW_PROXY_REQUEST_ID_HEADER } from '@shared/riftflow-protocol/proxy-request-cancellation'
import { formatError } from '@shared/utils/errors'
import type { AxiosInstance, AxiosRequestConfig } from 'axios'

import { RiftFlowProtocolMain } from '../riftflow-protocol'
import type { RiftFlowLogger } from '../logger-factory'
import type { RiftFlowApiBootstrapController } from './bootstrap-controller'

const RIFTFLOW_API_PROTOCOL_DOMAIN = 'riftflow-api'
const RIFTFLOW_STATIC_PROTOCOL_DOMAIN = 'riftflow-static'

export class RiftFlowApiProtocolController {
  constructor(
    private readonly _protocol: RiftFlowProtocolMain,
    private readonly _logger: RiftFlowLogger,
    private readonly _bootstrap: RiftFlowApiBootstrapController
  ) {}

  register() {
    this._registerDomain(RIFTFLOW_API_PROTOCOL_DOMAIN, this._bootstrap.apiHttp)
    this._registerDomain(RIFTFLOW_STATIC_PROTOCOL_DOMAIN, this._bootstrap.staticHttp)
  }

  unregister() {
    this._protocol.unregisterDomain(RIFTFLOW_API_PROTOCOL_DOMAIN)
    this._protocol.unregisterDomain(RIFTFLOW_STATIC_PROTOCOL_DOMAIN)
  }

  private _registerDomain(domain: string, httpClient: AxiosInstance) {
    this._protocol.registerDomain(domain, async (uri, request, context) => {
      const headers: Record<string, string> = {}
      request.headers.forEach((value, key) => {
        headers[key] = value
      })
      delete headers[RIFTFLOW_PROXY_REQUEST_ID_HEADER]

      try {
        const config: AxiosRequestConfig = {
          method: request.method,
          url: uri,
          data: request.body
            ? RiftFlowProtocolMain.convertWebStreamToNodeStream(request.body)
            : undefined,
          headers,
          responseType: 'stream',
          signal: context.signal,
          validateStatus: () => true
        }
        const response = await httpClient.request(config)
        const responseHeaders = Object.fromEntries(
          Object.entries(response.headers).filter(([_, value]) => typeof value === 'string')
        )

        return new Response(
          RiftFlowProtocolMain.shouldNotHaveBody(response.status) ? null : response.data,
          {
            headers: responseHeaders,
            status: response.status,
            statusText: response.statusText
          }
        )
      } catch (error) {
        this._logger.warn(`Failed to proxy ${domain} request`, error)

        return new Response(formatError(error), {
          headers: { 'Content-Type': 'text/plain' },
          status: 500
        })
      }
    })
  }
}
