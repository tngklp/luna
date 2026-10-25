import { useEffect, useMemo, useRef, useState } from 'react'
import type { SessionSummaryDto } from '@shared/ipc'
import { useLuna } from '../store'

type Bucket = 'Today' | 'Yesterday' | 'Earlier'

function startOfToday(): number {
  const now = new Date()
  return new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime()
}

function bucketOf(iso: string): Bucket {
  const then = new Date(iso).getTime()
  const today = startOfToday()
  if (then >= today) return 'Today'
  if (then >= today - 86_400_000) return 'Yesterday'
  return 'Earlier'
}

function relative(iso: string): string {
  const minutes = Math.floor((Date.now() - new Date(iso).getTime()) / 60_000)
  if (minutes < 1) return 'just now'
  if (minutes < 60) return `${minutes} min ago`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours} h ago`
  if (hours < 48) return 'Yesterday'
  return new Date(iso).toLocaleDateString()
}

/** Workspace switcher, session search/filter, and the grouped session list. */
export function Sidebar() {
  const workspace = useLuna((state) => state.workspace)
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
  const changeWorkspace = useLuna((state) => state.changeWorkspace)
  const setQuery = useLuna((state) => state.setSessionQuery)
  const setFilter = useLuna((state) => state.setSessionFilter)
  const toggleStar = useLuna((state) => state.toggleStar)
  const openSettings = useLuna((state) => state.openSettings)
  const openWelcome = useLuna((state) => state.openWelcome)
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

  const activePath = status?.sessionFile ?? null
  const running = Boolean(status?.isStreaming)

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
  // The Running view is a short, live list; date groups only make sense for the
  // full history.
  const grouped = filter !== 'running'
  const groups = [
    { label: 'Today', items: rest.filter((session) => bucketOf(session.modified) === 'Today') },
    {
      label: 'Yesterday',
      items: rest.filter((session) => bucketOf(session.modified) === 'Yesterday')
    },
    { label: 'Earlier', items: rest.filter((session) => bucketOf(session.modified) === 'Earlier') }
  ].filter((group) => group.items.length > 0)

  const commitRename = async (): Promise<void> => {
    const name = draft.trim()
    setEditing(null)
    if (name.length > 0) await renameSession(name)
  }

  const renderSession = (session: SessionSummaryDto) => {
    const active = session.path === activePath
    const title = session.name ?? session.firstMessage ?? 'Untitled session'
    const isRunning = active && running
    const isStarred = starred.includes(session.path)
    const isFork = Boolean(session.parentSessionPath)

    return (
      <div
        className={`sess${active ? ' on' : ''}${isFork ? ' fork' : ''}`}
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
          <span className="tx">
            <input
              className="rename"
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
          </span>
        ) : (
          <span className="tx">
            <b>{title}</b>
            <small>
              {isRunning ? 'Running · ' : isFork ? 'Fork · ' : ''}
              {relative(session.modified)} · {session.messageCount} messages
            </small>
          </span>
        )}

        {editing !== session.path ? (
          <span className="acts">
            <button
              title={isStarred ? 'Unstar' : 'Star'}
              aria-label={isStarred ? 'Unstar session' : 'Star session'}
              onClick={(event) => {
                event.stopPropagation()
                toggleStar(session.path)
              }}
            >
              {isStarred ? '★' : '☆'}
            </button>
            <button
              title="Rename session"
              aria-label="Rename session"
              onClick={(event) => {
                event.stopPropagation()
                setEditing(session.path)
                setDraft(session.name ?? '')
              }}
            >
              ✎
            </button>
            <button
              title="Duplicate session"
              aria-label="Duplicate session"
              onClick={(event) => {
                event.stopPropagation()
                void forkSession()
              }}
            >
              ⑂
            </button>
            <button
              title="Delete session"
              aria-label="Delete session"
              onClick={(event) => {
                event.stopPropagation()
                setPendingDelete(session)
              }}
            >
              <svg
                width="13"
                height="13"
                viewBox="0 0 16 16"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.4"
              >
                <path d="M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5l.6 8.2h5.8l.6-8.2" />
              </svg>
            </button>
          </span>
        ) : null}
      </div>
    )
  }

  return (
    <aside className="side">
      <button className="proj" onClick={() => void changeWorkspace()} title={workspace?.cwd ?? ''}>
        <span className="tx">
          <b>{workspace?.name ?? 'No workspace'}</b>
          <small>{sessions.length} saved sessions</small>
        </span>
      </button>

      <button className="newbtn" onClick={() => void newSession()} disabled={busy}>
        <span>＋</span>
        <span>New session</span>
        <kbd>Ctrl N</kbd>
      </button>

      <label className="sbox">
        <span aria-hidden="true">⌕</span>
        <input
          ref={searchRef}
          value={query}
          placeholder="Search sessions"
          aria-label="Search sessions"
          onChange={(event) => setQuery(event.target.value)}
        />
        <kbd>Ctrl K</kbd>
      </label>

      <div className="filters">
        <button className={filter === 'all' ? 'on' : ''} onClick={() => setFilter('all')}>
          All
        </button>
        <button className={filter === 'running' ? 'on' : ''} onClick={() => setFilter('running')}>
          Running
        </button>
        <button className={filter === 'starred' ? 'on' : ''} onClick={() => setFilter('starred')}>
          Starred
        </button>
      </div>

      <div className="slist">
        {grouped && pinned.length > 0 ? (
          <details className="g" open>
            <summary>Pinned</summary>
            {pinned.map(renderSession)}
          </details>
        ) : null}

        {grouped
          ? groups.map((group) => (
              <details className="g" open key={group.label}>
                <summary>{group.label}</summary>
                {group.items.map(renderSession)}
              </details>
            ))
          : visible.map(renderSession)}

        {visible.length === 0 ? (
          <p className="empty" style={{ padding: '20px 10px', fontSize: 13 }}>
            {sessions.length === 0 ? 'No saved sessions yet.' : 'No sessions match.'}
          </p>
        ) : null}
      </div>

      <div className="sidefoot">
        <button className="sidefoot__btn" onClick={openWelcome} title="Home" aria-label="Home">
          <svg
            width="16"
            height="16"
            viewBox="0 0 16 16"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.5"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <path d="M2.5 6.5 8 2l5.5 4.5V13a1 1 0 0 1-1 1h-9a1 1 0 0 1-1-1z" />
            <path d="M6.5 14V9.5h3V14" />
          </svg>
        </button>
        <button className="sidefoot__btn grow" onClick={openSettings} title="Settings">
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
              <b>{pendingDelete.name ?? pendingDelete.firstMessage ?? 'Untitled session'}</b> and
              its transcript will be deleted from disk. This cannot be undone.
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
