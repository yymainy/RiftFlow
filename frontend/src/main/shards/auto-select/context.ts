import type { RiftFlowIpcMain } from '../ipc'
import type { LeagueClientMain } from '../league-client'
import type { RiftFlowLogger } from '../logger-factory'
import type { MobxUtilsMain } from '../mobx-utils'
import type { SetterSettingService } from '../setting-factory/setter-setting-service'
import type { AutoSelectSettings, AutoSelectState } from './state'

export const AUTO_SELECT_MAIN_NAMESPACE = 'auto-select-main'

export interface AutoSelectMainContext {
  namespace: string
  settings: AutoSelectSettings
  state: AutoSelectState
  logger: RiftFlowLogger
  settingService: SetterSettingService
  leagueClient: LeagueClientMain
  mobxUtils: MobxUtilsMain
  ipc: RiftFlowIpcMain
}
