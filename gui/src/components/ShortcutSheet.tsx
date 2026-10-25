import { useEffect } from 'react'
import { useLuna } from '../store'

/**
 * The keyboard shortcut sheet.
 *
 * `?` opens it. Every shortcut the app claims is listed here, so the sheet is
 * the single place to check rather than a comment somewhere in the code.
 */

interface Shortcut {
  keys: string
  action: string
}

interface Group {
  title: string
  items: Shortcut[]
}

const GROUPS: Group[] = [
  {
    title: 'Session',
    items: [
      { keys: 'Ctrl N', action: 'New session' },
      { keys: 'Ctrl K', action: 'Search sessions' },
      { keys: 'Ctrl P', action: 'Command palette' },
      { keys: 'Ctrl B', action: 'Toggle the sidebar' }
    ]
  },
  {
    title: 'Composer',
    items: [
      { keys: 'Enter', action: 'Send' },
      { keys: 'Shift Enter', action: 'New line' },
      { keys: 'Ctrl Enter', action: 'Send while streaming' },
      { keys: '/', action: 'Open the command list' },
      { keys: 'Esc', action: 'Stop the running turn' }
    ]
  },
  {
    title: 'View',
    items: [
      { keys: 'Ctrl `', action: 'Toggle the terminal' },
      { keys: '?', action: 'This sheet' }
    ]
  }
]

export function ShortcutSheet() {
  const open = useLuna((state) => state.shortcutsOpen)
  const close = useLuna((state) => state.closeShortcuts)

  useEffect(() => {
    if (!open) return
    const onKey = (event: KeyboardEvent): void => {
      if (event.key === 'Escape') close()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [open, close])

  if (!open) return null

  return (
    <div className="modal" role="dialog" aria-modal="true" aria-label="Keyboard shortcuts">
      <div className="modal__backdrop" onClick={close} />
      <div className="modal__panel wide shortcuts">
        <h2 className="modal__title">Keyboard shortcuts</h2>
        <div className="shortcuts__grid">
          {GROUPS.map((group) => (
            <section key={group.title}>
              <h3>{group.title}</h3>
              {group.items.map((item) => (
                <div className="shortcuts__row" key={item.keys}>
                  <kbd>{item.keys}</kbd>
                  <span>{item.action}</span>
                </div>
              ))}
            </section>
          ))}
        </div>
        <div className="modal__actions">
          <button className="b" onClick={close}>
            Close
          </button>
        </div>
      </div>
    </div>
  )
}
