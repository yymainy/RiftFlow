export interface SignalMatch {
  id: number
  at: number
  queue: number
  duration: number
  won: boolean
  kills: number
  deaths: number
  assists: number
  champion: number
  position: string | null
}

export interface AccountSignals {
  status: 'insufficient' | 'none' | 'smurf' | 'change'
  strength: 'limited' | 'moderate' | 'strong'
  count: number
  excluded: number
  queue: number | null
  evidence: { key: string; values: Record<string, number> }[]
  gameIds: number[]
}

const median = (values: number[]) => {
  const sorted = [...values].sort((a, b) => a - b)
  const mid = Math.floor(sorted.length / 2)
  return sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2
}
const kda = (m: SignalMatch) => (m.kills + m.assists) / Math.max(1, m.deaths)
const winRate = (matches: SignalMatch[]) => matches.filter((m) => m.won).length / matches.length
const role = (matches: SignalMatch[]) => {
  const counts = new Map<string, number>()
  for (const m of matches)
    if (m.position && !['UNKNOWN', 'NONE', 'INVALID'].includes(m.position.toUpperCase()))
      counts.set(m.position, (counts.get(m.position) || 0) + 1)
  const best = [...counts].sort((a, b) => b[1] - a[1])[0]
  return best && best[1] / matches.length >= 0.7 ? best[0] : null
}

/** Descriptive, conservative heuristics. Strength is rule support, not identity probability. */
export function analyzeAccount(
  matches: SignalMatch[],
  level: number | undefined,
  now: number,
  preferredQueue?: number
): AccountSignals {
  const unique = new Map<number, SignalMatch>()
  for (const m of matches) {
    if (typeof m.won !== 'boolean') continue
    if (
      ![420, 440].includes(m.queue) ||
      m.duration < 900 ||
      m.duration > 7200 ||
      m.at > now ||
      now - m.at > 30 * 86400000
    )
      continue
    if (
      ![m.id, m.at, m.duration, m.kills, m.deaths, m.assists, m.champion].every(Number.isFinite) ||
      m.id <= 0 ||
      m.champion <= 0 ||
      [m.kills, m.deaths, m.assists].some((v) => v < 0)
    )
      continue
    unique.set(m.id, m)
  }
  const valid = [...unique.values()]
  const queue = [420, 440].includes(preferredQueue || 0)
    ? preferredQueue!
    : [420, 440].sort(
        (a, b) =>
          valid.filter((m) => m.queue === b).length - valid.filter((m) => m.queue === a).length
      )[0]
  const sample = valid
    .filter((m) => m.queue === queue)
    .sort((a, b) => b.at - a.at)
    .slice(0, 40)
  const result: AccountSignals = {
    status: 'insufficient',
    strength: 'limited',
    count: sample.length,
    excluded: matches.length - sample.length,
    queue,
    evidence: [],
    gameIds: sample.map((m) => m.id)
  }
  if (sample.length < 12) return result
  result.status = 'none'
  const wr = winRate(sample)
  const typicalKda = median(sample.map(kda))
  const sustained = sample.filter((m) => kda(m) >= 4).length / sample.length >= 0.6
  if (Number.isFinite(level) && level! >= 30 && level! <= 80)
    result.evidence.push({ key: 'lowLevel', values: { level: level! } })
  if (wr >= 0.75)
    result.evidence.push({
      key: 'winRate',
      values: { count: sample.length, rate: Math.round(wr * 100) }
    })
  if (typicalKda >= 4 && sustained)
    result.evidence.push({ key: 'kda', values: { value: Number(typicalKda.toFixed(1)) } })
  if (result.evidence.length === 3) {
    result.status = 'smurf'
    result.strength = sample.length >= 20 ? 'strong' : 'moderate'
  }
  if (sample.length >= 20) {
    const recent = sample.slice(0, 10),
      previous = sample.slice(10, 20)
    const common = new Set(
      recent.map((m) => m.champion).filter((id) => previous.some((m) => m.champion === id))
    )
    const comparable =
      role(recent) &&
      role(recent) === role(previous) &&
      [recent, previous].every((group) => group.filter((m) => common.has(m.champion)).length >= 6)
    const oldKda = median(previous.map(kda)),
      newKda = median(recent.map(kda))
    if (
      comparable &&
      winRate(recent) - winRate(previous) >= 0.4 &&
      newKda >= oldKda * 2 &&
      newKda - oldKda >= 1.5
    ) {
      result.status = 'change'
      result.strength = 'moderate'
      result.evidence.push({
        key: 'shift',
        values: {
          before: Math.round(winRate(previous) * 100),
          after: Math.round(winRate(recent) * 100),
          oldKda: Number(oldKda.toFixed(1)),
          newKda: Number(newKda.toFixed(1))
        }
      })
    }
  }
  return result
}
