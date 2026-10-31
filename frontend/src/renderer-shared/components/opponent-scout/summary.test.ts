import type { LcuOrSgpGameSummary } from '@shared/data-adapter/wrapper'
import { describe, expect, it } from 'vitest'

import type { SignalMatch } from '../ongoing-game-panel/widgets/player-info-card/account-signals/analyze'
import {
  type ScoutMatch,
  lcuPosition,
  opponentIds,
  readScoutMatch,
  scoutInsights,
  summarize
} from './summary'

const match = (id: number, extra: Partial<SignalMatch> = {}): SignalMatch => ({
  id,
  at: id * 1000,
  queue: 420,
  duration: 1800,
  won: true,
  kills: 4,
  deaths: 2,
  assists: 6,
  champion: 1,
  position: 'MIDDLE',
  ...extra
})

const scout = (id: number, extra: Partial<ScoutMatch> = {}): ScoutMatch => ({
  ...match(id),
  teammates: [],
  completeRoster: true,
  ...extra
})

describe('position and recurring teammate insights', () => {
  it('shows a supported off-role signal and the actual current-role record', () => {
    const games = Array.from({ length: 20 }, (_, i) =>
      scout(i + 1, {
        position: i < 16 ? 'UTILITY' : i < 19 ? 'JUNGLE' : null,
        won: i === 16
      })
    )
    expect(scoutInsights(games, 'JUNGLE', 420, [])).toMatchObject({
      known: 19,
      main: 'UTILITY',
      share: 84,
      offRole: true,
      roleCount: 3,
      roleWins: 1,
      roleLosses: 2
    })
  })
  it('does not guess a main role from missing, tied or small samples', () => {
    expect(scoutInsights([scout(1, { position: null })], 'JUNGLE', 420, []).main).toBeNull()
    expect(scoutInsights([scout(1)], 'JUNGLE', 420, []).offRole).toBe(false)
    const tied = [scout(1, { position: 'TOP' }), scout(2, { position: 'MIDDLE' })]
    expect(scoutInsights(tied, 'JUNGLE', 420, []).main).toBeNull()
    expect(
      scoutInsights(
        Array.from({ length: 10 }, (_, i) => scout(i)),
        null,
        420,
        []
      ).offRole
    ).toBe(false)
    expect(lcuPosition('BOTTOM', 'DUO_SUPPORT')).toBe('UTILITY')
    expect(lcuPosition('BOTTOM', 'DUO_CARRY')).toBe('BOTTOM')
    expect(lcuPosition('BOTTOM', 'NONE')).toBeNull()
    expect(lcuPosition('NONE', 'NONE')).toBeNull()
  })
  it('separates known together, verified apart and incomplete records without counting unknown as solo', () => {
    const games = [
      scout(1, { teammates: ['partner'], won: true }),
      scout(2, { teammates: ['partner'], completeRoster: false, won: false }),
      scout(3, { won: false }),
      scout(4, { completeRoster: false })
    ]
    expect(
      scoutInsights(
        [...games, games[0], scout(99, { queue: 450, teammates: ['partner'] })],
        'MIDDLE',
        420,
        ['partner', 'one-off']
      ).partners
    ).toEqual([
      {
        id: 'partner',
        count: 2,
        wins: 1,
        losses: 1,
        apartCount: 1,
        apartWins: 0,
        apartLosses: 1,
        unknown: 1
      }
    ])
  })
  it('does not treat a historical enemy as a teammate and can merge partial LCU roster records', () => {
    const record = (puuid: string, participantId: number, teamId: number): LcuOrSgpGameSummary =>
      ({
        gameId: 1,
        source: 'lcu',
        data: {
          gameId: 1,
          gameMode: 'CLASSIC',
          queueId: 420,
          gameDuration: 1800,
          gameCreation: 1000,
          participants: [
            {
              participantId,
              teamId,
              championId: 64,
              timeline: { lane: 'JUNGLE', role: 'NONE' },
              stats: { kills: 3, deaths: 1, assists: 5, win: true }
            }
          ],
          participantIdentities: [{ participantId, player: { puuid } }]
        }
      }) as LcuOrSgpGameSummary
    const own = record('player', 1, 100)
    const related = [record('partner', 2, 100), record('historical-enemy', 6, 200)]
    expect(readScoutMatch(own, 'player', related)).toMatchObject({
      position: 'JUNGLE',
      teammates: ['partner'],
      completeRoster: false
    })
    const full = Array.from({ length: 10 }, (_, i) =>
      record(i === 0 ? 'player' : `p${i}`, i + 1, i < 5 ? 100 : 200)
    )
    expect(readScoutMatch(own, 'player', full)?.completeRoster).toBe(true)
    expect(readScoutMatch(own, 'absent')).toBeNull()
  })
})

describe('opponent scout', () => {
  it('never treats teammates or spectator rosters as opponents', () => {
    const teams = { blue: ['self', 'ally'], red: ['enemy', 'other', 'enemy'] }
    expect(opponentIds(teams, 'self')).toEqual(['enemy', 'other'])
    expect(opponentIds(teams, 'spectator')).toEqual([])
    expect(opponentIds(teams, undefined)).toEqual([])
  })
  it('uses the latest 20 unique completed games from the current queue', () => {
    const games = Array.from({ length: 22 }, (_, i) => match(i + 1))
    const result = summarize(
      [
        ...games.reverse(),
        match(22),
        match(100, { queue: 450 }),
        match(101, { duration: 120 }),
        match(23, { won: false, champion: 2 })
      ],
      1,
      420
    )
    expect(result).toEqual({
      count: 20,
      wins: 19,
      losses: 1,
      kda: '5.0',
      championCount: 19,
      championWins: 19
    })
  })
  it('does not turn absent data into a zero percent win rate or infinite KDA', () => {
    expect(summarize([], 1, 420)).toMatchObject({ count: 0, kda: null })
    expect(summarize([match(1, { deaths: 0 })], 1, 420).kda).toBe('10.0')
    expect(summarize([match(1)], 2, 420)).toMatchObject({ championCount: 0, championWins: 0 })
  })
})
