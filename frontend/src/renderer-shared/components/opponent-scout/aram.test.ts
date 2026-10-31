import { describe, expect, it } from 'vitest'

import type { SignalMatch } from '../ongoing-game-panel/widgets/player-info-card/account-signals/analyze'
import { type AramTeamSummary, aggregateAramTeam, aramSample, aramVerdict } from './aram'

const now = Date.UTC(2026, 9, 29)

function match(id: number, overrides: Partial<SignalMatch> = {}): SignalMatch {
  return {
    id,
    at: now - id * 3600000,
    queue: 450,
    duration: 1200,
    won: true,
    kills: 10,
    deaths: 2,
    assists: 6,
    champion: 1,
    position: null,
    ...overrides
  }
}

describe('ARAM sample', () => {
  it('keeps only recent valid ARAM games and drops duplicates', () => {
    const sample = aramSample(
      [
        match(1),
        match(2, { won: false }),
        match(3),
        match(3),
        match(4, { queue: 420 }),
        match(5, { duration: 300 }),
        match(6, { at: now - 31 * 86400000 })
      ],
      now
    )
    expect(sample.count).toBe(3)
    expect(sample.wins).toBe(2)
    expect(sample.winRate).toBeCloseTo(2 / 3)
    expect(sample.kda).toBe(8)
  })
})

describe('ARAM team aggregation', () => {
  it('uses the median of sampled players and keeps the largest stack', () => {
    const summary = aggregateAramTeam('100', [
      { id: 'a', games: 20, winRate: 0.6, kda: 5, level: 100, stackSize: 1 },
      { id: 'b', games: 20, winRate: 0.5, kda: 4, level: 200, stackSize: 3 },
      { id: 'c', games: 4, winRate: 0.9, kda: 9, level: 50, stackSize: 1 }
    ])
    expect(summary.sampled).toBe(2)
    expect(summary.games).toBe(40)
    expect(summary.winRate).toBeCloseTo(0.55)
    expect(summary.kda).toBeCloseTo(4.5)
    expect(summary.avgLevel).toBe(350 / 3)
    expect(summary.stack).toBe(3)
  })

  it('reports insufficient data instead of guessing', () => {
    const our = aggregateAramTeam('100', [
      { id: 'a', games: 20, winRate: 0.6, kda: 5, level: 100, stackSize: 1 }
    ])
    const enemy = aggregateAramTeam('200', [
      { id: 'b', games: 20, winRate: 0.5, kda: 4, level: 100, stackSize: 1 }
    ])
    expect(aramVerdict(our, enemy)).toBe('insufficient')
  })
})

describe('ARAM strength verdict', () => {
  const summary = (overrides: Partial<AramTeamSummary>): AramTeamSummary => ({
    team: '100',
    total: 5,
    sampled: 5,
    games: 100,
    winRate: 0.5,
    kda: 4,
    avgLevel: 200,
    stack: 1,
    ...overrides
  })

  it('classifies by win rate and stack difference without a hidden score', () => {
    expect(aramVerdict(summary({ winRate: 0.6 }), summary({ team: '200' }))).toBe('higher')
    expect(aramVerdict(summary({ winRate: 0.68 }), summary({ team: '200' }))).toBe('muchHigher')
    expect(aramVerdict(summary({ winRate: 0.48 }), summary({ team: '200' }))).toBe('even')
    expect(aramVerdict(summary({ winRate: 0.32 }), summary({ team: '200' }))).toBe('muchLower')
    expect(aramVerdict(summary({ stack: 3 }), summary({ team: '200' }))).toBe('higher')
  })
})
