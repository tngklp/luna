/**
 * Reading tool arguments that are still being streamed.
 *
 * Arguments arrive as text appended to a character at a time, so for most of a
 * call the JSON is not parseable — `JSON.parse` sees `{"edits":[{"oldText":"a`.
 * Showing that raw is unreadable, and waiting for the closing brace means showing
 * nothing at all while the model writes a file.
 *
 * So this scans for `"key": "value"` pairs and reads each value to its closing
 * quote, treating an unterminated final value as complete. That always shows the
 * newest text. It is a preview, not a parser: nested objects and numbers are
 * ignored, and there is no attempt to be correct about malformed input.
 */
import { toolDescription } from './format'

/** A `"key": "` opener, at any depth. */
const OPENER = /"([A-Za-z_][A-Za-z0-9_]*)"\s*:\s*"/g

/** The escapes a JSON string can contain that are worth honouring in a preview. */
const ESCAPES: Record<string, string> = {
  '"': '"',
  '\\': '\\',
  '/': '/',
  b: '\b',
  f: '\f',
  n: '\n',
  r: '\r',
  t: '\t'
}

function unescapeAt(text: string, index: number): { value: string; next: number } {
  const marker = text[index + 1]
  if (marker === undefined) return { value: '', next: index + 1 }
  if (marker === 'u') {
    const hex = text.slice(index + 2, index + 6)
    const code = Number.parseInt(hex, 16)
    return Number.isNaN(code)
      ? { value: '', next: index + 2 }
      : { value: String.fromCharCode(code), next: index + 6 }
  }
  return { value: ESCAPES[marker] ?? marker, next: index + 2 }
}

/**
 * Every string value found for each key, in the order they appear.
 *
 * A list rather than a single value because keys repeat: an `edit` call carries
 * one `newText` per entry, and the one worth showing while it streams is the last.
 */
export function partialStringFields(text: string): Record<string, string[]> {
  const fields: Record<string, string[]> = {}
  if (text.length === 0) return fields

  OPENER.lastIndex = 0
  let opener = OPENER.exec(text)

  while (opener) {
    const key = opener[1] as string
    let value = ''
    let index = opener.index + opener[0].length
    let closed = false

    while (index < text.length) {
      const character = text[index] as string
      if (character === '\\') {
        const escape = unescapeAt(text, index)
        value += escape.value
        index = escape.next
        continue
      }
      if (character === '"') {
        closed = true
        index += 1
        break
      }
      value += character
      index += 1
    }

    ;(fields[key] ??= []).push(value)

    // An unterminated string owns everything after it, so there is nothing left
    // to find that is not part of this value.
    if (!closed) break

    OPENER.lastIndex = index
    opener = OPENER.exec(text)
  }

  return fields
}

/** Longest preview kept in the DOM; a whole file being written is not a preview. */
const PREVIEW_LIMIT = 4000

function tail(text: string): string {
  return text.length <= PREVIEW_LIMIT ? text : `…\n${text.slice(-PREVIEW_LIMIT)}`
}

/**
 * What to put in a streaming tool card's header.
 *
 * `toolDescription` falls back to the raw arguments for a tool it does not know,
 * which mid-stream is half-written JSON — the name alone reads better than that.
 */
export function streamedToolDescription(name: string, argsText: string): string {
  const fields = partialStringFields(argsText)
  const flat: Record<string, string> = {}
  for (const [key, values] of Object.entries(fields)) flat[key] = values[0] ?? ''

  const described = toolDescription(name, flat)
  return described.startsWith('{') || described.startsWith('[') ? name : described
}

/** The text a `write` or `edit` call is producing, so it can be watched arrive. */
export function streamedPreview(name: string, argsText: string): string {
  const fields = partialStringFields(argsText)

  if (name === 'write') return tail(fields.content?.[0] ?? '')
  // The newest replacement is the one being typed right now.
  if (name === 'edit') return tail(fields.newText?.[fields.newText.length - 1] ?? '')
  return ''
}
