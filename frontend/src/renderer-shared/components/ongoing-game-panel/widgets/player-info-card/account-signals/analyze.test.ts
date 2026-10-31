import { describe, expect, it } from 'vitest'

import { type SignalMatch, analyzeAccount } from './analyze'

const now = Date.UTC(2026, 9, 29)
function games(n: number): SignalMatch[] {
  return Array.from({ length: n }, (_, i) => ({
    id: i + 1,
    at: now - (i + 1) * 3600000,
    queue: 420,
    duration: 1800,
    won: i % 5 !== 0,
    kills: 10,
    deaths: 2,
    assists: 6,
    champion: 1,
    position: 'MIDDLE'
  }))
}
describe('account signals', () => {
  it('requires sustained combined evidence rather than a streak or low level alone', () => {
    expect(analyzeAccount(games(11), 40, now).status).toBe('insufficient')
    expect(analyzeAccount(games(20), 40, now).status).toBe('smurf')
    expect(analyzeAccount(games(20), 400, now).status).toBe('none')
    expect(
      analyzeAccount(
        games(20).map((m) => ({ ...m, kills: 2, assists: 2, deaths: 8 })),
        40,
        now
      ).status
    ).toBe('none')
  })
  it('does not inflate samples with duplicates, other queues, old or invalid games', () => {
    const data = games(10)
    const invalid = games(10).map((m, i) => ({ ...m, id: 100 + i, queue: 450 }))
    expect(analyzeAccount([...data, ...data, ...invalid], 40, now).count).toBe(10)
    expect(
      analyzeAccount(
        games(20).map((m) => ({ ...m, at: now - 31 * 86400000 })),
        40,
        now
      ).count
    ).toBe(0)
    expect(
      analyzeAccount(
        games(20).map((m) => ({ ...m, kills: NaN })),
        40,
        now
      ).count
    ).toBe(0)
    expect(
      analyzeAccount(
        [...games(10), ...games(10).map((m) => ({ ...m, id: m.id + 100, queue: 440 }))],
        40,
        now
      ).status
    ).toBe('insufficient')
  })
  it('only flags a shift with comparable roles and champions', () => {
    const data = games(20).map((m, i) => ({
      ...m,
      won: i < 10,
      kills: i < 10 ? 10 : 2,
      assists: 2,
      deaths: i < 10 ? 2 : 5
    }))
    expect(analyzeAccount(data, 200, now).status).toBe('change')
    expect(
      analyzeAccount(
        data.map((m, i) => ({ ...m, position: i < 10 ? 'MIDDLE' : 'JUNGLE' })),
        200,
        now
      ).status
    ).toBe('none')
    expect(
      analyzeAccount(
        data.map((m, i) => ({ ...m, champion: i < 10 ? 1 : 2 })),
        200,
        now
      ).status
    ).toBe('none')
  })
  it('keeps the current ranked queue separate and missing levels inconclusive for alternate accounts', () => {
    expect(analyzeAccount(games(20), undefined, now).status).toBe('none')
    expect(analyzeAccount(games(20), 40, now, 440).status).toBe('insufficient')
    expect(
      analyzeAccount(
        games(20).map((m) => ({ ...m, position: null })),
        200,
        now
      ).status
    ).toBe('none')
  })
  it('uses ARAM-specific samples and requires a high win rate plus another clue', () => {
    const aram = (n: number) =>
      games(n).map((m) => ({ ...m, queue: 450, duration: 1200, position: null }))

    const high = analyzeAccount(aram(20), 40, now, 450, 'aram')
    expect(high.status).toBe('smurf')
    expect(high.strength).toBe('strong')
    expect(high.evidence.map((item) => item.key)).toEqual(['lowLevel', 'aramWinRate', 'aramKda'])

    expect(analyzeAccount(aram(20), 400, now, 450, 'aram').status).toBe('watch')

    expect(
      analyzeAccount(
        aram(20).map((m) => ({ ...m, kills: 1, assists: 1, deaths: 10 })),
        400,
        now,
        450,
        'aram'
      ).status
    ).toBe('none')
    expect(
      analyzeAccount(
        aram(20).map((m, i) => ({ ...m, won: i % 2 === 0 })),
        400,
        now,
        450,
        'aram'
      ).status
    ).toBe('none')
  })
  it('does not mix ARAM samples with ranked games, short games or old games', () => {
    const aram = (n: number) =>
      games(n).map((m) => ({ ...m, queue: 450, duration: 1200, position: null }))
    const mixed = [...aram(10), ...aram(10).map((m) => ({ ...m, id: m.id + 100, queue: 420 }))]
    expect(analyzeAccount(mixed, 400, now, 450, 'aram').count).toBe(10)
    expect(
      analyzeAccount(
        aram(20).map((m) => ({ ...m, duration: 300 })),
        400,
        now,
        450,
        'aram'
      ).count
    ).toBe(0)
    expect(
      analyzeAccount(
        aram(20).map((m) => ({ ...m, at: now - 31 * 86400000 })),
        400,
        now,
        450,
        'aram'
      ).count
    ).toBe(0)
  })
})
