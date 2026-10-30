import { RiftFlowScorePopoverContent } from '@renderer-shared/components/riftflow-score'

import type { PlayerCardTagDefinition } from '../types'

const TAG_CLASS = 'rounded-xs bg-[#b81b86] px-1 py-0.5 text-[11px] leading-[11px] text-white'

export const RIFTFLOW_SCORE_TAG: PlayerCardTagDefinition = {
  id: 'riftflow-score',
  render: (ctx) => {
    const analysis = ctx.analysis

    if (!ctx.settings.showRiftFlowScoreTag || !analysis) {
      return null
    }

    return {
      label: <div class={TAG_CLASS}>RiftFlow {analysis.riftflowScore.total.toFixed(2)}</div>,
      popover: {
        content: (
          <div class="w-64">
            <RiftFlowScorePopoverContent score={analysis.riftflowScore} totalPrecision={3} />
            <div class="mt-2 text-xs leading-4 text-black/55 dark:text-white/55">
              {ctx.t('ongoingGame.playerCard.riftflowScorePopoverDescription')}
            </div>
          </div>
        )
      }
    }
  }
}
