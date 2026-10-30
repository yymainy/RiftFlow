import { Dep, Shard } from '@shared/riftflow-shard'
import { RiftFlowApiHttpApiAxiosHelper } from '@shared/http-api-axios-helper/riftflow/api'
import { RiftFlowStaticHttpApiAxiosHelper } from '@shared/http-api-axios-helper/riftflow/static'
import axios from 'axios'

import { RiftFlowProtocolRenderer } from '../riftflow-protocol'
import { PiniaMobxUtilsRenderer } from '../pinia-mobx-utils'
import { useRiftFlowApiStore } from './store'

@Shard(RiftFlowApiRenderer.id)
export class RiftFlowApiRenderer {
  static readonly id = 'riftflow-api-renderer'
  static readonly mainId = 'riftflow-api-main'

  public readonly httpClient = axios.create({
    adapter: 'fetch',
    baseURL: 'riftflow://riftflow-api',
    paramsSerializer: { indexes: null }
  })
  public readonly staticHttpClient = axios.create({
    adapter: 'fetch',
    baseURL: 'riftflow://riftflow-static'
  })
  public readonly api = new RiftFlowApiHttpApiAxiosHelper(this.httpClient)
  public readonly staticAssets = new RiftFlowStaticHttpApiAxiosHelper(this.staticHttpClient)

  constructor(
    @Dep(RiftFlowProtocolRenderer) riftflowProtocol: RiftFlowProtocolRenderer,
    @Dep(PiniaMobxUtilsRenderer)
    private readonly _piniaMobxUtils: PiniaMobxUtilsRenderer
  ) {
    riftflowProtocol.installProxyRequestCancellation(this.httpClient)
    riftflowProtocol.installProxyRequestCancellation(this.staticHttpClient)
  }

  async onInit() {
    const store = useRiftFlowApiStore()
    await this._piniaMobxUtils.sync(RiftFlowApiRenderer.mainId, 'state', store)
  }
}
