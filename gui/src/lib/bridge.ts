import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type {
  AgentEventDto,
  AppInfo,
  ApprovalConfig,
  ChatItemDto,
  ChangesStateDto,
  CommandDto,
  FsFileDto,
  ModelDefDto,
  ModelDto,
  ModelsConfigDto,
  ModelsUpdateResultDto,
  NoticeDto,
  PromptInput,
  RuntimeInfoDto,
  SessionStatusDto,
  SessionSummaryDto,
  ThinkingLevelDto,
  UiRequestDto,
  UiResponseDto,
  UpdateStateDto,
  WorkspaceDto
} from '@shared/ipc'

/**
 * The renderer's only contact with the Rust core.
 *
 * Every ported piui component talks to `window.luna` and nothing else, so this
 * file is the whole adaptation layer: the chat surface is unchanged, and the
 * engine behind it is Luna's.
 */

const EVENT_CHANNEL = 'luna://agent-event'
const NOTICE_CHANNEL = 'luna://notice'
const UI_REQUEST_CHANNEL = 'luna://ui-request'
const UPDATE_CHANNEL = 'luna://update'

/** Subscribe to a Tauri channel, returning piui's unsubscribe shape. */
function subscribe<T>(channel: string, listener: (payload: T) => void): () => void {
  let unlisten: UnlistenFn | null = null
  let cancelled = false

  void listen<T>(channel, (event) => listener(event.payload)).then((off) => {
    // The caller may have unsubscribed before `listen` resolved.
    if (cancelled) off()
    else unlisten = off
  })

  return () => {
    cancelled = true
    unlisten?.()
  }
}

export interface LunaApi {
  getAppInfo(): Promise<AppInfo>
  getRuntimeInfo(): Promise<RuntimeInfoDto>
  getStatus(): Promise<SessionStatusDto>
  getMessages(): Promise<ChatItemDto[]>
  getModels(): Promise<ModelDto[]>
  getModelDefs(): Promise<ModelDefDto[]>
  getCommands(): Promise<CommandDto[]>
  prompt(input: PromptInput): Promise<void>
  steer(text: string): Promise<void>
  followUp(text: string): Promise<void>
  abort(): Promise<void>
  newSession(): Promise<void>
  compact(customInstructions?: string): Promise<void>
  editMessage(entryId: string, text: string): Promise<void>
  retryMessage(entryId: string): Promise<void>
  setModel(provider: string, id: string): Promise<void>
  setThinkingLevel(level: ThinkingLevelDto): Promise<void>
  onAgentEvent(listener: (event: AgentEventDto) => void): () => void
  onNotice(listener: (notice: NoticeDto) => void): () => void
  onUiRequest(listener: (request: UiRequestDto) => void): () => void
  onUpdate(listener: (update: UpdateStateDto) => void): () => void
  respondToUi(response: UiResponseDto): Promise<void>
  getApprovalConfig(): Promise<ApprovalConfig>
  setApprovalConfig(config: ApprovalConfig): Promise<void>
  listSessions(): Promise<SessionSummaryDto[]>
  listAllSessions(): Promise<SessionSummaryDto[]>
  switchSession(path: string): Promise<void>
  renameSession(name: string): Promise<void>
  forkSession(): Promise<void>
  deleteSession(path: string): Promise<void>
  getWorkspace(): Promise<WorkspaceDto>
  pickWorkspace(): Promise<string | null>
  setWorkspace(path: string): Promise<WorkspaceDto>
  getChanges(): Promise<ChangesStateDto>
  keepChanges(path: string | null): Promise<ChangesStateDto>
  undoChanges(path: string | null): Promise<void>
  getModelsConfig(): Promise<ModelsConfigDto>
  setModelsConfig(config: ModelsConfigDto): Promise<ModelsUpdateResultDto>
  /** Show a path in the operating system's file manager. */
  revealPath(path: string): Promise<void>
  /** Read a file as text, for attachments dropped from the file manager. */
  readFile(path: string): Promise<FsFileDto>
  getUpdateState(): Promise<UpdateStateDto>
  checkForUpdates(): Promise<UpdateStateDto>
  downloadUpdate(): Promise<UpdateStateDto>
  installUpdate(): Promise<void>
}

export const luna: LunaApi = {
  getAppInfo: () => invoke('get_app_info'),
  getRuntimeInfo: () => invoke('get_runtime_info'),
  getStatus: () => invoke('get_status'),
  getMessages: () => invoke('get_messages'),
  getModels: () => invoke('get_models'),
  getModelDefs: () => invoke('get_model_defs'),
  getCommands: () => invoke('get_commands'),

  prompt: (input) => invoke('prompt', { input }),
  steer: (text) => invoke('steer', { text }),
  followUp: (text) => invoke('follow_up', { text }),
  abort: () => invoke('abort'),
  newSession: () => invoke('new_session'),
  compact: (customInstructions) => invoke('compact', { customInstructions }),
  editMessage: (entryId, text) => invoke('edit_message', { entryId, text }),
  retryMessage: (entryId) => invoke('retry_message', { entryId }),
  setModel: (provider, id) => invoke('set_model', { provider, id }),
  setThinkingLevel: (level) => invoke('set_thinking_level', { level }),

  onAgentEvent: (listener) => subscribe<AgentEventDto>(EVENT_CHANNEL, listener),
  onNotice: (listener) => subscribe<NoticeDto>(NOTICE_CHANNEL, listener),
  onUiRequest: (listener) => subscribe<UiRequestDto>(UI_REQUEST_CHANNEL, listener),
  onUpdate: (listener) => subscribe<UpdateStateDto>(UPDATE_CHANNEL, listener),
  respondToUi: (response) => invoke('respond_to_ui', { response }),

  getApprovalConfig: () => invoke('get_approval_config'),
  setApprovalConfig: (config) => invoke('set_approval_config', { config }),

  listSessions: () => invoke('list_sessions'),
  listAllSessions: () => invoke('list_all_sessions'),
  switchSession: (path) => invoke('switch_session', { path }),
  renameSession: (name) => invoke('rename_session', { name }),
  forkSession: () => invoke('fork_session'),
  deleteSession: (path) => invoke('delete_session', { path }),

  getWorkspace: () => invoke('get_workspace'),
  pickWorkspace: () => invoke('pick_workspace'),
  setWorkspace: (path) => invoke('set_workspace', { path }),

  getChanges: () => invoke('get_changes'),
  keepChanges: (path) => invoke('keep_changes', { path }),
  undoChanges: (path) => invoke('undo_changes', { path }),

  getModelsConfig: () => invoke('get_models_config'),
  setModelsConfig: (config) => invoke('set_models_config', { config }),
  revealPath: (path) => invoke('reveal_path', { path }),
  readFile: (path) => invoke('read_file', { path }),

  getUpdateState: () => invoke('get_update_state'),
  checkForUpdates: () => invoke('check_for_updates'),
  downloadUpdate: () => invoke('download_update'),
  installUpdate: () => invoke('install_update')
}

declare global {
  interface Window {
    luna: LunaApi
  }
}

/**
 * In a plain browser there is no Tauri runtime, so the mock takes over. Inside
 * the app `__TAURI_INTERNALS__` is always present, so this never fires there.
 *
 * The mock is imported dynamically so it is not part of the packaged bundle's
 * startup path; `ready` lets `main.tsx` wait for it before rendering.
 */
const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

export const ready: Promise<void> = inTauri
  ? Promise.resolve().then(() => {
      window.luna = luna
    })
  : import('./mock').then(({ createMockApi }) => {
      window.luna = createMockApi()
    })
