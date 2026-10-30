import type { RiftFlowAutoSelectGroup } from './types'

export function isAutoSelectGroupSupportedOnSgpServer(
  group: Pick<RiftFlowAutoSelectGroup, 'supportedSgpServers'>,
  sgpServerId: string,
  leagueServers: Readonly<Record<string, unknown>>
) {
  if (group.supportedSgpServers.includes('*')) {
    return true
  }

  return (
    Boolean(sgpServerId) &&
    Object.hasOwn(leagueServers, sgpServerId) &&
    group.supportedSgpServers.includes(sgpServerId)
  )
}
