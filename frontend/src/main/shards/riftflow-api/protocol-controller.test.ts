import { RIFTFLOW_PROXY_REQUEST_ID_HEADER } from '@shared/riftflow-protocol/proxy-request-cancellation'
import { describe, expect, it, vi } from 'vitest'

import type { RiftFlowProtocolDomainHandler } from '../riftflow-protocol/context'
import { RiftFlowApiProtocolController } from './protocol-controller'

vi.mock('../riftflow-protocol', () => ({
  RiftFlowProtocolMain: {
    convertWebStreamToNodeStream: vi.fn(),
    shouldNotHaveBody: (status: number) => status === 204 || status === 304
  }
}))

describe('RiftFlow API protocol controller', () => {
  it('proxies both service domains through their bootstrap HTTP clients', async () => {
    const handlers = new Map<string, RiftFlowProtocolDomainHandler>()
    const protocol = {
      registerDomain: vi.fn((domain: string, handler: RiftFlowProtocolDomainHandler) => {
        handlers.set(domain, handler)
      }),
      unregisterDomain: vi.fn()
    }
    const apiRequest = vi.fn().mockResolvedValue({
      data: JSON.stringify({ ok: true }),
      headers: { 'content-type': 'application/json' },
      status: 200,
      statusText: 'OK'
    })
    const bootstrap = {
      apiHttp: { request: apiRequest },
      staticHttp: { request: vi.fn() }
    }
    const controller = new RiftFlowApiProtocolController(
      protocol as never,
      { warn: vi.fn() } as never,
      bootstrap as never
    )

    controller.register()

    expect([...handlers.keys()]).toEqual(['riftflow-api', 'riftflow-static'])

    const signal = new AbortController().signal
    const response = await handlers.get('riftflow-api')!(
      'notice/v1/latest?lang=en',
      new Request('riftflow://riftflow-api/notice/v1/latest?lang=en', {
        headers: {
          [RIFTFLOW_PROXY_REQUEST_ID_HEADER]: 'request-id',
          'x-test': 'test'
        }
      }),
      { signal }
    )

    expect(apiRequest).toHaveBeenCalledWith(
      expect.objectContaining({
        headers: { 'x-test': 'test' },
        method: 'GET',
        signal,
        url: 'notice/v1/latest?lang=en',
        validateStatus: expect.any(Function)
      })
    )
    expect(response.status).toBe(200)
    expect(await response.json()).toEqual({ ok: true })

    controller.unregister()

    expect(protocol.unregisterDomain).toHaveBeenNthCalledWith(1, 'riftflow-api')
    expect(protocol.unregisterDomain).toHaveBeenNthCalledWith(2, 'riftflow-static')
  })
})
