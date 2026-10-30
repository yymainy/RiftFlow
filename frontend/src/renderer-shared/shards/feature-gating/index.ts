import { Dep, Shard } from '@shared/riftflow-shard'
import { FeatureGateEvaluator, isFeatureGateEnabled } from '@shared/shards/feature-gating'

import { RiftFlowApiRenderer } from '../riftflow-api'
import { useRiftFlowApiStore } from '../riftflow-api/store'
import { AppCommonRenderer } from '../app-common'
import { useAppCommonStore } from '../app-common/store'
import { useSgpStore } from '../sgp/store'

@Shard(FeatureGatingRenderer.id)
export class FeatureGatingRenderer {
  static readonly id = 'feature-gating-renderer'

  private readonly _evaluator = new FeatureGateEvaluator()

  constructor(
    @Dep(RiftFlowApiRenderer) _riftflowApi: RiftFlowApiRenderer,
    @Dep(AppCommonRenderer) _appCommon: AppCommonRenderer
  ) {}

  isEnabled(key: string, defaultValue: boolean) {
    const riftflowApi = useRiftFlowApiStore()
    const appCommon = useAppCommonStore()
    const sgp = useSgpStore()

    const evaluation = this._evaluator.evaluate(riftflowApi.featureGates, {
      platform: appCommon.platform,
      version: appCommon.version,
      sgpServerId: sgp.availability.sgpServerId
    })

    return isFeatureGateEnabled(key, defaultValue, evaluation)
  }
}
