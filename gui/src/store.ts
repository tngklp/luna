import { create } from 'zustand'
import type {
  AgentEventDto,
  AppInfo,
  ApprovalConfig,
  ChatItemDto,
  CommandDto,
  ModelDto,
  ModelsConfigDto,
  NoticeDto,
  PendingChangeDto,
  RuntimeInfoDto,
  SessionStatusDto,
  SessionSummaryDto,
  ThinkingLevelDto,
  UiRequestDto,
  UiResponseDto,
  UpdateStateDto,
  WorkspaceDto
} from '@shared/ipc'
import { loadThemeId, saveThemeId } from './theme/preference'
import { loadUiPreferences, saveUiPreferences, type UiPreferences } from './lib/ui-prefs'

/**
 * Luna's renderer store.
 *
 * This is the piui store with the panes Luna does not ship removed: there is no
 * editor, no file explorer and no right panel, so the state that only served
 * them is gone. Everything the chat surface reads — items, streaming,
 * runningTools, status, notices, dialogs — keeps piui's exact shape, which is
 * why the ported components need no changes.
 */

export type MainTab = 'chat' | 'changes'
export type SettingsTab = 'general' | 'models' | 'customization' | 'tools'
export type SessionFilter = 'all' | 'running' | 'starred'

interface StreamingState {
  text: string
  thinking: string
  /** True while thinking deltas are still arriving. */
  thinkingLive: boolean
  tools: { id: string; name: string; argsText: string }[]
}

/** A tool currently executing, with whatever output has streamed in so far. */
export interface RunningTool {
  id: string
  name: string
  /** Arguments from the call, so the card can show the command while it runs. */
  args?: unknown
  /** Output received from `tool_execution_update` so far. */
  text?: string
}

/** Event payload shapes the renderer reads out of the opaque agent event stream. */
interface AssistantUpdate {
  type: string
  delta?: string
  content?: string
  id?: string
  toolName?: string
  toolCall?: { id?: string; name?: string; arguments?: unknown }
  reason?: string
  /** Index into the partial message's content blocks. */
  contentIndex?: number
  /** The message as it stands, used to read a block the event does not carry. */
  partial?: { content?: unknown[] }
}

/**
 * The tool call a streaming event belongs to.
 *
 * `toolcall_start` carries only `contentIndex` and the partial message — not the
 * id or the name — so taking them from the event itself left every streamed call
 * anonymous and id-less. An id-less call is never matched against the running
 * card, which is why the block is read out of the partial message instead.
 */
function toolCallAt(update: AssistantUpdate): { id?: string; name?: string } | null {
  const content = update.partial?.content
  if (!Array.isArray(content)) return null
  const block = content[update.contentIndex ?? 0] as
    | { type?: string; id?: string; name?: string }
    | undefined
  if (!block || block.type !== 'toolCall') return null
  return { id: block.id, name: block.name }
}

/**
 * Text out of a partial tool result.
 *
 * The core streams `{ content: [{ type: 'text', text }] }`, the same shape the
 * finished result uses, so this mirrors how the host maps a result into a chat
 * item. `args` is read defensively because the field is typed `any` on the
 * event side.
 */
function partialToolText(value: unknown): string {
  if (typeof value === 'string') return value
  if (!value || typeof value !== 'object') return ''

  const content = (value as { content?: unknown }).content
  if (typeof content === 'string') return content
  if (!Array.isArray(content)) return ''

  return content
    .filter((block) => (block as { type?: string }).type === 'text')
    .map((block) => (block as { text?: string }).text ?? '')
    .join('')
}

interface LunaState {
  initialized: boolean
  error: string | null
  runtime: RuntimeInfoDto | null
  /** Static application information, including the version. */
  appInfo: AppInfo | null
  status: SessionStatusDto | null
  models: ModelDto[]
  /** Slash commands discovered from skills and prompt templates. */
  commands: CommandDto[]
  items: ChatItemDto[]
  streaming: StreamingState | null
  runningTools: RunningTool[]
  /** Files the agent changed and nobody has reviewed yet. */
  changes: PendingChangeDto[]
  /** The revision the change list was read at, to notice when it moves. */
  changesRevision: number
  /**
   * A sent message the user is rewriting. The composer takes its text, and the
   * next send replaces the message instead of appending a new turn.
   */
  editing: { id: string; entryId: string; text: string } | null
  notices: (NoticeDto & { id: number })[]
  busy: boolean
  /** A dialog the agent is waiting on, if any. */
  dialog: UiRequestDto | null
  /** Current tool-approval policy. */
  approvalConfig: ApprovalConfig | null
  /** Provider/model catalogue from `models.json`. */
  modelsConfig: ModelsConfigDto | null
  /** Why `models.json` could not be used, if it could not. */
  modelsError: string | null
  /** Self-update state, or null before it has been read. */
  update: UpdateStateDto | null
  /** Increments whenever a global shortcut asks for the session search box. */
  searchFocusSeq: number
  /** Saved sessions for the current workspace, newest first. */
  sessions: SessionSummaryDto[]
  /** The working directory the agent is operating on. */
  workspace: WorkspaceDto | null
  /** Saved sessions across every workspace, for the recents list. */
  allSessions: SessionSummaryDto[]
  /** Session paths the user starred, persisted locally. */
  starred: string[]
  sessionQuery: string
  sessionFilter: SessionFilter
  /** Theme id from the theme registry. */
  themeId: string
  /** Persisted interface preferences. */
  prefs: UiPreferences
  /** Which main column view is showing. */
  mainTab: MainTab
  /** Whether the settings dialog is open. */
  settingsOpen: boolean
  /** Which settings section is showing. */
  settingsTab: SettingsTab

  initialize: () => Promise<void>
  refresh: () => Promise<void>
  ingest: (event: AgentEventDto) => void
  dismissNotice: (id: number) => void
  respondToDialog: (response: UiResponseDto) => Promise<void>
  focusSearch: () => void
  saveApprovalConfig: (config: ApprovalConfig) => Promise<void>
  loadModelsConfig: () => Promise<void>
  saveModelsConfig: (config: ModelsConfigDto) => Promise<void>
  checkForUpdates: () => Promise<void>
  downloadUpdate: () => Promise<void>
  installUpdate: () => void
  loadSessions: () => Promise<void>
  loadCommands: () => Promise<void>
  switchSession: (path: string) => Promise<void>
  renameSession: (name: string) => Promise<void>
  forkSession: () => Promise<void>
  changeWorkspace: () => Promise<void>
  openWorkspace: (path: string) => Promise<void>
  setTheme: (id: string) => void
  setPrefs: (patch: Partial<UiPreferences>) => void
  openSettings: () => void
  closeSettings: () => void
  setSettingsTab: (tab: SettingsTab) => void
  /** Open a changed file in the operating system's editor. */
  openFile: (path: string) => void
  setSessionQuery: (query: string) => void
  setSessionFilter: (filter: SessionFilter) => void
  toggleStar: (path: string) => void
  loadAllSessions: () => Promise<void>
  setMainTab: (tab: MainTab) => void
  loadChanges: () => Promise<void>
  keepChanges: (path: string | null) => Promise<void>
  undoChanges: (path: string | null) => Promise<void>
  deleteSession: (path: string) => Promise<void>
  send: (
    text: string,
    mode?: 'prompt' | 'steer' | 'followUp',
    images?: { type: 'image'; data: string; mimeType: string }[]
  ) => Promise<void>
  beginEdit: (item: { id: string; entryId: string; text: string }) => void
  cancelEdit: () => void
  retryMessage: (entryId: string) => Promise<void>
  abort: () => Promise<void>
  newSession: () => Promise<void>
  compact: () => Promise<void>
  selectModel: (provider: string, id: string) => Promise<void>
  selectThinking: (level: ThinkingLevelDto) => Promise<void>
}

let noticeId = 0

function emptyStreaming(): StreamingState {
  return { text: '', thinking: '', thinkingLive: false, tools: [] }
}

function describeError(cause: unknown): string {
  if (cause instanceof Error) return cause.message
  return String(cause)
}

const STARRED_KEY = 'luna.starred'

function loadStarred(): string[] {
  try {
    const raw = localStorage.getItem(STARRED_KEY)
    if (!raw) return []
    const parsed = JSON.parse(raw)
    return Array.isArray(parsed) ? parsed.filter((value) => typeof value === 'string') : []
  } catch {
    return []
  }
}

function saveStarred(paths: string[]): void {
  try {
    localStorage.setItem(STARRED_KEY, JSON.stringify(paths))
  } catch {
    // Storage can be unavailable; starring is a convenience, not a requirement.
  }
}

export const useLuna = create<LunaState>()((set, get) => ({
  initialized: false,
  error: null,
  runtime: null,
  appInfo: null,
  status: null,
  models: [],
  commands: [],
  items: [],
  streaming: null,
  runningTools: [],
  changes: [],
  changesRevision: 0,
  editing: null,
  notices: [],
  busy: false,
  dialog: null,
  approvalConfig: null,
  modelsConfig: null,
  modelsError: null,
  update: null,
  searchFocusSeq: 0,
  sessions: [],
  workspace: null,
  allSessions: [],
  starred: loadStarred(),
  sessionQuery: '',
  sessionFilter: 'all',
  themeId: loadThemeId(),
  prefs: loadUiPreferences(),
  mainTab: 'chat',
  settingsOpen: false,
  settingsTab: 'general',

  initialize: async () => {
    if (get().initialized) return
    window.luna.onAgentEvent((event) => get().ingest(event))
    window.luna.onNotice((notice) => {
      noticeId += 1
      set((state) => ({ notices: [...state.notices, { ...notice, id: noticeId }].slice(-4) }))
    })
    window.luna.onUiRequest((request) => set({ dialog: request }))
    window.luna.onUpdate((update) => set({ update }))
    set({ initialized: true })

    try {
      const [
        appInfo,
        runtime,
        status,
        messages,
        models,
        approvalConfig,
        workspace,
        sessions,
        commands
      ] = await Promise.all([
        window.luna.getAppInfo(),
        window.luna.getRuntimeInfo(),
        window.luna.getStatus(),
        window.luna.getMessages(),
        window.luna.getModels(),
        window.luna.getApprovalConfig(),
        window.luna.getWorkspace(),
        window.luna.listSessions(),
        window.luna.getCommands()
      ])
      set({
        appInfo,
        runtime,
        status,
        items: messages,
        models,
        approvalConfig,
        workspace,
        sessions,
        commands,
        error: null
      })
      void get().loadAllSessions()
      void get().loadModelsConfig()
      void window.luna
        .getUpdateState()
        .then((update) => set({ update }))
        .catch(() => undefined)
    } catch (cause) {
      set({ error: describeError(cause) })
    }
  },

  refresh: async () => {
    try {
      const [status, messages] = await Promise.all([
        window.luna.getStatus(),
        window.luna.getMessages()
      ])
      set({ status, items: messages })
    } catch (cause) {
      set({ error: describeError(cause) })
    }
  },

  ingest: (event) => {
    switch (event.type) {
      case 'message_start': {
        const message = event.message as { role?: string } | undefined
        if (message?.role === 'assistant') set({ streaming: emptyStreaming() })
        break
      }

      case 'message_update': {
        const update = event.assistantMessageEvent as AssistantUpdate | undefined
        if (!update) break
        set((state) => {
          const stream = state.streaming ?? emptyStreaming()
          switch (update.type) {
            case 'text_delta':
              return { streaming: { ...stream, text: stream.text + (update.delta ?? '') } }
            case 'text_end':
              return { streaming: { ...stream, text: update.content ?? stream.text } }
            case 'thinking_delta':
              return {
                streaming: {
                  ...stream,
                  thinking: stream.thinking + (update.delta ?? ''),
                  thinkingLive: true
                }
              }
            case 'thinking_end':
              return {
                streaming: {
                  ...stream,
                  thinking: update.content ?? stream.thinking,
                  thinkingLive: false
                }
              }
            case 'toolcall_start': {
              const call = toolCallAt(update)
              return {
                streaming: {
                  ...stream,
                  // Reaching a tool call means reasoning for this turn is done.
                  thinkingLive: false,
                  tools: [
                    ...stream.tools,
                    { id: call?.id ?? '', name: call?.name ?? 'tool', argsText: '' }
                  ]
                }
              }
            }
            case 'toolcall_delta': {
              const tools = stream.tools.map((tool, index) =>
                index === stream.tools.length - 1
                  ? { ...tool, argsText: tool.argsText + (update.delta ?? '') }
                  : tool
              )
              return { streaming: { ...stream, tools } }
            }
            case 'toolcall_end': {
              const tools = stream.tools.map((tool, index) =>
                index === stream.tools.length - 1
                  ? {
                      ...tool,
                      id: update.toolCall?.id ?? tool.id,
                      name: update.toolCall?.name ?? tool.name,
                      argsText: JSON.stringify(update.toolCall?.arguments ?? {})
                    }
                  : tool
              )
              return { streaming: { ...stream, tools } }
            }
            default:
              return {}
          }
        })
        if (update.type === 'error') {
          set({ error: update.reason ?? 'The model stream ended with an error.' })
        }
        break
      }

      case 'message_end':
        set({ streaming: null })
        void get().refresh()
        break

      case 'tool_execution_start': {
        const id = String(event.toolCallId ?? '')
        const name = String(event.toolName ?? 'tool')
        set((state) => ({
          runningTools: [
            ...state.runningTools.filter((tool) => tool.id !== id),
            { id, name, args: event.args, text: '' }
          ]
        }))
        break
      }

      case 'tool_execution_update': {
        const id = String(event.toolCallId ?? '')
        const name = String(event.toolName ?? 'tool')
        const text = partialToolText(event.partialResult)
        // A tool that streams nothing still gets a card, so it is added when
        // the first update arrives rather than assumed to exist already.
        set((state) => ({
          runningTools: state.runningTools.some((tool) => tool.id === id)
            ? state.runningTools.map((tool) => (tool.id === id ? { ...tool, text } : tool))
            : [...state.runningTools, { id, name, args: event.args, text }]
        }))
        break
      }

      case 'tool_execution_end':
        set((state) => ({
          runningTools: state.runningTools.filter(
            (tool) => tool.id !== String(event.toolCallId ?? '')
          )
        }))
        void get().refresh()
        // A tool that writes a file adds it to the Changes tab.
        void get().loadChanges()
        break

      case 'agent_settled':
        set({ streaming: null, runningTools: [], busy: false })
        void get().refresh()
        void get().loadSessions()
        void get().loadChanges()
        break

      case 'queue_update':
        set((state) => ({
          status: state.status
            ? {
                ...state.status,
                steering: (event.steering as string[]) ?? [],
                followUp: (event.followUp as string[]) ?? [],
                pendingMessageCount:
                  ((event.steering as string[]) ?? []).length +
                  ((event.followUp as string[]) ?? []).length
              }
            : state.status
        }))
        break

      case 'thinking_level_changed':
        set((state) => ({
          status: state.status
            ? { ...state.status, thinkingLevel: event.level as ThinkingLevelDto }
            : state.status
        }))
        break

      case 'session_info_changed':
        set((state) => ({
          status: state.status
            ? { ...state.status, sessionName: (event.name as string | null) ?? null }
            : state.status
        }))
        break

      default:
        break
    }
  },

  dismissNotice: (id) => set((state) => ({ notices: state.notices.filter((n) => n.id !== id) })),

  respondToDialog: async (response) => {
    set({ dialog: null })
    try {
      await window.luna.respondToUi(response)
    } catch (cause) {
      set({ error: describeError(cause) })
    }
  },

  focusSearch: () => set((state) => ({ searchFocusSeq: state.searchFocusSeq + 1 })),

  saveApprovalConfig: async (config) => {
    try {
      await window.luna.setApprovalConfig(config)
      set({ approvalConfig: config })
    } catch (cause) {
      set({ error: describeError(cause) })
    }
  },

  loadModelsConfig: async () => {
    try {
      const config = await window.luna.getModelsConfig()
      set({ modelsConfig: config, modelsError: null })
    } catch (cause) {
      set({ modelsError: describeError(cause) })
    }
  },

  saveModelsConfig: async (config) => {
    try {
      const result = await window.luna.setModelsConfig(config)
      set({ modelsConfig: config, modelsError: null, models: result.models })
    } catch (cause) {
      set({ modelsError: describeError(cause) })
    }
  },

  checkForUpdates: async () => {
    try {
      set({ update: await window.luna.checkForUpdates() })
    } catch (cause) {
      set({ error: describeError(cause) })
    }
  },

  downloadUpdate: async () => {
    try {
      set({ update: await window.luna.downloadUpdate() })
    } catch (cause) {
      set({ error: describeError(cause) })
    }
  },

  installUpdate: () => {
    void window.luna.installUpdate()
  },

  loadSessions: async () => {
    try {
      set({ sessions: await window.luna.listSessions() })
    } catch (cause) {
      set({ error: describeError(cause) })
    }
  },

  loadCommands: async () => {
    try {
      set({ commands: await window.luna.getCommands() })
    } catch (cause) {
      set({ error: describeError(cause) })
    }
  },

  switchSession: async (path) => {
    set({ busy: true, streaming: null, runningTools: [] })
    try {
      await window.luna.switchSession(path)
      await get().refresh()
      await get().loadSessions()
    } catch (cause) {
      set({ error: describeError(cause) })
    } finally {
      set({ busy: false })
    }
  },

  renameSession: async (name) => {
    try {
      await window.luna.renameSession(name)
      await get().loadSessions()
      await get().refresh()
    } catch (cause) {
      set({ error: describeError(cause) })
    }
  },

  forkSession: async () => {
    try {
      await window.luna.forkSession()
      await get().refresh()
      await get().loadSessions()
    } catch (cause) {
      set({ error: describeError(cause) })
    }
  },

  changeWorkspace: async () => {
    try {
      const path = await window.luna.pickWorkspace()
      if (!path) return
      await get().openWorkspace(path)
    } catch (cause) {
      set({ error: describeError(cause) })
    }
  },

  openWorkspace: async (path) => {
    set({ busy: true })
    try {
      const workspace = await window.luna.setWorkspace(path)
      set({ workspace, items: [], streaming: null, runningTools: [] })
      await get().refresh()
      await get().loadSessions()
      await get().loadAllSessions()
    } catch (cause) {
      set({ error: describeError(cause) })
    } finally {
      set({ busy: false })
    }
  },

  setTheme: (id) => {
    saveThemeId(id)
    set({ themeId: id })
  },

  setPrefs: (patch) => {
    const prefs = { ...get().prefs, ...patch }
    saveUiPreferences(prefs)
    set({ prefs })
  },

  openSettings: () => set({ settingsOpen: true }),
  closeSettings: () => set({ settingsOpen: false }),
  setSettingsTab: (tab) => set({ settingsTab: tab }),
  openFile: (path) => {
    // Luna has no editor pane, so a changed file opens in whatever the user has
    // registered for it rather than in a tab that does not exist.
    void window.luna.revealPath(path)
  },
  setSessionQuery: (query) => set({ sessionQuery: query }),
  setSessionFilter: (filter) => set({ sessionFilter: filter }),

  toggleStar: (path) => {
    const starred = get().starred.includes(path)
      ? get().starred.filter((entry) => entry !== path)
      : [...get().starred, path]
    saveStarred(starred)
    set({ starred })
  },

  loadAllSessions: async () => {
    try {
      set({ allSessions: await window.luna.listAllSessions() })
    } catch (cause) {
      set({ error: describeError(cause) })
    }
  },

  setMainTab: (tab) => set({ mainTab: tab }),

  loadChanges: async () => {
    try {
      const state = await window.luna.getChanges()
      set({ changes: state.changes, changesRevision: state.revision })
    } catch {
      // The change list is advisory; a failure must not surface as an error.
    }
  },

  keepChanges: async (path) => {
    try {
      const state = await window.luna.keepChanges(path)
      set({ changes: state.changes, changesRevision: state.revision })
    } catch (cause) {
      set({ error: describeError(cause) })
    }
  },

  undoChanges: async (path) => {
    try {
      await window.luna.undoChanges(path)
      await get().loadChanges()
    } catch (cause) {
      set({ error: describeError(cause) })
    }
  },

  deleteSession: async (path) => {
    try {
      await window.luna.deleteSession(path)
      await get().loadSessions()
      await get().loadAllSessions()
    } catch (cause) {
      set({ error: describeError(cause) })
    }
  },

  send: async (text, mode = 'prompt', images) => {
    const trimmed = text.trim()
    // An image on its own is a valid message; empty text with nothing attached is not.
    if (trimmed.length === 0 && (images ?? []).length === 0) return
    set({ busy: true, error: null })

    const editing = get().editing

    try {
      if (editing) {
        // A rewrite replaces the turn rather than adding to it, so steering and
        // follow-ups do not apply — there is nothing in flight to steer.
        await window.luna.editMessage(editing.entryId, trimmed)
        set({ editing: null })
      } else if (mode === 'steer') await window.luna.steer(trimmed)
      else if (mode === 'followUp') await window.luna.followUp(trimmed)
      else await window.luna.prompt({ text: trimmed, images })
      await get().refresh()
    } catch (cause) {
      // A failed rewrite must not swallow the text: the composer cleared it on
      // the way out, so hand the message back with what the user actually typed.
      // A new object, because the composer watches the message, not the text.
      set({
        error: describeError(cause),
        busy: false,
        ...(editing ? { editing: { ...editing, text: trimmed } } : {})
      })
    }
  },

  beginEdit: (item) => set({ editing: item, error: null }),

  cancelEdit: () => set({ editing: null }),

  retryMessage: async (entryId) => {
    set({ busy: true, error: null })
    try {
      await window.luna.retryMessage(entryId)
      await get().refresh()
    } catch (cause) {
      set({ error: describeError(cause) })
    } finally {
      set({ busy: false })
    }
  },

  abort: async () => {
    try {
      await window.luna.abort()
    } catch (cause) {
      set({ error: describeError(cause) })
    } finally {
      set({ busy: false })
      await get().refresh()
    }
  },

  newSession: async () => {
    set({ busy: true, streaming: null, runningTools: [], items: [] })
    try {
      await window.luna.newSession()
      await get().refresh()
      await get().loadSessions()
      await get().loadCommands()
    } catch (cause) {
      set({ error: describeError(cause) })
    } finally {
      set({ busy: false })
    }
  },

  compact: async () => {
    set({ busy: true })
    try {
      await window.luna.compact()
      await get().refresh()
    } catch (cause) {
      set({ error: describeError(cause) })
    } finally {
      set({ busy: false })
    }
  },

  selectModel: async (provider, id) => {
    try {
      await window.luna.setModel(provider, id)
      await get().refresh()
    } catch (cause) {
      set({ error: describeError(cause) })
    }
  },

  selectThinking: async (level) => {
    try {
      await window.luna.setThinkingLevel(level)
      await get().refresh()
    } catch (cause) {
      set({ error: describeError(cause) })
    }
  }
}))
