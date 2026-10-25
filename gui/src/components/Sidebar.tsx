import { useEffect, useMemo, useRef, useState } from 'react'
import type { SessionSummaryDto } from '@shared/ipc'
import { useLuna } from '../store'

type Bucket = 'Today' | 'Yesterday' | 'Previous 7 days' | 'Older'

function startOfToday(): number {
  const now = new Date()
  return new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime()
}

function bucketOf(iso: string): Bucket {
  const then = new Date(iso).getTime()
  const today = startOfToday()
  if (then >= today) return 'Today'
  if (then >= today - 86_400_000) return 'Yesterday'
  if (then >= today - 7 * 86_400_000) return 'Previous 7 days'
  return 'Older'
}

/** A short, human timestamp: the time today, the date before that. */
function stamp(iso: string): string {
  const then = new Date(iso)
  const minutes = Math.floor((Date.now() - then.getTime()) / 60_000)
  if (minutes < 1) return 'now'
  if (minutes < 60) return `${minutes}m`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours}h`
  const days = Math.floor(hours / 24)
  if (days < 7) return `${days}d`
  return then.toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
}

/** The title a session shows: its name, else its first message. */
function titleOf(session: SessionSummaryDto): string {
  const raw = session.name ?? session.firstMessage ?? ''
  const trimmed = raw.trim()
  if (trimmed.length === 0) return 'New session'
  // A first message can be a paragraph; the list wants a line.
  return trimmed.length > 80 ? `${trimmed.slice(0, 80).trimEnd()}…` : trimmed
}

/**
 * The session sidebar.
 *
 * Laid out like the harnesses people already use: a brand row, one prominent
 * "New session" action, a search field, then the history grouped by recency.
 * The working directory is deliberately absent — it belongs in Settings, not in
 * the place you look to pick up work.
 */
export function Sidebar() {
  const sessions = useLuna((state) => state.sessions)
  const status = useLuna((state) => state.status)
  const busy = useLuna((state) => state.busy)
  const starred = useLuna((state) => state.starred)
  const query = useLuna((state) => state.sessionQuery)
  const filter = useLuna((state) => state.sessionFilter)
  const newSession = useLuna((state) => state.newSession)
  const switchSession = useLuna((state) => state.switchSession)
  const renameSession = useLuna((state) => state.renameSession)
  const forkSession = useLuna((state) => state.forkSession)
  const deleteSession = useLuna((state) => state.deleteSession)
  const setQuery = useLuna((state) => state.setSessionQuery)
  const setFilter = useLuna((state) => state.setSessionFilter)
  const toggleStar = useLuna((state) => state.toggleStar)
  const openSettings = useLuna((state) => state.openSettings)
  const searchFocusSeq = useLuna((state) => state.searchFocusSeq)

  const searchRef = useRef<HTMLInputElement | null>(null)

  // Ctrl+K asks for the search box; select-all so typing replaces the query.
  useEffect(() => {
    if (searchFocusSeq === 0) return
    searchRef.current?.focus()
    searchRef.current?.select()
  }, [searchFocusSeq])

  const [editing, setEditing] = useState<string | null>(null)
  const [draft, setDraft] = useState('')
  const [pendingDelete, setPendingDelete] = useState<SessionSummaryDto | null>(null)
  const [menuFor, setMenuFor] = useState<string | null>(null)

  const activePath = status?.sessionFile ?? null
  const running = Boolean(status?.isStreaming)

  // Close the row menu on an outside click or Escape.
  useEffect(() => {
    if (!menuFor) return
    const onPointerDown = (): void => setMenuFor(null)
    const onKey = (event: KeyboardEvent): void => {
      if (event.key === 'Escape') setMenuFor(null)
    }
    window.addEventListener('pointerdown', onPointerDown)
    window.addEventListener('keydown', onKey)
    return () => {
      window.removeEventListener('pointerdown', onPointerDown)
      window.removeEventListener('keydown', onKey)
    }
  }, [menuFor])

  const visible = useMemo(() => {
    const needle = query.trim().toLowerCase()
    return sessions.filter((session) => {
      if (filter === 'starred' && !starred.includes(session.path)) return false
      if (filter === 'running' && !(session.path === activePath && running)) return false
      if (needle.length === 0) return true
      const haystack = `${session.name ?? ''} ${session.firstMessage ?? ''}`.toLowerCase()
      return haystack.includes(needle)
    })
  }, [sessions, query, filter, starred, activePath, running])

  const pinned = visible.filter((session) => starred.includes(session.path))
  const rest = visible.filter((session) => !starred.includes(session.path))
  const grouped = filter !== 'running'
  const groups = (
    [
      'Today',
      'Yesterday',
      'Previous 7 days',
      'Older'
    ] as Bucket[]
  )
    .map((label) => ({ label, items: rest.filter((session) => bucketOf(session.modified) === label) }))
    .filter((group) => group.items.length > 0)

  const commitRename = async (): Promise<void> => {
    const name = draft.trim()
    setEditing(null)
    if (name.length > 0) await renameSession(name)
  }

  const renderSession = (session: SessionSummaryDto) => {
    const active = session.path === activePath
    const isRunning = active && running
    const isStarred = starred.includes(session.path)
    const isFork = Boolean(session.parentSessionPath)
    const menuOpen = menuFor === session.path

    return (
      <div
        className={`srow${active ? ' on' : ''}`}
        key={session.path}
        role="button"
        tabIndex={0}
        onClick={() => {
          if (editing !== session.path) void switchSession(session.path)
        }}
        onKeyDown={(event) => {
          if (event.key === 'Enter') void switchSession(session.path)
        }}
      >
        {editing === session.path ? (
          <input
            className="srow__rename"
            value={draft}
            autoFocus
            placeholder="Session name"
            onClick={(event) => event.stopPropagation()}
            onChange={(event) => setDraft(event.target.value)}
            onBlur={() => void commitRename()}
            onKeyDown={(event) => {
              event.stopPropagation()
              if (event.key === 'Enter') void commitRename()
              if (event.key === 'Escape') setEditing(null)
            }}
          />
        ) : (
          <>
            <span className="srow__body">
              <span className="srow__title">{titleOf(session)}</span>
              <span className="srow__meta">
                {isRunning ? <i className="srow__dot" aria-label="Running" /> : null}
                {isFork ? <span className="srow__tag">fork</span> : null}
                <span>{stamp(session.modified)}</span>
              </span>
            </span>

            {isStarred ? <span className="srow__star" aria-hidden="true">★</span> : null}

            <button
              className="srow__more"
              aria-label="Session actions"
              title="Session actions"
              onClick={(event) => {
                event.stopPropagation()
                setMenuFor(menuOpen ? null : session.path)
              }}
            >
              ⋯
            </button>

            {menuOpen ? (
              <div className="smenu" onClick={(event) => event.stopPropagation()}>
                <button
                  onClick={() => {
                    setMenuFor(null)
                    toggleStar(session.path)
                  }}
                >
                  {isStarred ? 'Unstar' : 'Star'}
                </button>
                <button
                  onClick={() => {
                    setMenuFor(null)
                    setEditing(session.path)
                    setDraft(session.name ?? '')
                  }}
                >
                  Rename
                </button>
                <button
                  onClick={() => {
                    setMenuFor(null)
                    void forkSession()
                  }}
                >
                  Duplicate
                </button>
                <button
                  className="bad"
                  onClick={() => {
                    setMenuFor(null)
                    setPendingDelete(session)
                  }}
                >
                  Delete
                </button>
              </div>
            ) : null}
          </>
        )}
      </div>
    )
  }

  return (
    <aside className="side">
      <div className="side__brand">
        <span className="side__mark" aria-hidden="true">
          ☾
        </span>
        <span className="side__name">Luna</span>
      </div>

      <button className="side__new" onClick={() => void newSession()} disabled={busy}>
        <span aria-hidden="true">＋</span>
        <span>New session</span>
        <kbd>⌘N</kbd>
      </button>

      <label className="side__search">
        <span aria-hidden="true">⌕</span>
        <input
          ref={searchRef}
          value={query}
          placeholder="Search"
          aria-label="Search sessions"
          onChange={(event) => setQuery(event.target.value)}
        />
      </label>

      <div className="side__filters">
        <button className={filter === 'all' ? 'on' : ''} onClick={() => setFilter('all')}>
          All
        </button>
        <button className={filter === 'starred' ? 'on' : ''} onClick={() => setFilter('starred')}>
          Starred
        </button>
        <button className={filter === 'running' ? 'on' : ''} onClick={() => setFilter('running')}>
          Running
        </button>
      </div>

      <div className="side__list">
        {grouped && pinned.length > 0 ? (
          <div className="sgroup">
            <div className="sgroup__label">Starred</div>
            {pinned.map(renderSession)}
          </div>
        ) : null}

        {grouped
          ? groups.map((group) => (
              <div className="sgroup" key={group.label}>
                <div className="sgroup__label">{group.label}</div>
                {group.items.map(renderSession)}
              </div>
            ))
          : visible.map(renderSession)}

        {visible.length === 0 ? (
          <p className="side__empty">
            {sessions.length === 0 ? 'No sessions yet.' : 'Nothing matches.'}
          </p>
        ) : null}
      </div>

      <div className="side__foot">
        <button className="side__settings" onClick={openSettings} title="Settings">
          <span aria-hidden="true">⚙</span>
          <span>Settings</span>
        </button>
      </div>

      {pendingDelete ? (
        <div className="modal" role="dialog" aria-modal="true">
          <div className="modal__backdrop" onClick={() => setPendingDelete(null)} />
          <div className="modal__panel">
            <h2 className="modal__title">Delete session?</h2>
            <p className="modal__hint">
              <b>{titleOf(pendingDelete)}</b> and its transcript will be deleted from disk. This
              cannot be undone.
            </p>
            <div className="modal__actions">
              <button className="b" onClick={() => setPendingDelete(null)}>
                Cancel
              </button>
              <button
                className="b bad"
                onClick={() => {
                  const target = pendingDelete
                  setPendingDelete(null)
                  void deleteSession(target.path)
                }}
              >
                Delete
              </button>
            </div>
          </div>
        </div>
      ) : null}
    </aside>
  )
}
