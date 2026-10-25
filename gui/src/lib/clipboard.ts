import type { ChatBlockDto } from '@shared/ipc'

/**
 * Put text on the system clipboard.
 *
 * `navigator.clipboard` needs a secure context, which the custom `piui-file`
 * scheme and the dev server both are. A failure is returned rather than thrown,
 * because the only useful response is to say nothing happened.
 */
export async function copyText(value: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(value)
    return true
  } catch {
    return false
  }
}

/**
 * The assistant's answer as markdown.
 *
 * Only text blocks: reasoning is not part of what the assistant said, and a
 * pasted chain of thought would be noise. Blocks are joined with a blank line so
 * a reply split around tool calls still reads as one document.
 */
export function answerMarkdown(blocks: ChatBlockDto[]): string {
  return blocks
    .filter((block): block is Extract<ChatBlockDto, { type: 'text' }> => block.type === 'text')
    .map((block) => block.text.trim())
    .filter((text) => text.length > 0)
    .join('\n\n')
}
