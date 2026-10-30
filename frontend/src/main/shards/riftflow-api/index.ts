import { IRiftFlowShardInitDispose, Shard } from '@shared/riftflow-shard'
import type { RiftFlowApiLanguage } from '@shared/shards/riftflow-api'

import { RiftFlowProtocolMain } from '../riftflow-protocol'
import { AppCommonMain } from '../app-common'
import { RiftFlowLogger, LoggerFactoryMain } from '../logger-factory'
import { MobxUtilsMain } from '../mobx-utils'
import { SettingFactoryMain } from '../setting-factory'
import { SetterSettingService } from '../setting-factory/setter-setting-service'
import { RiftFlowApiBootstrapController } from './bootstrap-controller'
import { RiftFlowApiConfigLoader } from './config-loader'
import type { RiftFlowApiMainContext } from './context'
import { RiftFlowApiNoticeLoader } from './notice-loader'
import { RiftFlowApiProtocolController } from './protocol-controller'
import { RiftFlowApiReleaseLoader } from './release-loader'
import { RiftFlowApiState } from './state'

@Shard(RiftFlowApiMain.id)
export class RiftFlowApiMain implements IRiftFlowShardInitDispose {
  static readonly id = 'riftflow-api-main'

  public readonly state = new RiftFlowApiState()

  private readonly _logger: RiftFlowLogger
  private readonly _settingService: SetterSettingService
  private readonly _context: RiftFlowApiMainContext
  private readonly _bootstrapController: RiftFlowApiBootstrapController
  private readonly _protocolController: RiftFlowApiProtocolController
  private readonly _configLoader: RiftFlowApiConfigLoader
  private readonly _noticeLoader: RiftFlowApiNoticeLoader
  private readonly _releaseLoader: RiftFlowApiReleaseLoader

  get api() {
    return this._bootstrapController.api
  }

  get staticAssets() {
    return this._bootstrapController.staticAssets
  }

  constructor(
    _loggerFactory: LoggerFactoryMain,
    _settingFactory: SettingFactoryMain,
    _protocol: RiftFlowProtocolMain,
    _mobxUtils: MobxUtilsMain,
    _appCommon: AppCommonMain
  ) {
    this._logger = _loggerFactory.create(RiftFlowApiMain.id)
    this._settingService = _settingFactory.register(RiftFlowApiMain.id)

    this._bootstrapController = new RiftFlowApiBootstrapController(this._settingService, this._logger)
    this._protocolController = new RiftFlowApiProtocolController(
      _protocol,
      this._logger,
      this._bootstrapController
    )
    this._context = {
      state: this.state,
      logger: this._logger,
      settingService: this._settingService,
      mobxUtils: _mobxUtils,
      appCommon: _appCommon,
      api: this.api
    }
    this._configLoader = new RiftFlowApiConfigLoader(this._context)
    this._noticeLoader = new RiftFlowApiNoticeLoader(this._context)
    this._releaseLoader = new RiftFlowApiReleaseLoader(this._context)
  }

  async onInit() {
    await this._bootstrapController.init()
    this._setupState()

    try {
      await this._configLoader.initFromLocal()
    } catch (error) {
      this._logger.warn('Failed to load config cache', error)
    }

    this._protocolController.register()
    this._configLoader.watch()
    this._noticeLoader.watch()
  }

  async onDispose() {
    this._configLoader.dispose()
    this._noticeLoader.dispose()
    this._protocolController.unregister()
  }

  updateLatestRelease(language: RiftFlowApiLanguage) {
    return this._releaseLoader.updateLatestRelease(language)
  }

  private _setupState() {
    this._context.mobxUtils.propSync(RiftFlowApiMain.id, 'state', this.state, [
      'featureGates',
      'notice',
      'contactChannels'
    ])
  }
}
