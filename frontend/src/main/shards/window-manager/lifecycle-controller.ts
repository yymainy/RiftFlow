import type { RiftFlowAuxWindow } from './aux-window/window'
import type { RiftFlowCdTimerWindow } from './cd-timer-window/windows'
import type { WindowManagerMainContext } from './context'
import type { RiftFlowMainWindow } from './main-window/window'
import type { RiftFlowOngoingGameWindow } from './ongoing-game-window/window'
import type { RiftFlowOpggWindow } from './opgg-window/window'

interface WindowManagerWindows {
  mainWindow: RiftFlowMainWindow
  auxWindow: RiftFlowAuxWindow
  opggWindow: RiftFlowOpggWindow
  ongoingGameWindow: RiftFlowOngoingGameWindow
  cdTimerWindow: RiftFlowCdTimerWindow
}

export class WindowManagerLifecycleController {
  constructor(
    private readonly _context: WindowManagerMainContext,
    private readonly _windows: WindowManagerWindows
  ) {}

  async init() {
    await this._context.settingService.applyToState()

    if (this._context.shared.global.isWindows11_22H2_OrHigher) {
      this._context.windowManager.state.setSupportsMica(true)
    }

    this._context.mobxUtils.propSync(
      this._context.namespace,
      'state',
      this._context.windowManager.state,
      ['supportsMica', 'downloadTasks']
    )
    this._context.mobxUtils.propSync(
      this._context.namespace,
      'settings',
      this._context.windowManager.settings,
      ['backgroundMaterial', 'contentProtection']
    )

    this._windows.mainWindow.on('main-window-close', () => {
      this._context.shared.global.quit()
    })

    await this._windows.mainWindow.onInit()
    await this._windows.auxWindow.onInit()
    await this._windows.opggWindow.onInit()
    await this._windows.ongoingGameWindow.onInit()
    await this._windows.cdTimerWindow.onInit()
  }

  finish() {
    this._context.shared.global.events.on('second-instance', () => {
      this._windows.mainWindow.showOrRestore()
    })
    this._context.windowManager.state.setManagerFinishedInit(true)
    this._windows.mainWindow.createWindow()
  }
}
