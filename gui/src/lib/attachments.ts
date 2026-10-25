/**
 * Attachments for the composer.
 *
 * The agent's prompt API takes exactly two things: text, and image blocks
 * (`PromptOptions.images`). There is no document or file block, so a picked file
 * can only travel one of two ways:
 *
 * - **Images** become image blocks, the way the provider expects them.
 * - **Text** is inlined into the prompt as a `<file name="...">` block, which is
 *   the same convention pi's own `@file` handling uses.
 *
 * That is also why a PDF cannot be attached: it is binary, so inlining it would
 * hand the model thousands of lines of mojibake, and there is no document block
 * to send instead. Binary files are refused with an explanation rather than
 * dumped into the prompt.
 */

/** One attachment waiting to be sent with the next message. */
export interface Attachment {
  /** Stable key for React, and the id used when removing it. */
  id: string
  kind: 'image' | 'text'
  name: string
  /** Media type for images; `text/plain` for inlined text. */
  mimeType: string
  /** Base64 payload, images only, without the data URL prefix. */
  data?: string
  /** The original data URL, used as the thumbnail source. Images only. */
  dataUrl?: string
  /** File contents. Text only. */
  text?: string
  /** Size on disk, for the chip caption. */
  bytes: number
}

/** Refuse anything the providers would reject anyway. */
const MAX_BYTES = 8 * 1024 * 1024
/** Cap on inlined text, so one attachment cannot fill the context window. */
const MAX_TEXT_BYTES = 256 * 1024

/** Extensions the picker offers, on top of anything the OS calls an image. */
export const TEXT_ACCEPT = [
  '.txt',
  '.md',
  '.json',
  '.jsonc',
  '.yaml',
  '.yml',
  '.toml',
  '.ini',
  '.csv',
  '.tsv',
  '.log',
  '.env',
  '.py',
  '.js',
  '.mjs',
  '.cjs',
  '.ts',
  '.tsx',
  '.jsx',
  '.cs',
  '.cpp',
  '.cc',
  '.h',
  '.hpp',
  '.java',
  '.rs',
  '.lua',
  '.go',
  '.rb',
  '.php',
  '.sh',
  '.ps1',
  '.sql',
  '.html',
  '.css',
  '.scss',
  '.xml'
].join(',')

let attachmentId = 0

function nextId(): string {
  attachmentId += 1
  return `att-${attachmentId}`
}

/** Human-readable byte count, for the attachment chips. */
export function humanSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`
}

/**
 * Whether a buffer looks binary. A NUL byte, or a high share of control
 * characters, is what separates a PDF from a text file.
 */
function looksBinary(text: string): boolean {
  const sample = text.slice(0, 4000)
  if (sample.includes('\u0000')) return true

  let control = 0
  for (const character of sample) {
    const code = character.codePointAt(0) ?? 0
    if (code < 9 || (code > 13 && code < 32)) control += 1
  }
  return sample.length > 0 && control / sample.length > 0.05
}

function readAsDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => resolve(String(reader.result))
    reader.onerror = () => reject(new Error(`Could not read ${file.name}.`))
    reader.readAsDataURL(file)
  })
}

function readAsText(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => resolve(String(reader.result))
    reader.onerror = () => reject(new Error(`Could not read ${file.name}.`))
    reader.readAsText(file)
  })
}

/** Read one picked file into an attachment. Throws with a readable reason. */
export async function readAttachment(file: File): Promise<Attachment> {
  if (file.size === 0) throw new Error(`${file.name} is empty.`)
  if (file.size > MAX_BYTES) {
    throw new Error(`${file.name} is larger than ${humanSize(MAX_BYTES)}.`)
  }

  if (file.type.startsWith('image/')) {
    const dataUrl = await readAsDataUrl(file)
    const separator = dataUrl.indexOf(',')
    if (separator < 0) throw new Error(`Could not read ${file.name}.`)

    return {
      id: nextId(),
      kind: 'image',
      name: file.name,
      mimeType: file.type,
      data: dataUrl.slice(separator + 1),
      dataUrl,
      bytes: file.size
    }
  }

  if (file.size > MAX_TEXT_BYTES) {
    throw new Error(
      `${file.name} is ${humanSize(file.size)}; the limit for text files is ${humanSize(MAX_TEXT_BYTES)}.`
    )
  }

  const text = await readAsText(file)
  if (looksBinary(text)) {
    throw new Error(
      `${file.name} is not a text file. The agent's prompt API only accepts text or images — ` +
        'there is no document block, so PDFs and archives cannot be attached.'
    )
  }

  return {
    id: nextId(),
    kind: 'text',
    name: file.name,
    mimeType: 'text/plain',
    text,
    bytes: file.size
  }
}

/** Read a whole selection, collecting per-file failures instead of aborting. */
export async function readAttachments(
  files: FileList | File[]
): Promise<{ added: Attachment[]; errors: string[] }> {
  const added: Attachment[] = []
  const errors: string[] = []

  for (const file of Array.from(files)) {
    try {
      added.push(await readAttachment(file))
    } catch (cause) {
      errors.push(cause instanceof Error ? cause.message : String(cause))
    }
  }

  return { added, errors }
}

/**
 * Build an attachment for a path rather than a `File`.
 *
 * A drop from another application arrives as a `File`, but a drag from PiUI's
 * own file panel carries only a path, so the contents have to come back through
 * the file IPC. That channel is text-only, so an image dropped this way is
 * refused the same way the picker refuses any other binary file.
 */
export async function attachmentFromPath(path: string): Promise<Attachment> {
  const name = path.split(/[\\/]/).filter(Boolean).pop() ?? path
  const file = await window.luna.readFile(path)
  if (file.error) throw new Error(`${name}: ${file.error}`)

  const text = file.content ?? ''
  if (text.length > MAX_TEXT_BYTES) {
    throw new Error(
      `${name} is ${humanSize(text.length)}; the limit for text files is ${humanSize(MAX_TEXT_BYTES)}.`
    )
  }
  if (looksBinary(text)) {
    throw new Error(
      `${name} is not a text file, and this way of adding a file can only read text. ` +
        'Use the attach button instead.'
    )
  }

  return {
    id: nextId(),
    kind: 'text',
    name,
    mimeType: 'text/plain',
    text,
    bytes: text.length
  }
}

/** The image blocks the agent expects. */
export function toPromptImages(
  attachments: Attachment[]
): { type: 'image'; data: string; mimeType: string }[] {
  return attachments
    .filter((attachment) => attachment.kind === 'image' && attachment.data !== undefined)
    .map((attachment) => ({
      type: 'image' as const,
      data: attachment.data as string,
      mimeType: attachment.mimeType
    }))
}

/**
 * Text attachments, inlined after the prompt. The `<file>` wrapper is the same
 * one pi uses for `@file` arguments, so the model sees a familiar shape.
 */
export function inlineTextAttachments(attachments: Attachment[]): string {
  return attachments
    .filter((attachment) => attachment.kind === 'text')
    .map((attachment) => `<file name="${attachment.name}">\n${attachment.text ?? ''}\n</file>`)
    .join('\n')
}
