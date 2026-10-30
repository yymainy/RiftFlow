import { gte, valid } from 'semver'
import { z } from 'zod'

import type {
  RiftFlowAutoSelectGroupsConfig,
  RiftFlowContactChannels,
  RiftFlowFeatureGateSnapshot,
  RiftFlowLeagueServersConfig,
  RiftFlowNotice,
  RiftFlowRelease,
  RiftFlowSupportedQueuesConfig
} from './types'

const ConfigMetadataShape = {
  updatedAt: z.iso.datetime({ offset: true })
}

const UniqueNonEmptyStringArraySchema = z
  .array(z.string().trim().min(1).max(64))
  .min(1)
  .refine((values) => new Set(values).size === values.length)

const RiftFlowFeatureGateRuntimeRuleSchema = z
  .object({
    platforms: z
      .array(z.enum(['win32', 'darwin']))
      .min(1)
      .refine((values) => new Set(values).size === values.length)
      .optional(),
    minVersionInclusive: z.string().optional(),
    maxVersionExclusive: z.string().optional(),
    sgpServers: UniqueNonEmptyStringArraySchema.optional()
  })
  .superRefine((gate, context) => {
    const minVersion = gate.minVersionInclusive && valid(gate.minVersionInclusive)
    const maxVersion = gate.maxVersionExclusive && valid(gate.maxVersionExclusive)

    if (gate.minVersionInclusive && !minVersion) {
      context.addIssue({
        code: 'custom',
        path: ['minVersionInclusive'],
        message: 'minVersionInclusive must be valid SemVer'
      })
    }
    if (gate.maxVersionExclusive && !maxVersion) {
      context.addIssue({
        code: 'custom',
        path: ['maxVersionExclusive'],
        message: 'maxVersionExclusive must be valid SemVer'
      })
    }
    if (minVersion && maxVersion && gte(minVersion, maxVersion)) {
      context.addIssue({
        code: 'custom',
        path: ['maxVersionExclusive'],
        message: 'maxVersionExclusive must be greater than minVersionInclusive'
      })
    }
  })
  .strict()

export const RiftFlowFeatureGateSnapshotSchema: z.ZodType<RiftFlowFeatureGateSnapshot> = z
  .object({
    ...ConfigMetadataShape,
    gates: z.record(
      z.string().regex(/^[a-z][a-z0-9-]*(?:\.[a-z][a-z0-9-]*)+$/),
      RiftFlowFeatureGateRuntimeRuleSchema
    )
  })
  .strict()

export const RiftFlowNoticeSchema: z.ZodType<RiftFlowNotice> = z
  .object({
    revision: z.string(),
    language: z.enum(['zh-CN', 'en']),
    severity: z.enum(['low', 'medium', 'high']),
    summary: z.string(),
    contentType: z.literal('text/markdown'),
    content: z.string(),
    updatedAt: z.iso.datetime({ offset: true })
  })
  .passthrough()

const RiftFlowContactChannelUrlSchema = z.url().refine(
  (value) => {
    const protocol = new URL(value).protocol
    return protocol === 'https:' || protocol === 'mailto:'
  },
  { message: 'url must use https or mailto' }
)

export const RiftFlowContactChannelsSchema: z.ZodType<RiftFlowContactChannels> = z
  .object({
    ...ConfigMetadataShape,
    channels: z.array(
      z
        .object({
          id: z
            .string()
            .trim()
            .min(1)
            .max(64)
            .regex(/^[a-z0-9][a-z0-9_-]*$/i),
          platform: z.enum(['qq', 'telegram', 'discord', 'email', 'website', 'other']),
          name: z.string().trim().min(1).max(80),
          identifier: z.string().trim().min(1).max(120),
          url: RiftFlowContactChannelUrlSchema,
          password: z.string().trim().min(1).max(120).nullable()
        })
        .passthrough()
    )
  })
  .passthrough()

export const RiftFlowAutoSelectGroupsConfigSchema: z.ZodType<RiftFlowAutoSelectGroupsConfig> = z
  .object({
    ...ConfigMetadataShape,
    groups: z.array(
      z
        .object({
          groupId: z.string(),
          name: z.object({
            'zh-CN': z.string(),
            en: z.string()
          }),
          iconPath: z.string().regex(/^\/lol-game-data\/assets\/.+/),
          isCustom: z.boolean(),
          supportedSgpServers: z.array(z.string()).min(1),
          targetGameModes: z.array(
            z
              .object({
                gameMode: z.string(),
                queueTypes: z.array(z.string())
              })
              .passthrough()
          ),
          positions: z.array(z.string()),
          additionalPicks: z.array(z.number()),
          additionalBans: z.array(z.number()),
          excludedPicks: z.array(z.number()),
          excludedBans: z.array(z.number())
        })
        .passthrough()
    )
  })
  .passthrough()

export const RiftFlowLeagueServersConfigSchema: z.ZodType<RiftFlowLeagueServersConfig> = z
  .object({
    ...ConfigMetadataShape,
    servers: z.record(
      z.string(),
      z
        .object({
          matchHistory: z.string(),
          common: z.string(),
          isTencent: z.boolean(),
          regionPathParam: z.string().optional()
        })
        .passthrough()
    ),
    serverNames: z.record(z.string(), z.record(z.string(), z.string()))
  })
  .passthrough()

export const RiftFlowSupportedQueuesConfigSchema: z.ZodType<RiftFlowSupportedQueuesConfig> = z
  .object({
    ...ConfigMetadataShape,
    queues: z.array(z.number())
  })
  .passthrough()

export const RiftFlowReleaseSchema: z.ZodType<RiftFlowRelease> = z
  .object({
    version: z.string(),
    publishedAt: z.iso.datetime({ offset: true }),
    description: z.string(),
    artifacts: z.array(
      z
        .object({
          platform: z.string(),
          arch: z.string(),
          fileName: z.string(),
          size: z.number(),
          contentType: z.string(),
          sha256: z.string().nullable(),
          downloadUrl: z.url()
        })
        .passthrough()
    )
  })
  .passthrough()
