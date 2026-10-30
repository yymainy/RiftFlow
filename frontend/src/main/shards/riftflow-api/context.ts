import type { RiftFlowApiHttpApiAxiosHelper } from '@shared/http-api-axios-helper/riftflow/api'

import type { AppCommonMain } from '../app-common'
import type { RiftFlowLogger } from '../logger-factory'
import type { MobxUtilsMain } from '../mobx-utils'
import type { SetterSettingService } from '../setting-factory/setter-setting-service'
import type { RiftFlowApiState } from './state'

export const RIFTFLOW_API_CACHED_RESOURCE_UPDATE_INTERVAL = 2 * 60 * 60 * 1000
export const RIFTFLOW_API_FEATURE_GATES_UPDATE_INTERVAL = 2 * 60 * 60 * 1000
export const RIFTFLOW_API_NOTICE_UPDATE_INTERVAL = 60 * 60 * 1000

export interface RiftFlowApiMainContext {
  state: RiftFlowApiState
  logger: RiftFlowLogger
  settingService: SetterSettingService
  mobxUtils: MobxUtilsMain
  appCommon: AppCommonMain
  api: RiftFlowApiHttpApiAxiosHelper
}
