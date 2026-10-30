import {
  type RiftFlowApiConfigResource,
  type RiftFlowApiConfigResourceMap,
  type RiftFlowApiLanguage,
  type RiftFlowContactChannels,
  type RiftFlowNotice,
  type RiftFlowRelease,
  type RiftFlowStatisticsRecordCreated,
  DEFAULT_RIFTFLOW_API_BASE_URL,
  DEFAULT_RIFTFLOW_API_LANGUAGE
} from '@shared/shards/riftflow-api'
import type { AxiosInstance } from 'axios'

import type { HttpApiRequestOptions } from '../request-options'

export class RiftFlowApiHttpApiAxiosHelper {
  constructor(private _http: AxiosInstance) {
    if (!_http.defaults.baseURL) {
      _http.defaults.baseURL = DEFAULT_RIFTFLOW_API_BASE_URL
    }
  }

  getLatestNotice(
    language: RiftFlowApiLanguage = DEFAULT_RIFTFLOW_API_LANGUAGE,
    options: HttpApiRequestOptions = {}
  ) {
    return this._http.get<RiftFlowNotice>('/notice/v1/latest', {
      params: { lang: language },
      signal: options.signal
    })
  }

  getContactChannels(options: HttpApiRequestOptions = {}) {
    return this._http.get<RiftFlowContactChannels>('/website/v1/contact-channels', {
      signal: options.signal
    })
  }

  getConfig<Resource extends RiftFlowApiConfigResource>(
    resource: Resource,
    options: HttpApiRequestOptions = {}
  ) {
    return this._http.get<RiftFlowApiConfigResourceMap[Resource]>(`/config/v1/${resource}`, {
      signal: options.signal
    })
  }

  getLatestRelease(
    language: RiftFlowApiLanguage = DEFAULT_RIFTFLOW_API_LANGUAGE,
    options: HttpApiRequestOptions = {}
  ) {
    return this._http.get<RiftFlowRelease>('/releases/v1/latest', {
      params: { lang: language },
      signal: options.signal
    })
  }

  postStatisticsRecord(version: string, options: HttpApiRequestOptions = {}) {
    return this._http.post<RiftFlowStatisticsRecordCreated>(
      '/statistics/v1/records',
      { version },
      { signal: options.signal }
    )
  }
}
