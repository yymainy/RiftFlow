import { SgpRankedStats } from '@shared/types/sgp/ranked'
import { AxiosInstance } from 'axios'

import { SgpRegionParam } from './dto'
import { RIFTFLOW_HEADER_SGP_SERVER_ID, RIFTFLOW_HEADER_TOKEN_TYPE } from './patterns'

export class LeaguesLedgeHttpApi {
  constructor(private _http: AxiosInstance) {}

  /**
   * 注：此 API 无法跨区
   */
  getRankedStatsByPuuid(puuid: string, options: SgpRegionParam = {}) {
    return this._http.get<SgpRankedStats>(`/leagues-ledge/v2/rankedStats/puuid/${puuid}`, {
      headers: {
        [RIFTFLOW_HEADER_SGP_SERVER_ID]: options.__sgpServerId,
        [RIFTFLOW_HEADER_TOKEN_TYPE]: 'league-session'
      },
      signal: options.signal
    })
  }
}
