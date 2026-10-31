import { describe, expect, it } from 'vitest'

import { AdviceLifecycle } from './lifecycle'
import { renderAiMarkdown } from './markdown'

describe('automatic advice lifecycle', () => {
  it('only attempts a match once across roster updates and a transient disconnect', () => {
    const lifecycle = new AdviceLifecycle()
    expect(lifecycle.start('account:game1')).toBe(true)
    expect(lifecycle.start('')).toBe(false)
    expect(lifecycle.start('account:game1')).toBe(false)
    expect(lifecycle.start('account:game2')).toBe(true)
  })
  it('dismissing a pending or completed match prevents reopening but permits the next match', () => {
    const lifecycle = new AdviceLifecycle()
    lifecycle.dismiss('account:game1')
    expect(lifecycle.start('account:game1')).toBe(false)
    expect(lifecycle.isDismissed('account:game1')).toBe(true)
    expect(lifecycle.start('account:game2')).toBe(true)
    lifecycle.dismiss('account:game2')
    expect(lifecycle.canStart('account:game2')).toBe(false)
  })
})
describe('model response formatting', () => {
  it('formats headings, emphasis, and lists without rendering raw HTML or remote images', () => {
    const result = renderAiMarkdown(
      '## 开局思路\n\n**先配合队友**\n\n- 留意开团\n\n<script>alert(1)</script>\n\n![tracking](https://example.com/pixel)\n\n[bad](javascript:alert(1))'
    )
    expect(result).toContain('<h2>开局思路</h2>')
    expect(result).toContain('<strong>先配合队友</strong>')
    expect(result).toContain('<li>留意开团</li>')
    expect(result).not.toContain('<script>')
    expect(result).not.toContain('<img')
    expect(result).not.toContain('href="javascript:')
  })
})
