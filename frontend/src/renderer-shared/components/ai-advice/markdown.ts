import MarkdownIt from 'markdown-it'

// Model output is untrusted: allow formatting, never raw HTML or remote images.
const renderer = new MarkdownIt({ html: false, linkify: false, breaks: true }).disable('image')
renderer.renderer.rules.link_open = (tokens, index, options, _env, self) => {
  tokens[index].attrSet('target', '_blank')
  tokens[index].attrSet('rel', 'noopener noreferrer')
  return self.renderToken(tokens, index, options)
}
export const renderAiMarkdown = (text: string) => renderer.render(text)
