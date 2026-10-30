import { describe, expect, it } from 'vitest'

import { parseRiftFlowApiBootstrapDocument, resolveRiftFlowStaticUrl } from './base-urls'
import {
  DEFAULT_RIFTFLOW_API_BASE_URL,
  DEFAULT_RIFTFLOW_SERVICE_BASE_URLS,
  DEFAULT_RIFTFLOW_STATIC_BASE_URL
} from './types'

describe('RiftFlow API service discovery', () => {
  it('uses the RiftFlow service origins by default', () => {
    expect(DEFAULT_RIFTFLOW_SERVICE_BASE_URLS).toEqual({
      api: DEFAULT_RIFTFLOW_API_BASE_URL,
      static: DEFAULT_RIFTFLOW_STATIC_BASE_URL
    })
    expect(DEFAULT_RIFTFLOW_API_BASE_URL).toBe('https://riftflow-api.yymainy.online')
    expect(DEFAULT_RIFTFLOW_STATIC_BASE_URL).toBe('https://riftflow-api.yymainy.online/static')
  })

  it('parses a valid bootstrap document', () => {
    expect(
      parseRiftFlowApiBootstrapDocument({
        schemaVersion: 1,
        generation: 2,
        baseUrls: {
          api: 'https://riftflow-api.yymainy.online',
          static: 'https://riftflow-api.yymainy.online/static'
        }
      })
    ).toEqual({
      schemaVersion: 1,
      generation: 2,
      baseUrls: {
        api: 'https://riftflow-api.yymainy.online',
        static: 'https://riftflow-api.yymainy.online/static'
      }
    })
  })

  it('rejects an invalid bootstrap document', () => {
    expect(() => parseRiftFlowApiBootstrapDocument({ schemaVersion: 2 })).toThrow()
  })

  it('resolves and encodes static object names', () => {
    expect(
      resolveRiftFlowStaticUrl(DEFAULT_RIFTFLOW_STATIC_BASE_URL, 'RiftFlow Desktop-1.5.0-win.7z')
    ).toBe('https://riftflow-api.yymainy.online/static/RiftFlow%20Desktop-1.5.0-win.7z')
  })

  it('allows an authoritative absolute static URL', () => {
    expect(
      resolveRiftFlowStaticUrl(DEFAULT_RIFTFLOW_STATIC_BASE_URL, 'https://cdn.example.com/file.7z')
    ).toBe('https://cdn.example.com/file.7z')
  })
})
