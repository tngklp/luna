import { useLuna } from '../store'
import { ChangesView } from './ChangesView'
import { ChatView } from './ChatView'

/**
 * Main column: the chat/changes tabs, the run state, and the active view.
 *
 * Luna ships the chat surface and the change review, but not the editor or the
 * right-hand panel, so those tabs and the panel toggle are gone. Everything
 * else — the header, the tab styling, the Idle/Running chip — is piui's.
 */
export function MainPane() {
  const mainTab = useLuna((state) => state.mainTab)
  const setMainTab = useLuna((state) => state.setMainTab)
  const status = useLuna((state) => state.status)
  const changes = useLuna((state) => state.changes)
  const autoKeep = useLuna((state) => state.prefs.autoKeepEdits)

  const running = Boolean(status?.isStreaming)

  return (
    <main className="main">
      <div className="head">
        <div className="tabs main-tabs" role="tablist">
          <button
            className={`t${mainTab === 'chat' ? ' on' : ''}`}
            role="tab"
            aria-selected={mainTab === 'chat'}
            onClick={() => setMainTab('chat')}
          >
            Chat
          </button>
          {/* Hidden entirely under auto-keep: there is nothing to review, and an
              always-empty tab is a worse answer than no tab. */}
          {autoKeep ? null : (
            <button
              className={`t${mainTab === 'changes' ? ' on' : ''}`}
              role="tab"
              aria-selected={mainTab === 'changes'}
              onClick={() => setMainTab('changes')}
              title="Files the agent changed, waiting for review"
            >
              Changes
              {changes.length > 0 ? <span className="t__count">{changes.length}</span> : null}
            </button>
          )}
        </div>

        <span className="sp" />
        <span className="chip lil">{running ? 'Running' : 'Idle'}</span>
      </div>

      {mainTab === 'changes' ? <ChangesView /> : <ChatView />}
    </main>
  )
}
