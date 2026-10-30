import { RiftFlowApiHttpApiAxiosHelper } from '@shared/http-api-axios-helper/riftflow/api'
import { RiftFlowStaticHttpApiAxiosHelper } from '@shared/http-api-axios-helper/riftflow/static'
import {
  type RiftFlowApiBootstrapDocument,
  DEFAULT_RIFTFLOW_SERVICE_BASE_URLS,
  parseRiftFlowApiBootstrapDocument
} from '@shared/shards/riftflow-api'
import axios, { type AxiosInstance } from 'axios'
import { app } from 'electron'

import type { RiftFlowLogger } from '../logger-factory'
import type { SetterSettingService } from '../setting-factory/setter-setting-service'

export const RIFTFLOW_API_BOOTSTRAP_CACHE_PATH = 'bootstrap.json'
export const RIFTFLOW_API_BOOTSTRAP_NPM_LATEST_URL = process.env.RIFTFLOW_BOOTSTRAP_URL || ''

const RIFTFLOW_API_REQUEST_TIMEOUT = 10_000

interface NpmLatestMetadata {
  riftflowBootstrap: unknown
}

export class RiftFlowApiBootstrapController {
  private _generation: number | null = null
  private readonly _npmHttp: AxiosInstance

  public readonly apiHttp = axios.create({
    baseURL: DEFAULT_RIFTFLOW_SERVICE_BASE_URLS.api,
    timeout: RIFTFLOW_API_REQUEST_TIMEOUT,
    headers: {
      Accept: 'application/json',
      'User-Agent': `RiftFlow/${app.getVersion()}`,
      'x-riftflow-version': app.getVersion()
    }
  })
  public readonly staticHttp = axios.create({
    baseURL: DEFAULT_RIFTFLOW_SERVICE_BASE_URLS.static,
    timeout: RIFTFLOW_API_REQUEST_TIMEOUT,
    headers: {
      'User-Agent': `RiftFlow/${app.getVersion()}`,
      'x-riftflow-version': app.getVersion()
    }
  })
  public readonly api = new RiftFlowApiHttpApiAxiosHelper(this.apiHttp)
  public readonly staticAssets = new RiftFlowStaticHttpApiAxiosHelper(this.staticHttp)

  constructor(
    private readonly _settingService: SetterSettingService,
    private readonly _logger: RiftFlowLogger,
    npmHttp?: AxiosInstance
  ) {
    this._npmHttp =
      npmHttp ??
      axios.create({
        timeout: RIFTFLOW_API_REQUEST_TIMEOUT,
        headers: {
          'User-Agent': `RiftFlow/${app.getVersion()}`
        }
      })
  }

  async init() {
    try {
      await this._loadFromLocal()
    } catch (error) {
      this._logger.warn('Failed to load bootstrap from local cache', error)
    }

    if (RIFTFLOW_API_BOOTSTRAP_NPM_LATEST_URL) void this._updateFromNpm()
  }

  private async _loadFromLocal() {
    if (!(await this._settingService.jsonConfigFileExists(RIFTFLOW_API_BOOTSTRAP_CACHE_PATH))) {
      return
    }

    let bootstrap: RiftFlowApiBootstrapDocument
    try {
      const cached = await this._settingService.readFromJsonConfigFile(
        RIFTFLOW_API_BOOTSTRAP_CACHE_PATH
      )
      bootstrap = parseRiftFlowApiBootstrapDocument(cached)
    } catch (error) {
      try {
        await this._settingService.deleteJsonConfigFile(RIFTFLOW_API_BOOTSTRAP_CACHE_PATH)
      } catch (deleteError) {
        this._logger.warn('Failed to delete invalid bootstrap cache', deleteError)
      }
      throw error
    }

    this._applyBootstrap(bootstrap)
    this._logger.info(`Loaded bootstrap generation ${bootstrap.generation}`)
  }

  private async _updateFromNpm() {
    try {
      const bootstrap = await this._fetchLatestBootstrap()
      if (this._generation !== null && bootstrap.generation <= this._generation) {
        this._logger.info(`Bootstrap generation ${this._generation} is up to date`)
        return
      }

      this._applyBootstrap(bootstrap)
      await this._settingService.writeToJsonConfigFile(RIFTFLOW_API_BOOTSTRAP_CACHE_PATH, bootstrap)
      this._logger.info(`Updated bootstrap to generation ${bootstrap.generation}`)
    } catch (error) {
      this._logger.warn('Failed to update bootstrap from npm', error)
    }
  }

  private async _fetchLatestBootstrap() {
    const metadataResponse = await this._npmHttp.get<NpmLatestMetadata>(
      RIFTFLOW_API_BOOTSTRAP_NPM_LATEST_URL
    )
    return parseRiftFlowApiBootstrapDocument(metadataResponse.data.riftflowBootstrap)
  }

  private _applyBootstrap(bootstrap: RiftFlowApiBootstrapDocument) {
    this.apiHttp.defaults.baseURL = bootstrap.baseUrls.api
    this.staticHttp.defaults.baseURL = bootstrap.baseUrls.static
    this._generation = bootstrap.generation
  }
}
