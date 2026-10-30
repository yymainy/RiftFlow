import {
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
import type { RiftFlowScore, SingleAnalysis } from '../types/single'

export function computeSingleRiftFlowScore(summary: SingleAnalysis['summary']): RiftFlowScore {
  const kdaScore = scoreKda(summary.kda)
  const winRateScore = scoreWinRate(summary.win ? 1 : 0)
  const dmgScore = scoreExpectedContribution(
    summary.championDamageRatioToExpectedContribution,
    RIFTFLOW_STANDARD_EXPECTED_CONTRIBUTION_FULL_SCORE_RATIO,
    RIFTFLOW_DAMAGE_WEIGHT
  )
  const dmgTakenScore = scoreExpectedContribution(
    summary.damageTakenRatioToExpectedContribution,
    RIFTFLOW_STANDARD_EXPECTED_CONTRIBUTION_FULL_SCORE_RATIO,
    RIFTFLOW_DAMAGE_TAKEN_WEIGHT
  )
  const healingScore = scoreHealing(
    summary.healingRatioToTeamAverageDamageTaken,
    summary.teamParticipantCount
  )
  const csScore = scoreCsPerMinute(summary.csPerMinute)
  const goldScore = scoreExpectedContribution(
    summary.goldRatioToExpectedContribution,
    RIFTFLOW_GOLD_EXPECTED_CONTRIBUTION_FULL_SCORE_RATIO,
    RIFTFLOW_GOLD_WEIGHT
  )
  const participationScore = scoreParticipation(summary.killParticipation)
  const visionScore = scoreExpectedContribution(
    summary.visionScoreRatioToExpectedContribution,
    RIFTFLOW_STANDARD_EXPECTED_CONTRIBUTION_FULL_SCORE_RATIO,
    RIFTFLOW_VISION_MAX_SCORE
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
    outstanding: false,
    extraordinary: false
  }
}
