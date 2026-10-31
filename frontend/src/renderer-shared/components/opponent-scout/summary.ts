import { toBasicInfo } from '@shared/data-adapter/match-history/match-basic'
import { toParticipants } from '@shared/data-adapter/match-history/participants'
import type { LcuOrSgpGameSummary } from '@shared/data-adapter/wrapper'

import type { SignalMatch } from '../ongoing-game-panel/widgets/player-info-card/account-signals/analyze'

export interface ScoutMatch extends SignalMatch {
  teammates: string[]
  completeRoster: boolean
}

export function normalizePosition(position: string | null | undefined): string | null {
  const value = position?.toUpperCase()
  if (value === 'MID') return 'MIDDLE'
  if (value === 'SUPPORT') return 'UTILITY'
  if (value === 'BOT' || value === 'ADC') return 'BOTTOM'
  return value && ['TOP', 'JUNGLE', 'MIDDLE', 'BOTTOM', 'UTILITY'].includes(value) ? value : null
}

export function lcuPosition(lane: string | undefined, role: string | undefined) {
  if (lane === 'BOTTOM' || lane === 'BOT') {
    return role === 'DUO_SUPPORT' ? 'UTILITY' : role === 'DUO_CARRY' ? 'BOTTOM' : null
  }
  return normalizePosition(lane)
}

// Summaries may contain only the queried player. Combine already-loaded records
// for this game; absence is evidence only when the full ten-player roster is known.
export function readScoutMatch(
  game: LcuOrSgpGameSummary,
  puuid: string,
  related: LcuOrSgpGameSummary[] = []
): ScoutMatch | null {
  const basic = toBasicInfo(game)
  const players = toParticipants(game, basic)
  const player = players.find((participant) => participant.puuid === puuid)
  if (!player || !['win', 'loss'].includes(player.winResult)) return null
  const roster = new Map(players.filter((p) => p.puuid).map((p) => [p.puuid, p]))
  for (const other of related) {
    if (other === game || other.gameId !== game.gameId) continue
    try {
      for (const participant of toParticipants(other, toBasicInfo(other))) {
        if (participant.puuid) roster.set(participant.puuid, participant)
      }
    } catch {
      /* A partial related record cannot invalidate the player's summary. */
    }
  }
  const teammates = [...roster.values()].filter((p) => p.teamIdentifier === player.teamIdentifier)
  const teamSizes = new Map<string, number>()
  for (const participant of roster.values()) {
    teamSizes.set(participant.teamIdentifier, (teamSizes.get(participant.teamIdentifier) ?? 0) + 1)
  }
  const raw =
    game.source === 'lcu'
      ? game.data.participants.find((p) => p.participantId === player.participantId)
      : undefined
  return {
    id: basic.gameId,
    at: basic.gameCreation,
    queue: basic.queueId,
    duration: basic.gameDuration,
    won: player.win,
    kills: player.kills,
    deaths: player.deaths,
    assists: player.assists,
    champion: player.championId,
    position:
      normalizePosition(player.position) ?? lcuPosition(raw?.timeline?.lane, raw?.timeline?.role),
    teammates: teammates.filter((p) => p.puuid !== puuid).map((p) => p.puuid),
    completeRoster:
      basic.isTwoTeam &&
      roster.size === 10 &&
      teamSizes.size === 2 &&
      [...teamSizes.values()].every((size) => size === 5)
  }
}

export function opponentIds(teams: Record<string, string[]>, self: string | undefined) {
  if (!self) return []
  const own = Object.values(teams).find((team) => team.includes(self))
  if (!own) return []
  return [
    ...new Set(
      Object.values(teams)
        .flat()
        .filter((id) => !own.includes(id))
    )
  ]
}

function recentMatches<T extends SignalMatch>(matches: T[], queue: number): T[] {
  const seen = new Set<number>()
  return matches
    .filter((match) => match.queue === queue && match.duration >= 300)
    .sort((a, b) => b.at - a.at)
    .filter((match) => {
      if (seen.has(match.id)) return false
      seen.add(match.id)
      return true
    })
    .slice(0, 20)
}

export function scoutInsights(
  matches: ScoutMatch[],
  current: string | null,
  queue: number,
  candidates: string[]
) {
  const recent = recentMatches(matches, queue)
  const positions = new Map<string, number>()
  for (const match of recent) {
    const position = normalizePosition(match.position)
    if (position) positions.set(position, (positions.get(position) ?? 0) + 1)
  }
  const known = [...positions.values()].reduce((a, b) => a + b, 0)
  const ordered = [...positions].sort((a, b) => b[1] - a[1])
  const main = ordered.length && ordered[0][1] !== ordered[1]?.[1] ? ordered[0] : null
  const roleGames = recent.filter(
    (match) => current && normalizePosition(match.position) === current
  )
  const partners = [...new Set(candidates)]
    .map((id) => {
      const together = recent.filter((match) => match.teammates.includes(id))
      const apart = recent.filter((match) => match.completeRoster && !match.teammates.includes(id))
      const wins = together.filter((match) => match.won).length
      const apartWins = apart.filter((match) => match.won).length
      return {
        id,
        count: together.length,
        wins,
        losses: together.length - wins,
        apartCount: apart.length,
        apartWins,
        apartLosses: apart.length - apartWins,
        unknown: recent.length - together.length - apart.length
      }
    })
    .filter((partner) => partner.count >= 2)
    .sort((a, b) => b.count - a.count || a.id.localeCompare(b.id))
    .slice(0, 2)
  return {
    known,
    main: main?.[0] ?? null,
    share: main ? Math.round((main[1] / known) * 100) : null,
    offRole: !!current && !!main && main[0] !== current && known >= 8 && main[1] / known >= 0.6,
    roleCount: roleGames.length,
    roleWins: roleGames.filter((match) => match.won).length,
    roleLosses: roleGames.filter((match) => !match.won).length,
    partners
  }
}

export function summarize(matches: SignalMatch[], champion: number, queue: number) {
  const recent = recentMatches(matches, queue)
  const wins = recent.filter((match) => match.won).length
  const championGames = recent.filter((match) => match.champion === champion)
  const kills = recent.reduce((sum, match) => sum + match.kills + match.assists, 0)
  const deaths = recent.reduce((sum, match) => sum + match.deaths, 0)
  return {
    count: recent.length,
    wins,
    losses: recent.length - wins,
    kda: recent.length ? (kills / Math.max(1, deaths)).toFixed(1) : null,
    championCount: championGames.length,
    championWins: championGames.filter((match) => match.won).length
  }
}
