import { createManager } from '@renderer-shared/shards'
import { RiftFlowApiRenderer } from '@renderer-shared/shards/riftflow-api'
import { RiftFlowProtocolRenderer } from '@renderer-shared/shards/riftflow-protocol'
import { AppCommonRenderer } from '@renderer-shared/shards/app-common'
import { FeatureGatingRenderer } from '@renderer-shared/shards/feature-gating'
import { RiftFlowIpcRenderer } from '@renderer-shared/shards/ipc'
import {
  LeagueClientRenderer,
  LeagueClientRendererConfig
} from '@renderer-shared/shards/league-client'
import { LoggerRenderer } from '@renderer-shared/shards/logger'
import { PiniaMobxUtilsRenderer } from '@renderer-shared/shards/pinia-mobx-utils'
import { SettingUtilsRenderer } from '@renderer-shared/shards/setting-utils'
import { SetupInAppScopeRenderer } from '@renderer-shared/shards/setup-in-app-scope'
import { SgpRenderer } from '@renderer-shared/shards/sgp'
import { WindowManagerRenderer } from '@renderer-shared/shards/window-manager'

import { AdditionalInfoShard } from './additional-info'

const manager = createManager()

manager.use(AdditionalInfoShard)
manager.use(RiftFlowIpcRenderer)
manager.use(RiftFlowApiRenderer)
manager.use(RiftFlowProtocolRenderer)
manager.use(AppCommonRenderer)
manager.use(FeatureGatingRenderer)
manager.use(LeagueClientRenderer, {
  // for better performance
  subscribeState: {
    gameData: true,
    gameflow: true,
    summoner: true,
    matchmaking: false,
    lobby: false,
    login: false,
    champSelect: false,
    chat: false,
    honor: false
  }
} as LeagueClientRendererConfig)
manager.use(LoggerRenderer)
manager.use(PiniaMobxUtilsRenderer)
manager.use(SettingUtilsRenderer)
manager.use(SetupInAppScopeRenderer)
manager.use(SgpRenderer)
manager.use(WindowManagerRenderer)

export { manager }
