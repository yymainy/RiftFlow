import type { SharedGlobalShard } from '@shared/riftflow-shard'

import type { WindowManagerMain } from '.'
import type { RiftFlowProtocolMain } from '../riftflow-protocol'
import type { AppCommonMain } from '../app-common'
import type { GameClientMain } from '../game-client'
import type { RiftFlowIpcMain } from '../ipc'
import type { KeyboardShortcutsMain } from '../keyboard-shortcuts'
import type { LeagueClientMain } from '../league-client'
import type { RiftFlowLogger, LoggerFactoryMain } from '../logger-factory'
import type { MobxUtilsMain } from '../mobx-utils'
import type { SelfUpdateMain } from '../self-update'
import type { SettingFactoryMain } from '../setting-factory'
import type { SetterSettingService } from '../setting-factory/setter-setting-service'

export const WINDOW_MANAGER_MAIN_NAMESPACE = 'window-manager-main'

export interface WindowManagerMainContext {
  namespace: string
  windowManagerClass: typeof WindowManagerMain
  windowManager: WindowManagerMain
  appCommon: AppCommonMain
  ipc: RiftFlowIpcMain
  settingService: SetterSettingService
  settingFactory: SettingFactoryMain
  loggerFactory: LoggerFactoryMain
  leagueClient: LeagueClientMain
  protocol: RiftFlowProtocolMain
  mobxUtils: MobxUtilsMain
  logger: RiftFlowLogger
  gameClient: GameClientMain
  keyboardShortcuts: KeyboardShortcutsMain
  selfUpdate: SelfUpdateMain
  shared: SharedGlobalShard
}
