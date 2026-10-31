import { readScoutMatch } from '@renderer-shared/components/opponent-scout/summary'
import type { LcuOrSgpGameSummary } from '@shared/data-adapter/wrapper'

import type { ArchivedMatch } from '../archive/archive-api'

export interface AiNote {
  id: string
  title: string
  body: string
  championId: number
  updatedAt: number
}
export interface AiMatch {
  id: string
  at: number
  queue: number
  mode: string
  champion: string
  won: boolean | null
  kills: number
  deaths: number
  assists: number
  duration: number
  position?: string | null
}
export const AI_MODES = [
  'chat',
  'lineup',
  'history',
  'weekly',
  'review',
  'beginner',
  'fun',
  'knowledge',
  'notes'
] as const
export type AiMode = (typeof AI_MODES)[number]
export const MODE_PROMPTS: Record<AiMode, string> = {
  chat: '',
  lineup:
    '解释当前双方阵容各自的优势、需要注意的配合，以及我可以采用的打法。如果还在选人，结合我可见的常玩英雄给出三个可选思路。不要把建议写成确定结论。',
  history:
    '总结所选玩家的可见近期战绩，按队列分别说明表现；给出一个有依据的优点、一个值得练习的方向和下一局可执行的小目标。不要从 KDA 推断具体死亡原因。',
  weekly:
    '根据近七天与前七天样本，生成个人成长周报。注明样本覆盖范围，不足以对比时直接说明。给出一个进步点和一个下周练习目标。',
  review:
    '复盘选中的这一场对局：解释可见的数据，区分已知事实与待核实的问题，最后给出三个下次可尝试的动作。当前没有时间线，不能编造对线、团战和死亡过程。',
  beginner:
    '为所选英雄生成一份适合新手的简短上手指南：定位、玩法目标、三个常见误区、一个练习方法。当前版本的数值、技能细节和出装没有资料时不做确定推荐。',
  fun: '结合所选玩家可见的常玩英雄，推荐三种轻松有趣的玩法或开黑组合，说明玩法乐趣和适合什么心情。朋友英雄池未知时说明只是候选组合。',
  knowledge: '',
  notes:
    '整理提供的个人笔记，保留用户自己的经验，区分原始观察和推测；结合本局已知英雄指出哪些笔记值得回看。不要改写成未经核实的游戏机制。'
}

export function archiveMatches(records: ArchivedMatch[], owner: string): AiMatch[] {
  return records
    .filter((r) => r.owner === owner)
    .map((r) => ({
      id: r.gameId,
      at: r.createdAt,
      queue: r.queueId,
      mode: r.gameMode,
      champion: r.championName || String(r.championId),
      won: r.win,
      kills: r.kills,
      deaths: r.deaths,
      assists: r.assists,
      duration: r.duration
    }))
}
export function visibleMatches(
  games: LcuOrSgpGameSummary[],
  owner: string,
  championName: (id: number) => string
): AiMatch[] {
  return games.flatMap((game) => {
    try {
      const match = readScoutMatch(game, owner)
      return match
        ? [
            {
              id: String(match.id),
              at: match.at,
              queue: match.queue,
              mode: '',
              champion: championName(match.champion),
              won: match.won,
              kills: match.kills,
              deaths: match.deaths,
              assists: match.assists,
              duration: match.duration,
              position: match.position
            }
          ]
        : []
    } catch {
      return []
    }
  })
}
export function deduplicateMatches(matches: AiMatch[]): AiMatch[] {
  return [...new Map(matches.map((m) => [m.id, m])).values()]
    .sort((a, b) => b.at - a.at)
    .slice(0, 100)
}
export function matchStats(matches: AiMatch[]) {
  const completed = matches.filter((m) => m.duration >= 300 && m.won !== null)
  const stats = (games: AiMatch[]) => ({
    games: games.length,
    wins: games.filter((g) => g.won).length,
    winRate: games.length
      ? Math.round((games.filter((g) => g.won).length / games.length) * 100)
      : null,
    averageDeaths: games.length
      ? Math.round((games.reduce((sum, g) => sum + g.deaths, 0) / games.length) * 10) / 10
      : null
  })
  return {
    ...stats(completed),
    queues: [...new Set(completed.map((m) => m.queue))].map((queue) => ({
      queue,
      ...stats(completed.filter((m) => m.queue === queue))
    }))
  }
}
export function weeklySamples(matches: AiMatch[], now: number) {
  const week = 7 * 86400000
  const current = matches.filter((m) => m.at <= now && m.at >= now - week)
  const previous = matches.filter((m) => m.at < now - week && m.at >= now - 2 * week)
  return {
    current: matchStats(current),
    previous: matchStats(previous),
    currentMatches: current,
    previousMatches: previous
  }
}
