import type { RiftFlowRelease } from '@shared/shards/riftflow-api'
import { afterEach, describe, expect, it, vi } from 'vitest'

import type { RiftFlowApiMainContext } from './context'
import { RiftFlowApiReleaseLoader } from './release-loader'
import { RiftFlowApiState } from './state'

const latestRelease: RiftFlowRelease = {
  version: '1.5.0',
  publishedAt: '2026-07-16T06:00:00.000Z',
  description: '更新内容',
  artifacts: [
    {
      platform: 'win32',
      arch: 'x64',
      fileName: 'RiftFlow Desktop-1.5.0-win.7z',
      size: 2048,
      contentType: 'application/x-7z-compressed',
      sha256: null,
      downloadUrl: 'https://riftflow-static.yuru-yuri.com/RiftFlow%20Desktop-1.5.0-win.7z'
    }
  ]
}

function createContext(getLatestRelease = vi.fn().mockResolvedValue({ data: latestRelease })) {
  return {
    api: { getLatestRelease },
    appCommon: { settings: { locale: 'zh-CN' } },
    logger: { info: vi.fn(), warn: vi.fn() },
    state: new RiftFlowApiState()
  } as unknown as RiftFlowApiMainContext
}

afterEach(() => {
  vi.restoreAllMocks()
})

describe('RiftFlow API release loader', () => {
  it('keeps the native release document unchanged', async () => {
    const context = createContext()
    const loader = new RiftFlowApiReleaseLoader(context)

    const release = await loader.updateLatestRelease('zh-CN')

    expect(release).toEqual(latestRelease)
    expect(context.state.latestRelease).toEqual(latestRelease)
  })

  it('does not filter artifacts at the RiftFlow API boundary', async () => {
    const unsupportedRelease: RiftFlowRelease = {
      ...latestRelease,
      artifacts: [
        {
          ...latestRelease.artifacts[0],
          contentType: 'application/octet-stream',
          fileName: 'RiftFlow-1.5.0-win.zip'
        }
      ]
    }
    const context = createContext(vi.fn().mockResolvedValue({ data: unsupportedRelease }))
    const loader = new RiftFlowApiReleaseLoader(context)

    await loader.updateLatestRelease('zh-CN')

    expect(context.state.latestRelease).toEqual(unsupportedRelease)
  })

  it('shares an in-flight request without changing its data', async () => {
    const getLatestRelease = vi.fn().mockResolvedValue({ data: latestRelease })
    const context = createContext(getLatestRelease)
    const loader = new RiftFlowApiReleaseLoader(context)

    const firstUpdate = loader.updateLatestRelease('zh-CN')
    const secondUpdate = loader.updateLatestRelease('zh-CN')

    await expect(firstUpdate).resolves.toEqual(latestRelease)
    await expect(secondUpdate).resolves.toEqual(latestRelease)
    expect(getLatestRelease).toHaveBeenCalledTimes(1)
  })

  it('tracks whether the latest release is being updated', async () => {
    let resolveRequest!: (value: { data: RiftFlowRelease }) => void
    const getLatestRelease = vi.fn().mockReturnValue(
      new Promise<{ data: RiftFlowRelease }>((resolve) => {
        resolveRequest = resolve
      })
    )
    const context = createContext(getLatestRelease)
    const loader = new RiftFlowApiReleaseLoader(context)

    const update = loader.updateLatestRelease('zh-CN')

    expect(context.state.isUpdatingLatestRelease).toBe(true)
    resolveRequest({ data: latestRelease })
    await update
    expect(context.state.isUpdatingLatestRelease).toBe(false)
  })

  it('does not fall back to the legacy endpoint when the request fails', async () => {
    const context = createContext(vi.fn().mockRejectedValue(new Error('request failed')))
    const loader = new RiftFlowApiReleaseLoader(context)

    await expect(loader.updateLatestRelease('zh-CN')).rejects.toThrow('request failed')

    expect(context.state.latestRelease).toBeNull()
    expect(context.logger.warn).toHaveBeenCalled()
  })
})
