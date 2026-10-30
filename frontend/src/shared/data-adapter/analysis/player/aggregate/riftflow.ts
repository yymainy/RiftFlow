import {
  AGGREGATE_RIFTFLOW_EXTRAORDINARY_MIN_COUNT,
  AGGREGATE_RIFTFLOW_EXTRAORDINARY_THRESHOLD,
  AGGREGATE_RIFTFLOW_OUTSTANDING_MIN_COUNT,
  AGGREGATE_RIFTFLOW_OUTSTANDING_THRESHOLD,
  RIFTFLOW_DAMAGE_TAKEN_WEIGHT,
  RIFTFLOW_DAMAGE_WEIGHT,
  RIFTFLOW_GOLD_EXPECTED_CONTRIBUTION_FULL_SCORE_RATIO,
  RIFTFLOW_GOLD_WEIGHT,
  RIFTFLOW_MAX_SCORE,
  RIFTFLOW_STANDARD_EXPECTED_CONTRIBUTION_FULL_SCORE_RATIO,
  RIFTFLOW_VISION_MAX_SCORE
} from '../constants'
import {
  scoreCsPerMinute,
  scoreExpectedContribution,
  scoreHealing,
  scoreKda,
  scoreParticipation,
  scoreWinRate
} from '../scoring'
import type { AggregatedAnalysis } from '../types/aggregated'
import type { PreparedGame } from '../types/helpers'
import type { RiftFlowScore } from '../types/single'
import { avgOrZero } from '../utils/math'

export function computeAggregatedRiftFlowScore(analysis: {
  count: number
  summary: AggregatedAnalysis['summary']
  games: PreparedGame[]
}): RiftFlowScore {
  const summaries = analysis.games.map((game) => game.single.summary)
  const kdaScore = scoreKda(analysis.summary.avgKda)
  const winRateScore = scoreWinRate(analysis.summary.winRate)
  const dmgScore = avgOrZero(
    summaries.map((summary) =>
      scoreExpectedContribution(
        summary.championDamageRatioToExpectedContribution,
        RIFTFLOW_STANDARD_EXPECTED_CONTRIBUTION_FULL_SCORE_RATIO,
        RIFTFLOW_DAMAGE_WEIGHT
      )
    )
  )
  const dmgTakenScore = avgOrZero(
    summaries.map((summary) =>
      scoreExpectedContribution(
        summary.damageTakenRatioToExpectedContribution,
        RIFTFLOW_STANDARD_EXPECTED_CONTRIBUTION_FULL_SCORE_RATIO,
        RIFTFLOW_DAMAGE_TAKEN_WEIGHT
      )
    )
  )
  const healingScore = avgOrZero(
    summaries.map((summary) =>
      scoreHealing(summary.healingRatioToTeamAverageDamageTaken, summary.teamParticipantCount)
    )
  )
  const csScore = avgOrZero(summaries.map((summary) => scoreCsPerMinute(summary.csPerMinute)))
  const goldScore = avgOrZero(
    summaries.map((summary) =>
      scoreExpectedContribution(
        summary.goldRatioToExpectedContribution,
        RIFTFLOW_GOLD_EXPECTED_CONTRIBUTION_FULL_SCORE_RATIO,
        RIFTFLOW_GOLD_WEIGHT
      )
    )
  )
  const participationScore = avgOrZero(
    summaries.map((summary) => scoreParticipation(summary.killParticipation))
  )
  const visionScore = avgOrZero(
    summaries.map((summary) =>
      scoreExpectedContribution(
        summary.visionScoreRatioToExpectedContribution,
        RIFTFLOW_STANDARD_EXPECTED_CONTRIBUTION_FULL_SCORE_RATIO,
        RIFTFLOW_VISION_MAX_SCORE
      )
    )
  )

  const total =
    kdaScore +
    winRateScore +
    dmgScore +
    dmgTakenScore +
    healingScore +
    csScore +
    goldScore +
    participationScore +
    visionScore

  return {
    kdaScore,
    winRateScore,
    dmgScore,
    dmgTakenScore,
    healingScore,
    csScore,
    goldScore,
    participationScore,
    visionScore,
    total,
    maxScore: RIFTFLOW_MAX_SCORE,
    outstanding:
      total >= AGGREGATE_RIFTFLOW_OUTSTANDING_THRESHOLD &&
      analysis.count >= AGGREGATE_RIFTFLOW_OUTSTANDING_MIN_COUNT,
    extraordinary:
      total >= AGGREGATE_RIFTFLOW_EXTRAORDINARY_THRESHOLD &&
      analysis.count >= AGGREGATE_RIFTFLOW_EXTRAORDINARY_MIN_COUNT
  }
}
