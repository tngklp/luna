import { useState } from 'react'
import { fileIconUrl } from '../lib/icon-packs'
import { useLuna } from '../store'

/** File name without its directory. */
function baseName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path
}

const ICONS = {
  keep: 'M3.5 8.5 6.5 11.5 12.5 5',
  undo: 'M4 7.5h5.2a3.3 3.3 0 0 1 0 6.6H6.5M4 7.5l2.6-2.6M4 7.5l2.6 2.6',
  open: 'M9.5 3h3.5v3.5M13 3 7.5 8.5M11.5 9.6V12a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V5.5a1 1 0 0 1 1-1h2.4'
} as const

function Icon({ path }: { path: string }) {
  return (
    <svg
      width="13"
      height="13"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d={path} />
    </svg>
  )
}

/**
 * What the agent has changed, sitting directly above the prompt.
 *
 * The agent writes files without asking, so the review is offered where the next
 * instruction is typed rather than only in the Changes tab — otherwise the first
 * thing a user notices is that a file they did not expect has moved. Collapsed by
 * default: the summary is the part worth interrupting for, the file list is not.
 *
 * Keep accepts and undoes the diff below it: the baseline is captured the first
 * time the agent touches a file, so Undo returns it to the state before the agent
 * started rather than to the previous edit.
 */
export function ChangesBar() {
  const changes = useLuna((state) => state.changes)
  const autoKeep = useLuna((state) => state.prefs.autoKeepEdits)
  const keepChanges = useLuna((state) => state.keepChanges)
  const undoChanges = useLuna((state) => state.undoChanges)
  const openFile = useLuna((state) => state.openFile)
  const setMainTab = useLuna((state) => state.setMainTab)
  const iconPack = useLuna((state) => state.prefs.iconPack)
  const [open, setOpen] = useState(false)

  // With auto-keep on the list is always empty, and the bar is not what the user
  // asked for, so it is not rendered at all rather than rendered empty.
  if (autoKeep || changes.length === 0) return null

  const added = changes.reduce((total, change) => total + change.added, 0)
  const removed = changes.reduce((total, change) => total + change.removed, 0)
  const summary = changes.length === 1 ? '1 file changed' : `${changes.length} files changed`

  return (
    <div className="cbar">
      <div className="cbar__hd">
        <button
          className="cbar__toggle"
          aria-expanded={open}
          title={open ? 'Collapse' : 'Expand'}
          onClick={() => setOpen((was) => !was)}
        >
          <span className={`chev${open ? ' o' : ''}`}>›</span>
        </button>

        <b className="cbar__sum">{summary}</b>
        <span className="ok">+{added}</span>
        <span className="del">−{removed}</span>

        <span className="sp" />

        <button className="b xs pri" onClick={() => void keepChanges(null)}>
          Keep
        </button>
        <button className="b xs" onClick={() => void undoChanges(null)}>
          Undo
        </button>
        <button
          className="ibtn sm"
          title="Open the Changes tab"
          aria-label="Open the Changes tab"
          onClick={() => setMainTab('changes')}
        >
          <Icon path={ICONS.open} />
        </button>
      </div>

      {open ? (
        <div className="cbar__list">
          {changes.map((change) => (
            <div className="cbar__row" key={change.path}>
              <button
                className="cbar__path"
                title={change.path}
                onClick={() => openFile(change.path)}
              >
                <img className="fd" src={fileIconUrl(iconPack, baseName(change.path))} alt="" />
                <span>{change.relative}</span>
              </button>
              {change.created ? <span className="tag">new</span> : null}

              <span className="cbar__num ok">+{change.added}</span>
              <span className="cbar__num del">−{change.removed}</span>

              <button
                className="ibtn sm"
                title={`Keep changes to ${change.relative}`}
                aria-label={`Keep changes to ${change.relative}`}
                onClick={() => void keepChanges(change.path)}
              >
                <Icon path={ICONS.keep} />
              </button>
              <button
                className="ibtn sm"
                title={`Undo changes to ${change.relative}`}
                aria-label={`Undo changes to ${change.relative}`}
                onClick={() => void undoChanges(change.path)}
              >
                <Icon path={ICONS.undo} />
              </button>
            </div>
          ))}
        </div>
      ) : null}
    </div>
  )
}
