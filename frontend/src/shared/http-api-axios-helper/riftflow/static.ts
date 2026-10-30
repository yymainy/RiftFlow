import { DEFAULT_RIFTFLOW_STATIC_BASE_URL, resolveRiftFlowStaticUrl } from '@shared/shards/riftflow-api'
import type { AxiosInstance } from 'axios'

import type { HttpApiRequestOptions } from '../request-options'

export class RiftFlowStaticHttpApiAxiosHelper {
  constructor(private readonly _http: AxiosInstance) {
    if (!_http.defaults.baseURL) {
      _http.defaults.baseURL = DEFAULT_RIFTFLOW_STATIC_BASE_URL
    }
  }

  resolveUrl(path: string) {
    return resolveRiftFlowStaticUrl(this._http.defaults.baseURL!, path)
  }

  getFile(path: string, options: HttpApiRequestOptions = {}) {
    return this._http.get(this.resolveUrl(path), { signal: options.signal })
  }
}
