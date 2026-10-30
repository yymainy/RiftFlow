// P.S. tsx 文件名用于保证 tailwind 自动扫描
export function getTeamIndicatorColorClass(team: string) {
  switch (team) {
    case 'TEAM-100':
      return 'bg-[#7dabbf]'

    case 'TEAM-200':
      return 'bg-[#c98c8c]'

    case 'LOBBY':
      return 'bg-neutral-500 dark:bg-neutral-200'

    default:
      return null
  }
}

export const WIN_RATE_TEAM_TAG_BG_CLASSES = 'bg-[#65516b]'
export const LOSS_RATE_TEAM_TAG_BG_CLASSES = 'bg-[#775353]'
export const WIN_RATE_GTE_50_TEXT_CLASSES = 'text-[#2c8c6c] dark:text-[#4cc69d]'
export const WIN_RATE_LT_50_TEXT_CLASSES = 'text-[#cc0000] dark:text-[#ff6161]'
