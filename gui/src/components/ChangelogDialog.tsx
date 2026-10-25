import { useEffect } from 'react'
import { Markdown } from './Markdown'

/**
 * Release notes for an available update.
 *
 * Shown on the settings panel's footprint — wide, fixed height, its own scrolling
 * — rather than squeezed into the corner toast. The notes electron-updater hands
 * over are markdown, so they are rendered as markdown; the toast used to paste
 * them in after stripping a few characters, which left the headings and list
 * markers on screen as literal text.
 */
export function ChangelogDialog({
  version,
  notes,
  onClose
}: {
  version: string | null
  notes: string
  onClose: () => void
}) {
  useEffect(() => {
    const onKey = (event: KeyboardEvent): void => {
      if (event.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])

  return (
    <div className="modal" role="dialog" aria-modal="true" aria-label="Release notes">
      <div className="modal__backdrop" onClick={onClose} />
      <div className="modal__panel changelog">
        <div className="changelog__hd">
          <h2 className="modal__title">{version ? `What's new in v${version}` : "What's new"}</h2>
          <button className="ibtn" title="Close" aria-label="Close" onClick={onClose}>
            ✕
          </button>
        </div>

        <div className="changelog__body">
          <Markdown text={notes} />
        </div>
      </div>
    </div>
  )
}
