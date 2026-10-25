import { useEffect, useState } from 'react'
import { useLuna } from '../store'

/**
 * Renders extension-UI dialogs and sends the answer back to the main process.
 *
 * Approvals are deliberately excluded: they are answered inside the transcript
 * instead, so the user keeps the context of what the agent is doing.
 */
export function DialogHost() {
  const dialog = useLuna((state) => state.dialog)
  const respond = useLuna((state) => state.respondToDialog)
  const [text, setText] = useState('')

  useEffect(() => {
    if (dialog?.method === 'editor') {
      setText(dialog.prefill ?? '')
    } else {
      setText('')
    }
  }, [dialog])

  if (!dialog || dialog.method === 'approval') return null

  const cancel = (): void => void respond({ id: dialog.id, cancelled: true })

  return (
    <div className="modal" role="dialog" aria-modal="true">
      <div className="modal__backdrop" onClick={cancel} />
      <div className="modal__panel">
        <h2 className="modal__title">{dialog.title}</h2>

        {dialog.method === 'confirm' ? (
          <pre className="modal__message">{dialog.message}</pre>
        ) : null}

        {dialog.method === 'select' ? (
          <div className="modal__options">
            {dialog.options.map((option, index) => (
              <button
                className="modal__option"
                key={option}
                autoFocus={index === 0}
                onClick={() => void respond({ id: dialog.id, value: option })}
              >
                {option}
              </button>
            ))}
          </div>
        ) : null}

        {dialog.method === 'input' ? (
          <input
            className="modal__input"
            value={text}
            placeholder={dialog.placeholder ?? ''}
            autoFocus
            onChange={(event) => setText(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Enter') void respond({ id: dialog.id, value: text })
              if (event.key === 'Escape') cancel()
            }}
          />
        ) : null}

        {dialog.method === 'editor' ? (
          <textarea
            className="modal__input mono multiline"
            value={text}
            autoFocus
            onChange={(event) => setText(event.target.value)}
          />
        ) : null}

        {dialog.method === 'select' ? null : (
          <div className="modal__actions">
            <button className="b" onClick={cancel}>
              Cancel
            </button>
            {dialog.method === 'input' || dialog.method === 'editor' ? (
              <button
                className="b pri"
                onClick={() => void respond({ id: dialog.id, value: text })}
              >
                OK
              </button>
            ) : null}
          </div>
        )}
      </div>
    </div>
  )
}
