/** IPC may reject with a plain object rather than an Error instance. */
export function archiveErrorMessage(error: unknown, fallback: string): string {
  const seen = new Set<unknown>()
  function message(value: unknown, depth: number): string | undefined {
    if (depth > 5 || value == null || seen.has(value)) return
    seen.add(value)
    if (typeof value === 'string') {
      const text = value.trim()
      return text && text !== '[object Object]' ? text : undefined
    }
    if (typeof value === 'object') {
      for (const field of ['message', 'reason', 'error', 'cause']) {
        const result = message((value as Record<string, unknown>)[field], depth + 1)
        if (result) return result
      }
    }
    return undefined
  }
  return message(error, 0) || fallback
}
