import { useEffect, useMemo, useRef, useState } from 'react'
import { useLuna } from '../store'

/**
 * The command palette.
 *
 * One list for everything the user can jump to: sessions, commands, and the
 * settings tabs. `Ctrl+P` opens it, typing filters, Enter runs the highlighted
 * entry.
 */

export type PaletteKind = 'session' | 'command' | 'setting'

export interface PaletteEntry {
  id: string
  kind: PaletteKind
  title: string
  hint: string
  run: () => void
}

/** Rank an entry against a query: lower is better, `None` means no match. */
export function rank(title: string, hint: string, query: string): number | null {
  if (query.length === 0) return 1
  const needle = query.toLowerCase()
  const name = title.toLowerCase()
  if (name.startsWith(needle)) return 0
  if (name.includes(needle)) return 1
  if (hint.toLowerCase().includes(needle)) return 2
  return null
}

/** Filter and sort entries for a query. */
export function filterEntries(entries: PaletteEntry[], query: string): PaletteEntry[] {
  const scored: { rank: number; entry: PaletteEntry }[] = []
  for (const entry of entries) {
    const score = rank(entry.title, entry.hint, query)
    if (score !== null) scored.push({ rank: score, entry })
  }
  scored.sort((a, b) => a.rank - b.rank || a.entry.title.localeCompare(b.entry.title))
  return scored.map((item) => item.entry)
}

export function CommandPalette() {
  const open = useLuna((state) => state.quickOpen)
  const close = useLuna((state) => state.closeQuickOpen)
  const sessions = useLuna((state) => state.sessions)
  const commands = useLuna((state) => state.commands)
  const switchSession = useLuna((state) => state.switchSession)
  const newSession = useLuna((state) => state.newSession)
  const openSettings = useLuna((state) => state.openSettings)
  const setSettingsTab = useLuna((state) => state.setSettingsTab)
  const send = useLuna((state) => state.send)

  const [query, setQuery] = useState('')
  const [cursor, setCursor] = useState(0)
  const inputRef = useRef<HTMLInputElement | null>(null)

  useEffect(() => {
    if (!open) return
    setQuery('')
    setCursor(0)
    // Focus after the dialog paints.
    const frame = requestAnimationFrame(() => inputRef.current?.focus())
    return () => cancelAnimationFrame(frame)
  }, [open])

  const entries = useMemo<PaletteEntry[]>(() => {
    const list: PaletteEntry[] = [
      {
        id: 'new-session',
        kind: 'command',
        title: 'New session',
        hint: 'Start a blank conversation',
        run: () => void newSession()
      },
      ...commands.map((command) => ({
        id: `cmd-${command.name}`,
        kind: 'command' as const,
        title: command.name,
        hint: command.description,
        run: () => void send(command.name)
      })),
      ...(['general', 'models', 'customization', 'tools'] as const).map((tab) => ({
        id: `set-${tab}`,
        kind: 'setting' as const,
        title: `Settings: ${tab}`,
        hint: 'Open this settings tab',
        run: () => {
          setSettingsTab(tab)
          openSettings()
        }
      })),
      ...sessions.map((session) => ({
        id: `sess-${session.path}`,
        kind: 'session' as const,
        title: session.name ?? session.firstMessage ?? 'Untitled session',
        hint: session.cwd,
        run: () => void switchSession(session.path)
      }))
    ]
    return list
  }, [commands, sessions, newSession, send, setSettingsTab, openSettings, switchSession])

  const matches = useMemo(() => filterEntries(entries, query).slice(0, 40), [entries, query])

  if (!open) return null

  const choose = (entry: PaletteEntry | undefined): void => {
    if (!entry) return
    close()
    entry.run()
  }

  return (
    <div className="modal" role="dialog" aria-modal="true" aria-label="Command palette">
      <div className="modal__backdrop" onClick={close} />
      <div className="palette">
        <input
          ref={inputRef}
          className="palette__input"
          value={query}
          placeholder="Search sessions, commands and settings…"
          aria-label="Command palette search"
          onChange={(event) => {
            setQuery(event.target.value)
            setCursor(0)
          }}
          onKeyDown={(event) => {
            if (event.key === 'Escape') {
              event.preventDefault()
              close()
            } else if (event.key === 'ArrowDown') {
              event.preventDefault()
              setCursor((current) => Math.min(current + 1, matches.length - 1))
            } else if (event.key === 'ArrowUp') {
              event.preventDefault()
              setCursor((current) => Math.max(current - 1, 0))
            } else if (event.key === 'Enter') {
              event.preventDefault()
              choose(matches[cursor])
            }
          }}
        />
        <div className="palette__list">
          {matches.length === 0 ? (
            <p className="palette__empty">Nothing matches.</p>
          ) : null}
          {matches.map((entry, index) => (
            <button
              key={entry.id}
              className={`palette__row${index === cursor ? ' on' : ''}`}
              onMouseEnter={() => setCursor(index)}
              onClick={() => choose(entry)}
            >
              <span className={`palette__kind ${entry.kind}`}>{entry.kind}</span>
              <span className="palette__title">{entry.title}</span>
              <span className="palette__hint">{entry.hint}</span>
            </button>
          ))}
        </div>
      </div>
    </div>
  )
}
