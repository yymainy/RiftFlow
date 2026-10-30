import { z } from 'zod'

const RiftFlowApiBootstrapSchema = z.object({
  schemaVersion: z.literal(1),
  generation: z.number(),
  baseUrls: z.object({
    api: z.string(),
    static: z.string()
  })
})

export function parseRiftFlowApiBootstrapDocument(value: unknown) {
  return RiftFlowApiBootstrapSchema.parse(value)
}

export function resolveRiftFlowStaticUrl(baseUrl: string, path: string) {
  return new URL(path, `${baseUrl}/`).toString()
}
