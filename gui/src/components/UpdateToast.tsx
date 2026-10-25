import { useState } from 'react'
import { useLuna } from '../store'
import { ChangelogDialog } from './ChangelogDialog'

/**
 * Update prompt.
 *
 * Sits in the corner once the release feed reports a newer version, and carries
 * the whole flow: download, then restart to install. Portable and development
 * builds can never get past the "unsupported" state, so they show nothing.
 *
 * The release notes are not shown here. They are markdown, and there is nowhere
 * in a 360px toast to render them properly, so a button opens them in a panel the
 * size of the settings dialog instead.
 */
export function UpdateToast() {
  const update = useLuna((state) => state.update)
  const download = useLuna((state) => state.downloadUpdate)
  const install = useLuna((state) => state.installUpdate)

  const [hidden, setHidden] = useState(false)
  const [showNotes, setShowNotes] = useState(false)

  if (!update || hidden) return null
  if (update.phase !== 'available' && update.phase !== 'downloading' && update.phase !== 'ready') {
    return null
  }

  return (
    <aside className="upd" role="status" aria-live="polite">
      <div className="upd__hd">
        <b>
          {update.phase === 'ready' ? 'Update ready' : 'Update available'}
          {update.version ? <span className="upd__ver">v{update.version}</span> : null}
        </b>
        <button
          className="ibtn"
          title="Dismiss"
          aria-label="Dismiss"
          onClick={() => setHidden(true)}
        >
          ✕
        </button>
      </div>

      <p className="upd__body">
        {update.phase === 'downloading'
          ? `Downloading… ${update.percent ?? 0}%`
          : update.phase === 'ready'
            ? 'Restarting installs it silently and reopens Luna. No setup steps.'
            : `You are on v${update.currentVersion}. It downloads on its own once offered.`}
      </p>

      {update.phase === 'downloading' ? (
        <span className="upd__bar">
          <i style={{ width: `${Math.min(100, Math.max(0, update.percent ?? 0))}%` }} />
        </span>
      ) : null}

      <div className="upd__btns">
        {update.notes ? (
          <button className="b sm" onClick={() => setShowNotes(true)}>
            Changelog
          </button>
        ) : null}
        {update.phase === 'available' ? (
          <button className="b pri sm" onClick={() => void download()}>
            Download now
          </button>
        ) : null}
        {update.phase === 'downloading' ? (
          <button className="b sm" disabled>
            Downloading…
          </button>
        ) : null}
        {update.phase === 'ready' ? (
          <button className="b pri sm" onClick={install}>
            Restart and update
          </button>
        ) : null}
      </div>

      {showNotes && update.notes ? (
        <ChangelogDialog
          version={update.version}
          notes={update.notes}
          onClose={() => setShowNotes(false)}
        />
      ) : null}
    </aside>
  )
}
