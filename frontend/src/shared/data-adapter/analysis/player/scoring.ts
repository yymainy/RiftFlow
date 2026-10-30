import {
  RIFTFLOW_CS_FULL_SCORE_PER_MINUTE,
  RIFTFLOW_CS_MAX_SCORE,
  RIFTFLOW_CS_MIN_SCORE_PER_MINUTE,
  RIFTFLOW_EXPECTED_CONTRIBUTION_BASELINE_RATIO,
  RIFTFLOW_HEALING_FULL_SCORE_TEAM_AVERAGE_DAMAGE_TAKEN_RATIO,
  RIFTFLOW_HEALING_MAX_SCORE,
  RIFTFLOW_HEALING_MIN_TEAM_AVERAGE_DAMAGE_TAKEN_RATIO,
  RIFTFLOW_HEALING_SOLO_FULL_SCORE_TEAM_AVERAGE_DAMAGE_TAKEN_RATIO,
  RIFTFLOW_KDA_BASELINE,
  RIFTFLOW_KDA_MAX_SCORE,
  RIFTFLOW_KDA_WEIGHT,
  RIFTFLOW_PARTICIPATION_MIN_SHARE,
  RIFTFLOW_PARTICIPATION_WEIGHT,
  RIFTFLOW_WIN_RATE_BASELINE,
  RIFTFLOW_WIN_RATE_WEIGHT
} from './constants'

function clamp(value: number, min: number, max: number) {
  return Math.max(Math.min(value, max), min)
}

function scoreLinearRange(value: number, min: number, max: number, maxScore: number) {
  return ((clamp(value, min, max) - min) / (max - min)) * maxScore
}

export function scoreExpectedContribution(ratio: number, fullScoreRatio: number, maxScore: number) {
  return scoreLinearRange(
    ratio,
    RIFTFLOW_EXPECTED_CONTRIBUTION_BASELINE_RATIO,
    fullScoreRatio,
    maxScore
  )
}

export function scoreKda(kda: number) {
  const effectiveKda = Math.max(kda - RIFTFLOW_KDA_BASELINE, 0)
  return clamp(Math.sqrt(effectiveKda) * RIFTFLOW_KDA_WEIGHT, 0, RIFTFLOW_KDA_MAX_SCORE)
}

export function scoreHealing(
  healingRatioToTeamAverageDamageTaken: number,
  teamParticipantCount: number
) {
  const fullScoreRatio =
    teamParticipantCount === 1
      ? RIFTFLOW_HEALING_SOLO_FULL_SCORE_TEAM_AVERAGE_DAMAGE_TAKEN_RATIO
      : RIFTFLOW_HEALING_FULL_SCORE_TEAM_AVERAGE_DAMAGE_TAKEN_RATIO

  return scoreLinearRange(
    healingRatioToTeamAverageDamageTaken,
    RIFTFLOW_HEALING_MIN_TEAM_AVERAGE_DAMAGE_TAKEN_RATIO,
    fullScoreRatio,
    RIFTFLOW_HEALING_MAX_SCORE
  )
}

export function scoreWinRate(winRate: number) {
  return scoreLinearRange(winRate, RIFTFLOW_WIN_RATE_BASELINE, 1, RIFTFLOW_WIN_RATE_WEIGHT)
}

export function scoreCsPerMinute(csPerMinute: number) {
  return scoreLinearRange(
    csPerMinute,
    RIFTFLOW_CS_MIN_SCORE_PER_MINUTE,
    RIFTFLOW_CS_FULL_SCORE_PER_MINUTE,
    RIFTFLOW_CS_MAX_SCORE
  )
}

export function scoreParticipation(killParticipation: number) {
  return scoreLinearRange(
    killParticipation,
    RIFTFLOW_PARTICIPATION_MIN_SHARE,
    1,
    RIFTFLOW_PARTICIPATION_WEIGHT
  )
}
