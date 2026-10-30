import type { RiftFlowScore } from '@shared/data-adapter/analysis/player'
import {
  RIFTFLOW_CS_MAX_SCORE,
  RIFTFLOW_DAMAGE_TAKEN_WEIGHT,
  RIFTFLOW_DAMAGE_WEIGHT,
  RIFTFLOW_GOLD_WEIGHT,
  RIFTFLOW_HEALING_MAX_SCORE,
  RIFTFLOW_KDA_MAX_SCORE,
  RIFTFLOW_PARTICIPATION_WEIGHT,
  RIFTFLOW_VISION_MAX_SCORE,
  RIFTFLOW_WIN_RATE_WEIGHT
} from '@shared/data-adapter/analysis/player/constants'

export type RiftFlowScorePartKey =
  | 'kdaScore'
  | 'winRateScore'
  | 'dmgScore'
  | 'dmgTakenScore'
  | 'healingScore'
  | 'csScore'
  | 'goldScore'
  | 'participationScore'
  | 'visionScore'

export type RiftFlowScoreProgressStatus = 'default' | 'info'

export type RiftFlowScoreBreakdownItem = {
  key: RiftFlowScorePartKey
  labelKey: string
  value: number
  max: number | null
  progressPercentage: number
  progressStatus: RiftFlowScoreProgressStatus
}

const RIFTFLOW_SCORE_PARTS: Array<{
  key: RiftFlowScorePartKey
  labelKey: string
  max: number | null
}> = [
  {
    key: 'kdaScore',
    labelKey: 'riftflowScore.parts.kda',
    max: RIFTFLOW_KDA_MAX_SCORE
  },
  {
    key: 'winRateScore',
    labelKey: 'riftflowScore.parts.winRate',
    max: RIFTFLOW_WIN_RATE_WEIGHT
  },
  {
    key: 'dmgScore',
    labelKey: 'riftflowScore.parts.damage',
    max: RIFTFLOW_DAMAGE_WEIGHT
  },
  {
    key: 'dmgTakenScore',
    labelKey: 'riftflowScore.parts.damageTaken',
    max: RIFTFLOW_DAMAGE_TAKEN_WEIGHT
  },
  {
    key: 'healingScore',
    labelKey: 'riftflowScore.parts.healing',
    max: RIFTFLOW_HEALING_MAX_SCORE
  },
  {
    key: 'csScore',
    labelKey: 'riftflowScore.parts.cs',
    max: RIFTFLOW_CS_MAX_SCORE
  },
  {
    key: 'goldScore',
    labelKey: 'riftflowScore.parts.gold',
    max: RIFTFLOW_GOLD_WEIGHT
  },
  {
    key: 'participationScore',
    labelKey: 'riftflowScore.parts.participation',
    max: RIFTFLOW_PARTICIPATION_WEIGHT
  },
  {
    key: 'visionScore',
    labelKey: 'riftflowScore.parts.vision',
    max: RIFTFLOW_VISION_MAX_SCORE
  }
]

function clampProgressPercentage(value: number) {
  if (!Number.isFinite(value)) {
    return 0
  }

  return Math.min(Math.max(value, 0), 100)
}

export function getRiftFlowScoreBreakdownItems(score: RiftFlowScore): RiftFlowScoreBreakdownItem[] {
  return RIFTFLOW_SCORE_PARTS.map((part) => {
    const value = score[part.key]
    const max = part.max
    const hasFixedMax = max !== null

    return {
      ...part,
      value,
      progressPercentage: hasFixedMax ? clampProgressPercentage((value / max) * 100) : 100,
      progressStatus: hasFixedMax ? 'default' : 'info'
    }
  })
}
