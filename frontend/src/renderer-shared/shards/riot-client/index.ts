import { Dep, IRiftFlowShardInitDispose, Shard } from '@shared/riftflow-shard'
import type { RiotClientHttpApiAxiosHelper } from '@shared/http-api-axios-helper/riot-client'
import type { AxiosInstance } from 'axios'

import { RiftFlowProtocolRenderer } from '../riftflow-protocol'
import { PiniaMobxUtilsRenderer } from '../pinia-mobx-utils'
import { RIOT_CLIENT_RENDERER_NAMESPACE } from './context'
import { createRiotClientHttpApi } from './http-api'

@Shard(RiotClientRenderer.id)
export class RiotClientRenderer implements IRiftFlowShardInitDispose {
  static id = RIOT_CLIENT_RENDERER_NAMESPACE

  public readonly httpClient: AxiosInstance
  public readonly api: RiotClientHttpApiAxiosHelper

  async onInit() {}

  constructor(
    @Dep(PiniaMobxUtilsRenderer) _piniaMobxUtils: PiniaMobxUtilsRenderer,
    @Dep(RiftFlowProtocolRenderer) riftflowProtocol: RiftFlowProtocolRenderer
  ) {
    const { httpClient, api } = createRiotClientHttpApi()

    riftflowProtocol.installProxyRequestCancellation(httpClient)

    this.httpClient = httpClient
    this.api = api
  }
}
