import { SgpSummonerLol } from '@shared/types/sgp/summoner'
import { AxiosInstance } from 'axios'

import { SgpRegionParam } from './dto'
import {
  RIFTFLOW_HEADER_FORCE_STREAM_COLLECT,
  RIFTFLOW_HEADER_SGP_SERVER_ID,
  RIFTFLOW_HEADER_TOKEN_TYPE,
  URL_PLACEHOLDER_SUB_ID
} from './patterns'

export class SummonerLedgeHttpApi {
  constructor(private _http: AxiosInstance) {}

  postSummonersByPuuids(puuids: string[], options: SgpRegionParam = {}) {
    return this._http.post<SgpSummonerLol[]>(
      `/summoner-ledge/v1/regions/${URL_PLACEHOLDER_SUB_ID}/summoners/puuids`,
      puuids,
      {
        headers: {
          [RIFTFLOW_HEADER_SGP_SERVER_ID]: options.__sgpServerId,
          [RIFTFLOW_HEADER_TOKEN_TYPE]: 'league-session',
          [RIFTFLOW_HEADER_FORCE_STREAM_COLLECT]: 'true'
        },
        signal: options.signal
      }
    )
  }
}
