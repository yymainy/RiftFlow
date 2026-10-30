import { describe, expect, test } from 'vitest'

import {
  RIFTFLOW_PROXY_REQUEST_ID_HEADER,
  RiftFlowProtocolProxyRequestCancellation
} from './proxy-request-cancellation'

describe('RiftFlowProtocolProxyRequestCancellation', () => {
  test('aborts and clears a registered proxy request by request id', () => {
    const cancellation = new RiftFlowProtocolProxyRequestCancellation()
    const request = new Request('riftflow://league-client/lol-gameflow/v1/session', {
      headers: {
        [RIFTFLOW_PROXY_REQUEST_ID_HEADER]: 'request-1'
      }
    })

    const registration = cancellation.register(request)

    expect(registration?.requestId).toBe('request-1')
    expect(registration?.signal.aborted).toBe(false)

    expect(cancellation.cancel('request-1')).toBe(true)
    expect(registration?.signal.aborted).toBe(true)

    cancellation.dispose(registration!)
    expect(cancellation.cancel('request-1')).toBe(false)
  })

  test('ignores requests without a proxy request id', () => {
    const cancellation = new RiftFlowProtocolProxyRequestCancellation()
    const request = new Request('riftflow://league-client/lol-gameflow/v1/session')

    expect(cancellation.register(request)).toBeNull()
    expect(cancellation.cancel('missing')).toBe(false)
  })
})
