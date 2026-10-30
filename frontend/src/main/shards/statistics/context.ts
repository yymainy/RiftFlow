import type { RiftFlowApiHttpApiAxiosHelper } from '@shared/http-api-axios-helper/riftflow/api'

import type { RiftFlowLogger } from '../logger-factory'
import type { SetterSettingService } from '../setting-factory/setter-setting-service'

export const STATISTICS_MAIN_NAMESPACE = 'statistics-main'

export interface StatisticsMainContext {
  namespace: string
  riftflowApi: RiftFlowApiHttpApiAxiosHelper
  logger: RiftFlowLogger
  settingService: SetterSettingService
}
