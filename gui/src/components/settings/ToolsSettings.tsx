import type { ApprovalAction } from '@shared/ipc'
import { useLuna } from '../../store'
import { Select, type SelectOption } from '../Select'
import { Row, SettingsHeader } from './rows'

const ACTION_OPTIONS: SelectOption<ApprovalAction>[] = [
  { value: 'allow', label: 'Allow' },
  { value: 'ask', label: 'Ask' },
  { value: 'deny', label: 'Deny' }
]

/** One-line explanation of each policy, shown under the row title. */
const ACTION_HINTS: Record<ApprovalAction, string> = {
  allow: 'Runs immediately, without asking.',
  ask: 'Pauses the agent and shows an approval card in the transcript.',
  deny: 'Blocked outright; the agent is told it is not allowed.'
}

/**
 * Approval policy, per tool class.
 *
 * Luna groups tools by what they can do rather than listing each one: a rule for
 * `write` covers `write`, `edit` and `multi_edit`, which is the granularity a
 * user actually reasons about.
 */
const CLASSES: { key: 'read' | 'write' | 'bash' | 'network'; title: string; hint: string }[] = [
  {
    key: 'read',
    title: 'Read files',
    hint: 'read, ls, glob, grep — anything that only looks at the workspace.'
  },
  {
    key: 'write',
    title: 'Change files',
    hint: 'write, edit, multi_edit — anything that modifies the workspace.'
  },
  {
    key: 'bash',
    title: 'Run commands',
    hint: 'bash — shell commands, builds and tests.'
  },
  {
    key: 'network',
    title: 'Reach the network',
    hint: 'Tools that fetch from the internet.'
  }
]

/** Per-class approval policy. Every change is saved immediately. */
export function ToolsSettings() {
  const config = useLuna((state) => state.approvalConfig)
  const save = useLuna((state) => state.saveApprovalConfig)

  const policyOf = (key: string): ApprovalAction => {
    const tools = config?.tools ?? {}
    return tools[key] ?? config?.defaultPolicy ?? 'ask'
  }

  const setClass = (key: string, action: ApprovalAction): void => {
    if (!config) return
    void save({ ...config, tools: { ...config.tools, [key]: action } })
  }

  const setDefault = (action: ApprovalAction): void => {
    if (config) void save({ ...config, defaultPolicy: action })
  }

  const defaultPolicy = config?.defaultPolicy ?? 'ask'

  return (
    <section className="set-section">
      <SettingsHeader
        title="Tools"
        subtitle="Decide what Luna does before the agent runs a tool."
      />

      <h3>Everything else</h3>
      <div className="gcard">
        <Row
          title="Default policy"
          hint={`Applied to any tool without its own rule below. ${ACTION_HINTS[defaultPolicy]}`}
        >
          <Select
            value={defaultPolicy}
            options={ACTION_OPTIONS}
            title="Default approval for unlisted tools"
            onChange={setDefault}
          />
        </Row>
      </div>

      <h3>Per class</h3>
      <div className="gcard">
        {CLASSES.map((entry) => {
          const policy = policyOf(entry.key)
          return (
            <Row key={entry.key} title={entry.title} hint={`${entry.hint} ${ACTION_HINTS[policy]}`}>
              <Select
                value={policy}
                options={ACTION_OPTIONS}
                title={`${entry.title} approval`}
                onChange={(action) => setClass(entry.key, action)}
              />
            </Row>
          )
        })}
      </div>

      <p className="set-note">
        Reading is safe to allow. Leave the classes that run commands or change files on <b>Ask</b>{' '}
        so you can review them first.
      </p>
    </section>
  )
}
