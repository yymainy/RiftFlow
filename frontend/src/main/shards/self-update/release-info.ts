import type { RiftFlowRelease, RiftFlowReleaseArtifact } from '@shared/shards/riftflow-api'
import type { SelfUpdateReleaseInfo } from '@shared/shards/self-update'
import { gt } from 'semver'

import { shouldRunSelfUpdateLifecycle } from './platform'

export interface SelfUpdateTarget {
  platform: NodeJS.Platform
  arch: string
}

export function isSupportedWin32X64Artifact(artifact: RiftFlowReleaseArtifact) {
  if (artifact.platform !== 'win32' || artifact.arch !== 'x64') {
    return false
  }

  if (artifact.contentType === 'application/x-7z-compressed') {
    return true
  }

  return (
    artifact.contentType === 'application/octet-stream' &&
    artifact.fileName.toLowerCase().endsWith('.7z')
  )
}

export function resolveSelfUpdateReleaseInfo(
  release: RiftFlowRelease | null,
  currentVersion: string,
  target: SelfUpdateTarget
): SelfUpdateReleaseInfo | null {
  if (!release) {
    return null
  }

  const artifact = shouldRunSelfUpdateLifecycle(target.platform, target.arch)
    ? (release.artifacts.find(isSupportedWin32X64Artifact) ?? null)
    : null

  return {
    version: release.version,
    currentVersion,
    publishedAt: release.publishedAt,
    description: release.description,
    isNew: gt(release.version, currentVersion),
    isUpdateSupported: artifact !== null,
    artifact
  }
}
