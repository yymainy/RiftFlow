export const DEFAULT_RIFTFLOW_API_BASE_URL = 'https://riftflow-api.yymainy.online'
export const DEFAULT_RIFTFLOW_STATIC_BASE_URL = 'https://riftflow-api.yymainy.online/static'

export interface RiftFlowServiceBaseUrls {
  api: string
  static: string
}

export const DEFAULT_RIFTFLOW_SERVICE_BASE_URLS: Readonly<RiftFlowServiceBaseUrls> = Object.freeze({
  api: DEFAULT_RIFTFLOW_API_BASE_URL,
  static: DEFAULT_RIFTFLOW_STATIC_BASE_URL
})

export interface RiftFlowApiBootstrapDocument {
  schemaVersion: 1
  generation: number
  baseUrls: RiftFlowServiceBaseUrls
}

export type RiftFlowApiLanguage = 'zh-CN' | 'en'
export const DEFAULT_RIFTFLOW_API_LANGUAGE: RiftFlowApiLanguage = 'zh-CN'

export type RiftFlowNoticeSeverity = 'low' | 'medium' | 'high'

export interface RiftFlowNotice {
  revision: string
  language: RiftFlowApiLanguage
  severity: RiftFlowNoticeSeverity
  summary: string
  contentType: 'text/markdown'
  content: string
  updatedAt: string
}

export type RiftFlowContactChannelPlatform =
  'qq' | 'telegram' | 'discord' | 'email' | 'website' | 'other'

export interface RiftFlowContactChannel {
  id: string
  platform: RiftFlowContactChannelPlatform
  name: string
  identifier: string
  url: string
  password: string | null
}

export interface RiftFlowContactChannels {
  channels: RiftFlowContactChannel[]
  updatedAt: string
}

export interface RiftFlowReleaseArtifact {
  platform: string
  arch: string
  fileName: string
  size: number
  contentType: string
  sha256: string | null
  downloadUrl: string
}

export interface RiftFlowRelease {
  version: string
  publishedAt: string
  description: string
  artifacts: RiftFlowReleaseArtifact[]
}

export interface RiftFlowApiErrorBody {
  error: {
    code: string
    message: string
  }
}

export interface RiftFlowStatisticsRecordCreated {
  id: number
}

export interface RiftFlowConfigMetadata {
  updatedAt: string
}

export type RiftFlowFeatureGatePlatform = 'win32' | 'darwin'

export interface RiftFlowFeatureGateRuntimeRule {
  platforms?: RiftFlowFeatureGatePlatform[]
  minVersionInclusive?: string
  maxVersionExclusive?: string
  sgpServers?: string[]
}

export interface RiftFlowFeatureGateSnapshot extends RiftFlowConfigMetadata {
  gates: Record<string, RiftFlowFeatureGateRuntimeRule>
}

export interface RiftFlowAutoSelectTargetGameMode {
  gameMode: string
  queueTypes: string[]
}

export interface RiftFlowAutoSelectGroup {
  groupId: string
  name: {
    'zh-CN': string
    en: string
  }
  iconPath: string
  isCustom: boolean
  supportedSgpServers: string[]
  targetGameModes: RiftFlowAutoSelectTargetGameMode[]
  positions: string[]
  additionalPicks: number[]
  additionalBans: number[]
  excludedPicks: number[]
  excludedBans: number[]
}

export interface RiftFlowAutoSelectGroupsConfig extends RiftFlowConfigMetadata {
  groups: RiftFlowAutoSelectGroup[]
}

export interface RiftFlowLeagueServerEndpoint {
  matchHistory: string
  common: string
  isTencent: boolean
  regionPathParam?: string
}

export interface RiftFlowLeagueServersConfig extends RiftFlowConfigMetadata {
  servers: Record<string, RiftFlowLeagueServerEndpoint>
  serverNames: Record<string, Record<string, string>>
}

export interface RiftFlowSupportedQueuesConfig extends RiftFlowConfigMetadata {
  queues: number[]
}

export interface RiftFlowApiConfigResourceMap {
  'app/feature-gates': RiftFlowFeatureGateSnapshot
  'auto-select/groups': RiftFlowAutoSelectGroupsConfig
  'sgp/league-servers': RiftFlowLeagueServersConfig
  'sgp/supported-queues': RiftFlowSupportedQueuesConfig
}

export type RiftFlowApiConfigResource = keyof RiftFlowApiConfigResourceMap
