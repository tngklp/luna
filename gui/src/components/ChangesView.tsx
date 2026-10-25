import { useEffect } from 'react'
import { fileIconUrl } from '../lib/icon-packs'
import { useLuna } from '../store'
import { DiffView } from './DiffView'

/** File name without its directory. */
function baseName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path
}

/**
 * Review list for what the agent has written.
 *
 * Keep accepts a change: it stays on disk and leaves the list, which is the usual
 * outcome and why the button is the prominent one. Undo puts the file back the way
 * it was before the agent first touched it — the baseline is captured per file, not
 * per edit, so undoing after several edits still returns to a state the user
 * recognises.
 */
export function ChangesView() {
  const changes = useLuna((state) => state.changes)
  const loadChanges = useLuna((state) => state.loadChanges)
  const keepChanges = useLuna((state) => state.keepChanges)
  const undoChanges = useLuna((state) => state.undoChanges)
  const openFile = useLuna((state) => state.openFile)
  const iconPack = useLuna((state) => state.prefs.iconPack)

  useEffect(() => {
    void loadChanges()
  }, [loadChanges])

  const added = changes.reduce((total, change) => total + change.added, 0)
  const removed = changes.reduce((total, change) => total + change.removed, 0)
  const summary = changes.length === 1 ? '1 file changed' : `${changes.length} files changed`

  return (
    <div className="changes">
      <div className="changes__bar">
        <b>{changes.length === 0 ? 'No changes yet' : summary}</b>
        {changes.length > 0 ? (
          <>
            <span className="ok">+{added}</span>
            <span className="del">−{removed}</span>
          </>
        ) : null}

        <span className="sp" />

        {changes.length > 0 ? (
          <>
            <button className="b sm" onClick={() => void undoChanges(null)}>
              Undo all
            </button>
            <button className="b sm pri" onClick={() => void keepChanges(null)}>
              Keep all
            </button>
          </>
        ) : null}
      </div>

      <div className="changes__list">
        {changes.length === 0 ? (
          <p className="hint">
            Nothing to review. A file the agent writes shows up here until you keep or undo it.
          </p>
        ) : null}

        {changes.map((change) => (
          <div className="changes__item" key={change.path}>
            <div className="changes__hd">
              <button
                className="changes__path"
                title={change.path}
                onClick={() => openFile(change.path)}
              >
                <img className="fd" src={fileIconUrl(iconPack, baseName(change.path))} alt="" />
                {change.relative}
              </button>
              {change.created ? <span className="tag">new</span> : null}
              <span className="ok">+{change.added}</span>
              <span className="del">−{change.removed}</span>

              <span className="sp" />

              <button className="b sm" onClick={() => void undoChanges(change.path)}>
                Undo
              </button>
              <button className="b sm pri" onClick={() => void keepChanges(change.path)}>
                Keep
              </button>
            </div>

            {change.diff ? <DiffView diff={change.diff} /> : null}
          </div>
        ))}
      </div>
    </div>
  )
}
