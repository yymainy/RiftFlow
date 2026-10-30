import { useInstance } from '@renderer-shared/shards'
import { RiftFlowIpcRenderer } from '@renderer-shared/shards/ipc'
import type { Game } from '@shared/types/league-client/match-history'

export interface ArchivedMatch {
  key: string
  owner: string
  region: string
  gameId: string
  playerName: string
  championId: number
  championName: string
  createdAt: number
  gameMode: string
  queueId: number
  duration: number
  version: string
  kills: number
  deaths: number
  assists: number
  win: boolean | null
  gold: number
  damage: number
  cs: number
  source: string
  savedAt: number
  complete: boolean
  game?: Game
}

export interface ArchiveList {
  records: ArchivedMatch[]
  total: number
  matched: number
  modes: string[]
}

export interface Diagnostics {
  connection: string
  detectedClients: number
  connected: boolean
  loggedIn: boolean
  playerName: string
  region: string
  phase: string
  elevated: boolean
  sgpConfigured: boolean
  busy: boolean
  paused: boolean
  archiveCount: number
  lastSync: {
    at?: number
    success?: boolean
    saved?: number
    incomplete?: number
    reason?: string
    failures?: { gameId: string; reason: string }[]
  }
}

export function useArchiveApi() {
  const ipc = useInstance(RiftFlowIpcRenderer)
  return {
    list: (filter: Record<string, unknown>) =>
      ipc.call<ArchiveList>('riftflow-archive-main', 'list', filter),
    get: (key: string) => ipc.call<ArchivedMatch>('riftflow-archive-main', 'get', key),
    diagnostics: () => ipc.call<Diagnostics>('riftflow-archive-main', 'diagnostics'),
    sync: () =>
      ipc.call<{
        success: boolean
        saved: number
        skipped: number
        incomplete: number
        failures: { gameId: string; reason: string }[]
      }>('riftflow-archive-main', 'sync'),
    export: (format: 'json' | 'csv') =>
      ipc.call<{ cancelled: boolean; path?: string }>('riftflow-archive-main', 'export', format),
    restore: () =>
      ipc.call<{ cancelled: boolean; count?: number }>('riftflow-archive-main', 'restore')
  }
}
