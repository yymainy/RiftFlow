import type { RiftFlowProtocolMain } from '../riftflow-protocol'
import type { RiftFlowIpcMain } from '../ipc'
import type { LeagueClientMain } from '../league-client'
import type { RiftFlowLogger } from '../logger-factory'
import type { MobxUtilsMain } from '../mobx-utils'
import type { RiotClientMain } from './index'

export const RIOT_CLIENT_MAIN_NAMESPACE = 'riot-client-main'
export const RIOT_CLIENT_REQUEST_TIMEOUT_MS = 17500

export class RiotClientRcuUninitializedError extends Error {
  name = 'RiotClientRcuUninitializedError'
}

export interface RiotClientMainContext {
  namespace: string
  ipc: RiftFlowIpcMain
  leagueClient: LeagueClientMain
  logger: RiftFlowLogger
  mobxUtils: MobxUtilsMain
  protocol: RiftFlowProtocolMain
  riotClient: RiotClientMain
}
