import { describe, expect, it } from 'vitest'

import type { ArchivedMatch } from '../archive/archive-api'
import {
  type AiMatch,
  archiveMatches,
  deduplicateMatches,
  matchStats,
  weeklySamples
} from './context'

const sample = (id: string, patch: Partial<AiMatch> = {}): AiMatch => ({
  id,
  at: 1000,
  queue: 420,
  mode: 'CLASSIC',
  champion: '阿狸',
  won: true,
  kills: 5,
  deaths: 3,
  assists: 7,
  duration: 1800,
  ...patch
})
describe('AI context is scoped to facts and visible samples', () => {
  it('separates accounts and strips identity fields before transmission', () => {
    const result = archiveMatches(
      [
        {
          owner: 'a',
          playerName: 'private',
          region: 'CN1',
          gameId: '1',
          createdAt: 1000,
          championName: '阿狸',
          championId: 103,
          queueId: 420,
          gameMode: 'CLASSIC',
          win: true,
          kills: 5,
          deaths: 3,
          assists: 7,
          duration: 1800
        },
        { owner: 'b', gameId: '2' }
      ] as ArchivedMatch[],
      'a'
    )
    expect(result).toHaveLength(1)
    expect(JSON.stringify(result)).not.toContain('private')
    expect(result[0]).not.toHaveProperty('owner')
    expect(result[0]).not.toHaveProperty('region')
  })
  it('deduplicates shared archive/live records before counting outcomes and separates queues', () => {
    const records = deduplicateMatches([
      sample('1'),
      sample('1'),
      sample('2', { won: false }),
      sample('3', { queue: 450 }),
      sample('4', { won: null }),
      sample('5', { duration: 100 })
    ])
    expect(matchStats(records)).toMatchObject({
      games: 3,
      wins: 2,
      winRate: 67,
      queues: [
        { queue: 420, games: 2, wins: 1, winRate: 50 },
        { queue: 450, games: 1, wins: 1, winRate: 100 }
      ]
    })
    expect(matchStats([]).winRate).toBeNull()
  })
  it('assigns the seven-day boundary to one period and excludes future/outdated games', () => {
    const now = Date.UTC(2026, 9, 31),
      week = 7 * 86400000
    const result = weeklySamples(
      [
        sample('a', { at: now }),
        sample('b', { at: now - week }),
        sample('c', { at: now - week - 1, won: false }),
        sample('d', { at: now - 2 * week - 1 }),
        sample('e', { at: now + 1 })
      ],
      now
    )
    expect(result.current.games).toBe(2)
    expect(result.previous.games).toBe(1)
    expect(result.previous.wins).toBe(0)
  })
})
