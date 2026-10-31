import type { SignalMatch } from '../ongoing-game-panel/widgets/player-info-card/account-signals/analyze'

const ARAM_QUEUE = 450
const DAY = 86400000
const MIN_DURATION = 600
const MAX_DURATION = 3600
const MAX_SAMPLE = 40

export interface AramSample {
  count: number
  wins: number
  winRate: number
  kda: number
}

export interface AramPlayerInput {
  id: string
  games: number
  winRate: number
  kda: number
  level?: number
  stackSize: number
}

export interface AramTeamSummary {
  team: string
  total: number
  sampled: number
  games: number
  winRate: number | null
  kda: number | null
  avgLevel: number | null
  stack: number
}

export type AramVerdict =
  'unknown' | 'insufficient' | 'even' | 'higher' | 'muchHigher' | 'lower' | 'muchLower'

const median = (values: number[]) => {
  if (!values.length) return 0
  const sorted = [...values].sort((a, b) => a - b)
  const mid = Math.floor(sorted.length / 2)
  return sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2
}

const kda = (match: SignalMatch) => (match.kills + match.assists) / Math.max(1, match.deaths)

/** ARAM has no visible LP; this sample only describes recent ARAM performance. */
export function aramSample(matches: SignalMatch[], now = Date.now()): AramSample {
  const unique = new Map<number, SignalMatch>()
  for (const match of matches) {
    if (
      match.queue !== ARAM_QUEUE ||
      match.duration < MIN_DURATION ||
      match.duration > MAX_DURATION ||
      match.at > now ||
      now - match.at > 30 * DAY
    )
      continue
    if (
      ![match.id, match.at, match.duration, match.kills, match.deaths, match.assists].every(
        Number.isFinite
      ) ||
      match.id <= 0 ||
      [match.kills, match.deaths, match.assists].some((value) => value < 0)
    )
      continue
    unique.set(match.id, match)
  }

  const sample = [...unique.values()].sort((a, b) => b.at - a.at).slice(0, MAX_SAMPLE)
  const wins = sample.filter((match) => match.won).length
  return {
    count: sample.length,
    wins,
    winRate: sample.length ? wins / sample.length : 0,
    kda: median(sample.map(kda))
  }
}

export function aggregateAramTeam(team: string, players: AramPlayerInput[]): AramTeamSummary {
  const usable = players.filter(
    (player) => player.games >= 8 && Number.isFinite(player.winRate) && Number.isFinite(player.kda)
  )
  const levels = players
    .map((player) => player.level)
    .filter((level): level is number => typeof level === 'number' && Number.isFinite(level))
  return {
    team,
    total: players.length,
    sampled: usable.length,
    games: usable.reduce((sum, player) => sum + player.games, 0),
    winRate: usable.length ? median(usable.map((player) => player.winRate)) : null,
    kda: usable.length ? median(usable.map((player) => player.kda)) : null,
    avgLevel: levels.length ? levels.reduce((sum, level) => sum + level, 0) / levels.length : null,
    stack: players.length ? Math.max(...players.map((player) => player.stackSize)) : 1
  }
}

export function aramVerdict(our: AramTeamSummary, enemy: AramTeamSummary): AramVerdict {
  if (our.winRate == null || enemy.winRate == null) return 'unknown'
  if (our.sampled < 3 || enemy.sampled < 3 || our.games < 24 || enemy.games < 24)
    return 'insufficient'

  const winRateDiff = our.winRate - enemy.winRate
  const stackDiff = our.stack - enemy.stack
  if (winRateDiff >= 0.15 || stackDiff >= 3) return 'muchHigher'
  if (winRateDiff >= 0.07 || stackDiff >= 2) return 'higher'
  if (winRateDiff <= -0.15 || stackDiff <= -3) return 'muchLower'
  if (winRateDiff <= -0.07 || stackDiff <= -2) return 'lower'
  return 'even'
}
