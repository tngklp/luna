import { useLuna } from '../../store'
import { Row, SettingsHeader, Switch } from './rows'

/** App and runtime defaults that are not tied to one panel. */
export function GeneralSettings() {
  const prefs = useLuna((state) => state.prefs)
  const setPrefs = useLuna((state) => state.setPrefs)
  const workspace = useLuna((state) => state.workspace)
  const runtime = useLuna((state) => state.runtime)

  return (
    <section className="set-section">
      <SettingsHeader title="General" subtitle="App and runtime defaults." />

      <h3>Agent</h3>
      <div className="gcard">
        <Row
          title="Expand reasoning by default"
          hint="Show the model's thinking blocks opened rather than collapsed."
        >
          <Switch
            label="Expand reasoning by default"
            checked={prefs.expandThinking}
            onChange={(expandThinking) => setPrefs({ expandThinking })}
          />
        </Row>

        <Row
          title="Expand tool output by default"
          hint="Open each tool card straight away, showing the command, diff, or output it produced."
        >
          <Switch
            label="Expand tool output by default"
            checked={prefs.expandToolOutput}
            onChange={(expandToolOutput) => setPrefs({ expandToolOutput })}
          />
        </Row>

        <Row
          title="Auto-keep all edits"
          hint="Accept every file the agent writes, without reviewing it first. Turn this off to get the Changes tab and the pending-changes bar above the prompt."
        >
          <Switch
            label="Auto-keep all edits"
            checked={prefs.autoKeepEdits}
            onChange={(autoKeepEdits) => setPrefs({ autoKeepEdits })}
          />
        </Row>

        <Row
          title="Send with Enter"
          hint={
            prefs.sendOnEnter
              ? 'Enter sends the message. Shift+Enter starts a new line.'
              : 'Enter starts a new line. Ctrl+Enter sends the message.'
          }
        >
          <Switch
            label="Send with Enter"
            checked={prefs.sendOnEnter}
            onChange={(sendOnEnter) => setPrefs({ sendOnEnter })}
          />
        </Row>
      </div>

      <h3>Workspace</h3>
      <div className="gcard">
        <Row title="Working directory" hint="Where the agent reads and writes files.">
          <span className="mono set-value">{workspace?.cwd ?? '—'}</span>
        </Row>
        <Row title="Engine" hint="The version of luna-core running this window.">
          <span className="mono set-value">{runtime?.sdkVersion ?? '—'}</span>
        </Row>
      </div>
    </section>
  )
}
