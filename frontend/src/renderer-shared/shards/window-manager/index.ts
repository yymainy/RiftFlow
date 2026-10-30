import { Dep, IRiftFlowShardInitDispose, Shard } from '@shared/riftflow-shard'

import { RiftFlowIpcRenderer } from '../ipc'
import { LoggerRenderer } from '../logger'
import { PiniaMobxUtilsRenderer } from '../pinia-mobx-utils'
import { SettingUtilsRenderer } from '../setting-utils'
import {
  MAIN_SHARD_NAMESPACE,
  WINDOW_MANAGER_RENDERER_NAMESPACE,
  type WindowManagerRendererContext
} from './context'
import { useWindowManagerStore } from './store'
import {
  RiftFlowAuxWindow,
  RiftFlowCdTimerWindow,
  RiftFlowMainWindow,
  RiftFlowOngoingGameWindow,
  RiftFlowOpggWindow
} from './windows'

export {
  RiftFlowCdTimerWindow,
  RiftFlowOngoingGameWindow,
  RiftFlowOpggWindow,
  type WindowManagerRendererContext
}

@Shard(WindowManagerRenderer.id)
export class WindowManagerRenderer implements IRiftFlowShardInitDispose {
  static id = WINDOW_MANAGER_RENDERER_NAMESPACE

  private context: WindowManagerRendererContext

  public mainWindow: RiftFlowMainWindow
  public auxWindow: RiftFlowAuxWindow
  public opggWindow: RiftFlowOpggWindow
  public ongoingGameWindow: RiftFlowOngoingGameWindow
  public cdTimerWindow: RiftFlowCdTimerWindow

  constructor(
    @Dep(RiftFlowIpcRenderer) private readonly _ipc: RiftFlowIpcRenderer,
    @Dep(PiniaMobxUtilsRenderer) private readonly _piniaMobxUtils: PiniaMobxUtilsRenderer,
    @Dep(SettingUtilsRenderer) private readonly _settingUtils: SettingUtilsRenderer,
    @Dep(LoggerRenderer) readonly _logger: LoggerRenderer
  ) {
    this.context = {
      setting: this._settingUtils,
      ipc: this._ipc,
      pm: this._piniaMobxUtils
    }

    this.mainWindow = new RiftFlowMainWindow(this.context)
    this.auxWindow = new RiftFlowAuxWindow(this.context)
    this.opggWindow = new RiftFlowOpggWindow(this.context)
    this.ongoingGameWindow = new RiftFlowOngoingGameWindow(this.context)
    this.cdTimerWindow = new RiftFlowCdTimerWindow(this.context)
  }

  async onInit() {
    const store = useWindowManagerStore()
    await this.context.pm.sync(MAIN_SHARD_NAMESPACE, 'state', store)
    await this.context.pm.sync(MAIN_SHARD_NAMESPACE, 'settings', store.settings)

    await this.mainWindow.onInit()
    await this.auxWindow.onInit()
    await this.opggWindow.onInit()
    await this.ongoingGameWindow.onInit()
    await this.cdTimerWindow.onInit()
  }

  setBackgroundMaterial(value: string) {
    return this.context.setting.set(MAIN_SHARD_NAMESPACE, 'backgroundMaterial', value)
  }

  setContentProtection(value: boolean) {
    return this.context.setting.set(MAIN_SHARD_NAMESPACE, 'contentProtection', value)
  }
}
