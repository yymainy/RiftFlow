import type { SharedGlobalShard } from '@shared/riftflow-shard'
import type { RiftFlowFeatureGateSnapshot } from '@shared/shards/riftflow-api'
import { reaction } from 'mobx'
import { describe, expect, it, vi } from 'vitest'

import { FeatureGatingMain } from '.'
import type { RiftFlowApiMain } from '../riftflow-api'
import { RiftFlowApiState } from '../riftflow-api/state'
import type { LeagueClientMain } from '../league-client'

vi.mock('../riftflow-api', () => ({ RiftFlowApiMain: class RiftFlowApiMain {} }))
vi.mock('../league-client', () => ({ LeagueClientMain: class LeagueClientMain {} }))

const snapshot = (gates: RiftFlowFeatureGateSnapshot['gates']): RiftFlowFeatureGateSnapshot => ({
  updatedAt: '2026-07-25T04:00:00.000Z',
  gates
})

describe('FeatureGatingMain', () => {
  it('can be observed from a MobX reaction', () => {
    const state = new RiftFlowApiState()
    const featureGating = new FeatureGatingMain(
      {
        global: {
          platform: 'win32',
          version: '1.5.0'
        }
      } as unknown as SharedGlobalShard,
      { state } as unknown as RiftFlowApiMain,
      { state: { auth: null } } as unknown as LeagueClientMain
    )
    const values: boolean[] = []
    const dispose = reaction(
      () => featureGating.isEnabled('ongoing-game.deobfuscation', true),
      (enabled) => values.push(enabled),
      { fireImmediately: true }
    )

    state.setFeatureGates(snapshot({}))
    state.setFeatureGates(snapshot({ 'ongoing-game.deobfuscation': {} }))

    expect(values).toEqual([true, false, true])
    dispose()
  })
})
