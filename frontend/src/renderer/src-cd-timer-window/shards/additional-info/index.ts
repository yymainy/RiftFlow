import { PiniaMobxUtilsRenderer } from '@renderer-shared/shards/pinia-mobx-utils'
import { Dep, IRiftFlowShardInitDispose, Shard } from '@shared/riftflow-shard'

import { ADDITIONAL_INFO_RENDERER_NAMESPACE, type AdditionalInfoRendererContext } from './context'
import { syncAdditionalInfoState } from './state-sync'

@Shard(AdditionalInfoShard.id)
export class AdditionalInfoShard implements IRiftFlowShardInitDispose {
  static id = ADDITIONAL_INFO_RENDERER_NAMESPACE

  private readonly _context: AdditionalInfoRendererContext

  constructor(@Dep(PiniaMobxUtilsRenderer) piniaMobxUtils: PiniaMobxUtilsRenderer) {
    this._context = { piniaMobxUtils }
  }

  async onInit() {
    syncAdditionalInfoState(this._context)
  }
}
