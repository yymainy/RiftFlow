import { RiftFlowScorePopoverContent } from '@renderer-shared/components/riftflow-score'

import type { PlayerCardTagDefinition } from '../types'

const TAG_CLASS = 'rounded-xs bg-[#b81b86] px-1 py-0.5 text-[11px] leading-[11px] text-white'

export const GREAT_PERFORMANCE_TAG: PlayerCardTagDefinition = {
  id: 'great-performance',
  render: (ctx) => {
    const analysis = ctx.analysis

    if (!ctx.settings.showGreatPerformanceTag || !analysis) {
      return null
    }

    const score = analysis.riftflowScore

    if (!score.outstanding && !score.extraordinary) {
      return null
    }

    return {
      label: (
        <div class={TAG_CLASS}>
          {score.extraordinary
            ? ctx.t('ongoingGame.playerCard.riftflowLoved.extraordinary')
            : ctx.t('ongoingGame.playerCard.riftflowLoved.outstanding')}
        </div>
      ),
      popover: {
        content: (
          <div class="max-w-60 text-xs">
            <div>
              {score.extraordinary
                ? ctx.t('ongoingGame.playerCard.riftflowLoved.extraordinaryPopover')
                : ctx.t('ongoingGame.playerCard.riftflowLoved.outstandingPopover')}
            </div>
            <RiftFlowScorePopoverContent
              class="mt-2 border-t border-black/10 pt-2 dark:border-white/10"
              score={score}
              totalPrecision={1}
            />
          </div>
        )
      }
    }
  }
}
