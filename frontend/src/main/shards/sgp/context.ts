import type { AxiosInstance } from 'axios'

import type { RiftFlowProtocolMain } from '../riftflow-protocol'
import type { AppCommonMain } from '../app-common'
import type { LeagueClientMain } from '../league-client'
import type { RiftFlowLogger } from '../logger-factory'
import type { MobxUtilsMain } from '../mobx-utils'
import type { SgpState } from './state'

export const SGP_MAIN_NAMESPACE = 'sgp-main'

export interface SgpMainContext {
  namespace: string
  appCommon: AppCommonMain
  httpClient: AxiosInstance
  leagueClient: LeagueClientMain
  logger: RiftFlowLogger
  mobxUtils: MobxUtilsMain
  protocol: RiftFlowProtocolMain
  state: SgpState
}
