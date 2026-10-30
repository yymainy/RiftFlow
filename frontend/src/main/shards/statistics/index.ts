import { IRiftFlowShardInitDispose, Shard } from '@shared/riftflow-shard'

import { RiftFlowApiMain } from '../riftflow-api'
import { RiftFlowLogger, LoggerFactoryMain } from '../logger-factory'
import { SettingFactoryMain } from '../setting-factory'
import { SetterSettingService } from '../setting-factory/setter-setting-service'
import { STATISTICS_MAIN_NAMESPACE, type StatisticsMainContext } from './context'
import { VersionUsageRecorder } from './version-usage-recorder'

/**
 * 进行简单的数据统计
 */
@Shard(StatisticsMain.id)
export class StatisticsMain implements IRiftFlowShardInitDispose {
  static readonly id = STATISTICS_MAIN_NAMESPACE

  private _logger: RiftFlowLogger
  private _settingService: SetterSettingService
  private readonly _context: StatisticsMainContext
  private readonly _versionUsageRecorder: VersionUsageRecorder

  constructor(
    _loggerFactory: LoggerFactoryMain,
    _settingFactory: SettingFactoryMain,
    _riftflowApi: RiftFlowApiMain
  ) {
    this._logger = _loggerFactory.create(StatisticsMain.id)
    this._settingService = _settingFactory.register(StatisticsMain.id, {})
    this._context = {
      namespace: StatisticsMain.id,
      riftflowApi: _riftflowApi.api,
      logger: this._logger,
      settingService: this._settingService
    }
    this._versionUsageRecorder = new VersionUsageRecorder(this._context)
  }

  async onInit() {
    this._versionUsageRecorder.recordOnce().catch((e) => {
      // normally it should not happen
      this._logger.error('Oops... Something went wrong when counting visitors', e)
    })
  }
}
