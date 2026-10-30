import {
  type RiftFlowApiLanguage,
  type RiftFlowRelease,
  RiftFlowReleaseSchema
} from '@shared/shards/riftflow-api'

import type { RiftFlowApiMainContext } from './context'

export class RiftFlowApiReleaseLoader {
  private _updatePromise: Promise<RiftFlowRelease> | null = null

  constructor(private readonly _context: RiftFlowApiMainContext) {}

  updateLatestRelease(language: RiftFlowApiLanguage) {
    if (this._updatePromise) {
      return this._updatePromise
    }

    this._context.state.setUpdatingLatestRelease(true)

    const updatePromise = this._fetchLatestRelease(language)
      .then((release) => {
        this._context.state.setLatestRelease(release)
        this._context.logger.info('Updated latest release')
        return release
      })
      .catch((error) => {
        this._context.logger.warn('Update latest release failed', error)
        throw error
      })
      .finally(() => {
        this._context.state.setUpdatingLatestRelease(false)
        if (this._updatePromise === updatePromise) {
          this._updatePromise = null
        }
      })

    this._updatePromise = updatePromise
    return updatePromise
  }

  private async _fetchLatestRelease(language: RiftFlowApiLanguage): Promise<RiftFlowRelease> {
    const response = await this._context.api.getLatestRelease(language)
    return RiftFlowReleaseSchema.parse(response.data)
  }
}
