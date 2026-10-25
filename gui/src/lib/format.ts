/** Formatting helpers for usage, cost, and paths in the UI. */

export function formatTokens(value: number): string {
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`
  if (value >= 1_000) return `${(value / 1_000).toFixed(1)}k`
  return String(value)
}

export function formatCost(value: number): string {
  if (value === 0) return '$0.00'
  if (value < 0.01) return `$${value.toFixed(4)}`
  return `$${value.toFixed(2)}`
}

export function formatPercent(value: number | null): string {
  if (value === null) return '—'
  return `${Math.round(value)}%`
}

/** Format a wall-clock duration for the tool-card header. */
export function formatDuration(ms: number): string {
  if (ms < 1000) return `${Math.max(1, Math.round(ms))}ms`
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)}s`
  const minutes = Math.floor(ms / 60_000)
  const seconds = Math.round((ms % 60_000) / 1000)
  return `${minutes}m ${seconds}s`
}

/** Shorten a filesystem path for display without losing the file name. */
export function shortenPath(path: string, maxLength = 48): string {
  if (path.length <= maxLength) return path
  const separator = path.includes('\\') ? '\\' : '/'
  const parts = path.split(separator)
  const tail = parts.slice(-2).join(separator)
  return `…${separator}${tail}`
}

/** Render a tool-call argument object compactly for inline display. */
export function summarizeToolArguments(args: unknown): string {
  if (args === null || args === undefined) return ''
  if (typeof args === 'string') return args
  if (typeof args !== 'object') return String(args)

  const record = args as Record<string, unknown>
  const preferred = ['command', 'path', 'file_path', 'pattern', 'query', 'url']
  for (const key of preferred) {
    const value = record[key]
    if (typeof value === 'string' && value.length > 0) return value
  }

  try {
    return JSON.stringify(args)
  } catch {
    return ''
  }
}

/**
 * One-line description of a tool call, the way an editor labels an action in a
 * list: "Read src/app.ts", "Ran bash: npm test". Used by the collapsed card, so
 * the transcript stays readable without expanding anything.
 */
export function toolDescription(name: string, args: unknown): string {
  const record = (args ?? {}) as Record<string, unknown>
  const path =
    typeof record.path === 'string'
      ? record.path
      : typeof record.file_path === 'string'
        ? record.file_path
        : null
  const command = typeof record.command === 'string' ? record.command : null

  switch (name) {
    case 'read':
      return path ? `Read ${path}` : 'Read a file'
    case 'write':
      return path ? `Wrote ${path}` : 'Wrote a file'
    case 'edit':
      return path ? `Edited ${path}` : 'Edited a file'
    case 'bash':
      return command ? `Ran bash: ${command}` : 'Ran a command'
    case 'powershell':
      return command ? `Ran PowerShell: ${command}` : 'Ran a command'
    default:
      return summarizeToolArguments(args) || name
  }
}
