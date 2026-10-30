import { IRiftFlowShardInitDispose, Shard, SharedGlobalShard } from '@shared/riftflow-shard'
import { z } from 'zod'

import { RiftFlowProtocolMain } from '../riftflow-protocol'
import { AppCommonMain } from '../app-common'
import { GameClientMain } from '../game-client'
import { RiftFlowIpcMain } from '../ipc'
import { KeyboardShortcutsMain } from '../keyboard-shortcuts'
import { LeagueClientMain } from '../league-client'
import { RiftFlowLogger, LoggerFactoryMain } from '../logger-factory'
import { MobxUtilsMain } from '../mobx-utils'
import { SelfUpdateMain } from '../self-update'
import { SettingFactoryMain } from '../setting-factory'
import { SetterSettingService } from '../setting-factory/setter-setting-service'
import { RiftFlowAuxWindow } from './aux-window/window'
import { settingToNativeBackgroundMaterial } from './background-material-resolver'
import { RiftFlowCdTimerWindow } from './cd-timer-window/windows'
import { WINDOW_MANAGER_MAIN_NAMESPACE, type WindowManagerMainContext } from './context'
import { WindowManagerLifecycleController } from './lifecycle-controller'
import { RiftFlowMainWindow } from './main-window/window'
import { RiftFlowOngoingGameWindow } from './ongoing-game-window/window'
import { RiftFlowOpggWindow } from './opgg-window/window'
import { WindowManagerSettings, WindowManagerState } from './state'

@Shard(WindowManagerMain.id)
export class WindowManagerMain implements IRiftFlowShardInitDispose {
  static id = WINDOW_MANAGER_MAIN_NAMESPACE

  private readonly _logger: RiftFlowLogger
  private readonly _settingService: SetterSettingService
  private readonly _context: WindowManagerMainContext
  private readonly _lifecycleController: WindowManagerLifecycleController

  public readonly settings = new WindowManagerSettings()
  public readonly state = new WindowManagerState()

  public readonly mainWindow: RiftFlowMainWindow
  public readonly auxWindow: RiftFlowAuxWindow
  public readonly opggWindow: RiftFlowOpggWindow
  public readonly ongoingGameWindow: RiftFlowOngoingGameWindow
  public readonly cdTimerWindow: RiftFlowCdTimerWindow

  constructor(
    private readonly _ipc: RiftFlowIpcMain,
    private readonly _mobxUtils: MobxUtilsMain,
    private readonly _loggerFactory: LoggerFactoryMain,
    private readonly _settingFactory: SettingFactoryMain,
    private readonly _leagueClient: LeagueClientMain,
    private readonly _shared: SharedGlobalShard,
    private readonly _protocol: RiftFlowProtocolMain,
    private readonly _keyboardShortcuts: KeyboardShortcutsMain,
    private readonly _appCommon: AppCommonMain,
    private readonly _gameClient: GameClientMain,
    private readonly _selfUpdate: SelfUpdateMain
  ) {
    this._logger = _loggerFactory.create(WindowManagerMain.id)
    this._settingService = _settingFactory.register(
      WindowManagerMain.id,
      {
        backgroundMaterial: {
          default: this.settings.backgroundMaterial,
          schema: z.enum(['mica', 'none'])
        },
        contentProtection: { default: this.settings.contentProtection, schema: z.boolean() }
      },
      this.settings
    )

    this._context = this.getContext()
    this.mainWindow = new RiftFlowMainWindow(this._context)
    this.auxWindow = new RiftFlowAuxWindow(this._context)
    this.opggWindow = new RiftFlowOpggWindow(this._context)
    this.ongoingGameWindow = new RiftFlowOngoingGameWindow(this._context)
    this.cdTimerWindow = new RiftFlowCdTimerWindow(this._context)
    this._lifecycleController = new WindowManagerLifecycleController(this._context, {
      mainWindow: this.mainWindow,
      auxWindow: this.auxWindow,
      opggWindow: this.opggWindow,
      ongoingGameWindow: this.ongoingGameWindow,
      cdTimerWindow: this.cdTimerWindow
    })
  }

  getContext(): WindowManagerMainContext {
    return {
      namespace: WindowManagerMain.id,
      windowManagerClass: WindowManagerMain,
      appCommon: this._appCommon,
      windowManager: this,
      ipc: this._ipc,
      settingService: this._settingService,
      settingFactory: this._settingFactory,
      loggerFactory: this._loggerFactory,
      leagueClient: this._leagueClient,
      protocol: this._protocol,
      mobxUtils: this._mobxUtils,
      logger: this._logger,
      gameClient: this._gameClient,
      keyboardShortcuts: this._keyboardShortcuts,
      selfUpdate: this._selfUpdate,
      shared: this._shared
    }
  }

  async onInit() {
    await this._lifecycleController.init()
  }

  async onFinish() {
    this._lifecycleController.finish()
  }

  /**
   * 设置项的背景材质转换为原生系统级别背景材质
   */
  _settingToNativeBackgroundMaterial(material: string) {
    // fixed in v35.0.0, https://github.com/electron/electron/pull/45525
    // if (material === 'mica' && process.env['NODE_ENV'] !== 'development') {
    //   this._logger.warn(
    //     'Mica is disabled in production mode. (https://github.com/electron/electron/issues/41824)'
    //   )
    //   return 'none'
    // }

    return settingToNativeBackgroundMaterial(material, this.state.supportsMica)
  }
}
