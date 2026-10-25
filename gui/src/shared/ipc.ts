/**
 * Shared IPC contract between the PiUI main process and the renderer.
 *
 * Keep this module dependency-free — it is imported by both the Node-side
 * main process and the browser-side renderer, so it must not reference
 * Electron, Node built-ins, or DOM globals.
 */

export const IpcChannel = {
  /** Returns static application/runtime information. */
  AppInfo: 'piui:app:info',
  /** Describes the embedded SDK and the installed CLI. */
  RuntimeInfo: 'piui:runtime:info',
  AgentGetStatus: 'piui:agent:get-status',
  AgentGetMessages: 'piui:agent:get-messages',
  AgentGetModels: 'piui:agent:get-models',
  AgentGetStats: 'piui:agent:get-stats',
  AgentPrompt: 'piui:agent:prompt',
  AgentSteer: 'piui:agent:steer',
  AgentFollowUp: 'piui:agent:follow-up',
  AgentAbort: 'piui:agent:abort',
  AgentClearQueue: 'piui:agent:clear-queue',
  AgentNewSession: 'piui:agent:new-session',
  AgentCompact: 'piui:agent:compact',
  /** Rewrite a message the user already sent, discarding what followed it. */
  AgentEditMessage: 'piui:agent:edit-message',
  /** Run the turn behind an assistant message again. */
  AgentRetryMessage: 'piui:agent:retry-message',
  AgentSetModel: 'piui:agent:set-model',
  AgentCycleModel: 'piui:agent:cycle-model',
  AgentSetThinking: 'piui:agent:set-thinking',
  /** Slash commands discovered from skills and prompt templates. */
  AgentGetCommands: 'piui:agent:get-commands',
  /** Resolve a pending extension-UI dialog opened by the main process. */
  UiRespond: 'piui:ui:respond',
  /** Read or replace the tool-approval rules. */
  ApprovalGetConfig: 'piui:approval:get-config',
  ApprovalSetConfig: 'piui:approval:set-config',
  SessionsList: 'piui:sessions:list',
  SessionsListAll: 'piui:sessions:list-all',
  SessionsSwitch: 'piui:sessions:switch',
  SessionsRename: 'piui:sessions:rename',
  SessionsFork: 'piui:sessions:fork',
  WorkspaceGet: 'piui:workspace:get',
  WorkspacePick: 'piui:workspace:pick',
  WorkspaceSet: 'piui:workspace:set',
  FsList: 'piui:fs:list',
  FsRead: 'piui:fs:read',
  FsWrite: 'piui:fs:write',
  /** File operations the explorer's context menu performs. */
  FsRename: 'piui:fs:rename',
  FsDelete: 'piui:fs:delete',
  FsTransfer: 'piui:fs:transfer',
  FsCreate: 'piui:fs:create',
  /** Reveal a path in the operating system's file manager. */
  FsReveal: 'piui:fs:reveal',
  /** Take back the last file operation the explorer performed. */
  FsUndo: 'piui:fs:undo',
  /** Ask where to write a buffer that has no path yet. */
  FsPickSavePath: 'piui:fs:pick-save-path',
  /** Files the agent has changed, for review. */
  ChangesGet: 'piui:changes:get',
  ChangesKeep: 'piui:changes:keep',
  ChangesUndo: 'piui:changes:undo',
  /** Accept the agent's edits without review, so nothing is listed or marked. */
  ChangesSetAutoKeep: 'piui:changes:set-auto-keep',
  /** Run the official installer for the `pi` CLI, which PiUI depends on. */
  PiInstall: 'piui:pi:install',
  /** Relaunch the app, for changes that only take effect on a fresh start. */
  AppRelaunch: 'piui:app:relaunch',
  /** Format a buffer with Prettier before it is written. */
  FsFormat: 'piui:fs:format',
  /** Recursive workspace file index for quick open. */
  FsIndex: 'piui:fs:index',
  SessionsDelete: 'piui:sessions:delete',
  MonitorGet: 'piui:monitor:get',
  /** Read or replace the provider/model definitions in `models.json`. */
  ModelsGet: 'piui:models:get',
  ModelsSet: 'piui:models:set',
  /** Models a provider offers, for the Add model picker. */
  ModelsCatalog: 'piui:models:catalog',
  /** Real terminal sessions backed by a pseudo-terminal. */
  TerminalCreate: 'piui:terminal:create',
  TerminalWrite: 'piui:terminal:write',
  TerminalResize: 'piui:terminal:resize',
  TerminalDispose: 'piui:terminal:dispose',
  /** pi.dev package catalogue and installation. */
  PackagesSearch: 'piui:packages:search',
  PackagesInstalled: 'piui:packages:installed',
  PackagesInstall: 'piui:packages:install',
  PackagesRemove: 'piui:packages:remove',
  /** Self-update: state, manual check, download, and install. */
  UpdateGetState: 'piui:update:get-state',
  UpdateCheck: 'piui:update:check',
  UpdateDownload: 'piui:update:download',
  UpdateInstall: 'piui:update:install'
} as const

export type IpcChannel = (typeof IpcChannel)[keyof typeof IpcChannel]

/** Push channels sent from the main process to the renderer. */
export const IpcEvent = {
  AgentEvent: 'piui:agent:event',
  Notice: 'piui:notice',
  /** A dialog an extension (or PiUI itself) is waiting on. */
  UiRequest: 'piui:ui:request',
  /** Output from a terminal session. */
  TerminalData: 'piui:terminal:data',
  /** A terminal session ended. */
  TerminalExit: 'piui:terminal:exit',
  /** Progress from a package install. */
  PackagesProgress: 'piui:packages:progress',
  /** A change in the self-update state. */
  Update: 'piui:update'
} as const

export type IpcEvent = (typeof IpcEvent)[keyof typeof IpcEvent]

/** One package from the pi.dev catalogue. */
export interface CatalogPackageDto {
  name: string
  description: string
  /** Any of `skill`, `extension`, `prompt`, `theme`. */
  types: string[]
  downloads: number
  /** ISO 8601 timestamp of the last publish, or an empty string. */
  updatedAt: string
  /** Install spec handed to the package manager, e.g. `npm:pi-hermes-memory`. */
  source: string
  url: string
}

/** One page of catalogue search results. */
export interface CatalogPageDto {
  items: CatalogPackageDto[]
  page: number
  pageSize: number
  /** Total matches for the query, before paging. */
  total: number
}

/** A package already configured in settings. */
export interface InstalledPackageDto {
  source: string
  scope: 'user' | 'project'
  filtered: boolean
}

/** Progress from a package install. */
export interface PackageProgressDto {
  source: string
  /** Lifecycle stage the event belongs to. */
  phase: 'start' | 'progress' | 'complete' | 'error'
  /** What the package manager is doing, e.g. `install` or `clone`. */
  action: string
  message: string
}

/** Options for starting a terminal session. */
export interface TerminalCreateInput {
  /** Caller-chosen id; reusing an id re-attaches to the running shell. */
  id: string
  cwd: string
  cols: number
  rows: number
  /**
   * Command to run in the terminal, e.g. `pwsh.exe -NoLogo` or
   * `docker exec -it pi bash`. Empty picks a sensible default shell.
   */
  shell?: string
}

/** A live terminal session, including the output produced so far. */
export interface TerminalSessionDto {
  id: string
  cwd: string
  /** Command line of the shell that was started. */
  shell: string
  /** Bounded replay of everything the shell has written. */
  buffer: string
}

/** One chunk of terminal output. */
export interface TerminalDataDto {
  id: string
  data: string
}

/** A terminal session ended. */
export interface TerminalExitDto {
  id: string
  exitCode: number
}

/** A slash command the composer can offer. */
export interface CommandDto {
  /** What the user types, including the leading slash. */
  name: string
  description: string
  /** Where the command comes from. */
  kind: 'builtin' | 'template' | 'skill'
  /** Placeholder for the arguments, when the command takes any. */
  argumentHint?: string
}

/** Static information about the running PiUI application. */
export interface AppInfo {
  /** Application name, e.g. `PiUI`. */
  name: string
  /** Application version from package.json. */
  version: string
  /** Host platform, e.g. `win32`, `linux`. */
  platform: string
  /** Underlying runtime versions. */
  versions: {
    electron: string
    chrome: string
    node: string
    v8: string
  }
}

/** Reasoning effort supported by a model. */
export type ThinkingLevelDto = 'off' | 'minimal' | 'low' | 'medium' | 'high' | 'xhigh' | 'max'

/** A model PiUI can select, flattened for the renderer. */
export interface ModelDto {
  provider: string
  id: string
  name: string
  reasoning: boolean
  contextWindow: number
  /** Maximum tokens the model will generate in one response. */
  maxTokens: number
  input: string[]
  cost: { input: number; output: number; cacheRead: number; cacheWrite: number }
}

/** Token usage against the active model's context window. */
export interface ContextUsageDto {
  /** Estimated context tokens, or null right after compaction. */
  tokens: number | null
  contextWindow: number
  percent: number | null
}

/** Full session snapshot used to drive the header, footer, and status UI. */
export interface SessionStatusDto {
  cwd: string
  model: ModelDto | null
  thinkingLevel: ThinkingLevelDto
  availableThinkingLevels: ThinkingLevelDto[]
  supportsThinking: boolean
  isStreaming: boolean
  isCompacting: boolean
  isRetrying: boolean
  sessionId: string
  sessionFile: string | null
  sessionName: string | null
  messageCount: number
  pendingMessageCount: number
  steering: string[]
  followUp: string[]
  contextUsage: ContextUsageDto | null
  cost: number
  tokens: { input: number; output: number; cacheRead: number; cacheWrite: number; total: number }
}

/** Cumulative usage statistics for the session. */
export interface SessionStatsDto {
  sessionFile: string | null
  sessionId: string
  userMessages: number
  assistantMessages: number
  toolCalls: number
  toolResults: number
  totalMessages: number
  tokens: { input: number; output: number; cacheRead: number; cacheWrite: number; total: number }
  cost: number
}

/** Version information for the embedded SDK and the installed CLI. */
export interface RuntimeInfoDto {
  sdkVersion: string
  cliVersion: string | null
  cliPath: string | null
  /** False when nothing that looks like the `pi` CLI exists on this machine. */
  cliInstalled: boolean
  versionMatch: boolean
  agentDir: string
  node: string
  /** Path of the SDK copy in use, or null when PiUI's bundled copy is used. */
  sdkPath: string | null
}

/** A renderable block inside an assistant turn. */
export type ChatBlockDto =
  | { type: 'text'; text: string }
  | { type: 'thinking'; text: string }
  | { type: 'toolCall'; id: string; name: string; arguments: unknown }

/** A renderable transcript entry. */
export type ChatItemDto =
  | {
      kind: 'user'
      id: string
      text: string
      imageCount: number
      /**
       * Session entry this message came from, when the host could identify it.
       * Editing and retrying move the session's leaf back to this entry, so the
       * turn is rebuilt rather than appended to.
       */
      entryId?: string
    }
  | {
      kind: 'assistant'
      id: string
      blocks: ChatBlockDto[]
      error?: string
      stopped?: boolean
      /** Session entry behind the response, for "try again". */
      entryId?: string
    }
  | {
      kind: 'tool'
      id: string
      toolCallId: string
      name: string
      arguments: unknown
      /** Result text, once the tool has finished. */
      text?: string
      isError?: boolean
      /** True while the call is still executing. */
      running?: boolean
      /** Display diff produced by the edit tool. */
      diff?: string
      /** Unified patch produced by the edit tool. */
      patch?: string
      /** Absolute or relative path the tool acted on, when known. */
      filePath?: string
      addedLines?: number
      removedLines?: number
      /** Wall-clock execution time in milliseconds, when known. */
      durationMs?: number
    }
  | { kind: 'bash'; id: string; command: string; output: string; exitCode: number | null }

/** A session event forwarded from the main process, already JSON-safe. */
export type AgentEventDto = { type: string } & Record<string, unknown>

/** A transient message to show in the UI. */
export interface NoticeDto {
  level: 'info' | 'warning' | 'error'
  message: string
}

/** Input for sending a prompt to the agent. */
export interface PromptInput {
  text: string
  images?: { type: 'image'; data: string; mimeType: string }[]
  /** Required when the agent is already streaming. */
  streamingBehavior?: 'steer' | 'followUp'
}

/** A dialog the agent or an extension is waiting on the user to answer. */
export type UiRequestDto =
  | { id: string; method: 'select'; title: string; options: string[] }
  | { id: string; method: 'confirm'; title: string; message: string; danger?: boolean }
  | { id: string; method: 'input'; title: string; placeholder?: string }
  | { id: string; method: 'editor'; title: string; prefill?: string }
  | {
      id: string
      method: 'approval'
      /** Name of the tool that wants to run. */
      tool: string
      /** Short headline, e.g. `Wants to run a command`. */
      title: string
      /** The command or path being approved. */
      detail: string
    }

/** The renderer's answer to a {@link UiRequestDto}. */
export type UiResponseDto =
  | { id: string; cancelled: true }
  | { id: string; value: string }
  | { id: string; confirmed: boolean }
  | { id: string; decision: 'allow' | 'deny' }

/** What PiUI does before running a tool. */
export type ApprovalAction = 'allow' | 'ask' | 'deny'

/** Tools the approval list shows, in display order. */
export const APPROVAL_TOOLS = [
  'read',
  'bash',
  'powershell',
  'edit',
  'write',
  'grep',
  'find',
  'ls'
] as const

/**
 * Approval policy: one action per tool name. A `*` entry covers any tool not
 * listed, and `defaultPolicy` covers the rest.
 */
export interface ApprovalConfig {
  defaultPolicy: ApprovalAction
  tools: Record<string, ApprovalAction>
}

/** One model definition inside a provider entry of `models.json`. */
export interface ModelDefDto {
  id: string
  name: string
  reasoning: boolean
  /** Modalities the model accepts, e.g. `text` and `image`. */
  input: string[]
  contextWindow: number
  maxTokens: number
  cost: { input: number; output: number; cacheRead: number; cacheWrite: number }
}

/** One provider entry of `models.json`. */
export interface ProviderConfigDto {
  id: string
  baseUrl: string
  api: string
  apiKey: string
  models: ModelDefDto[]
}

/** Result of a format-on-save request. */
export interface FormatResultDto {
  /** Formatted text, or null when the file was left untouched. */
  text: string | null
  /** Why nothing was formatted, when that is worth reporting. */
  error: string | null
}

/** The provider/model catalogue PiUI edits, persisted as `<agentDir>/models.json`. */
export interface ModelsConfigDto {
  /** Absolute path of the file the catalogue is read from. */
  path: string
  providers: ProviderConfigDto[]
}

/** Result of saving the catalogue: the reloaded definitions and model list. */
export interface ModelsUpdateResultDto {
  config: ModelsConfigDto
  models: ModelDto[]
  /**
   * Why the catalogue could not be used, when it could not. One invalid field
   * invalidates the whole file, and the effect is that no model is selectable.
   */
  error: string | null
}

/** One saved session, flattened for the session browser. */
export interface SessionSummaryDto {
  path: string
  id: string
  cwd: string
  name: string | null
  /** ISO 8601 timestamp. */
  created: string
  /** ISO 8601 timestamp. */
  modified: string
  messageCount: number
  /** First user message, used as a fallback title. */
  firstMessage: string
  parentSessionPath: string | null
}

/** The working directory the agent is currently operating on. */
export interface WorkspaceDto {
  cwd: string
  /** Leaf folder name, for display. */
  name: string
}

/** One entry in the file explorer. */
export interface FsEntryDto {
  name: string
  path: string
  kind: 'file' | 'directory'
  /** Working-tree status when the folder is in a git repository. */
  status?: 'M' | 'U'
}

/** A directory listing for the explorer. */
export interface FsListingDto {
  path: string
  entries: FsEntryDto[]
  /** Set when the path is outside the workspace or unreadable. */
  error: string | null
}

/** File contents for the editor. */
export interface FsFileDto {
  path: string
  content: string
  /** Set when the file could not be read as text. */
  error: string | null
}

/** Outcome of a file operation, so the explorer can say why one was refused. */
export interface FsResultDto {
  ok: boolean
  error: string | null
  /** Set by an undo, describing what it reversed. */
  label?: string
}

/** Outcome of running the pi installer. */
export interface PiInstallResultDto {
  ok: boolean
  /** The tail of the installer's output, shown when something went wrong. */
  output: string
  error: string | null
}

/** Lines of the current file an edit touched, for the editor's gutter. */
export interface LineChangeDto {
  /** 1-based line numbers inserted. */
  added: number[]
  /** 1-based line numbers that replaced something. */
  modified: number[]
  /**
   * 1-based line numbers a deletion sits above, since a removed line has no line
   * of its own to mark. 1 when the deletion is above the first line.
   */
  removed: number[]
}

/** A file the agent changed, and what it would take to put it back. */
export interface PendingChangeDto {
  path: string
  /** Workspace-relative path, for display. */
  relative: string
  /** True when the agent created the file rather than editing an existing one. */
  created: boolean
  /** Lines that differ, removed then added. */
  diff: string
  added: number
  removed: number
  /** Where those lines are, so the editor can mark them in the gutter. */
  lines: LineChangeDto
}

/** The files the agent changed, and a counter that moves whenever they do. */
export interface ChangesStateDto {
  changes: PendingChangeDto[]
  /**
   * Increments when a reviewed file may have changed on disk. The editor watches
   * it so an open buffer is re-read rather than quietly showing what the file
   * used to say.
   */
  revision: number
}

/** One GPU as reported by nvidia-smi. */
export interface MonitorGpuDto {
  name: string
  /** Percent, 0-100. */
  utilization: number
  /** MiB. */
  memoryUsed: number
  /** MiB. */
  memoryTotal: number
  powerWatts: number | null
  /** Core temperature in Celsius, when the source reports it. */
  temperatureC: number | null
}

/** Host CPU and memory, which is where a local engine's work actually happens. */
export interface MonitorSystemDto {
  /** CPU model, as reported by the operating system. */
  cpuModel: string
  /** Logical core count. */
  cpuCores: number
  /** Busy percentage across every core, or null on the first sample. */
  cpuPercent: number | null
  /** MiB. */
  memoryUsed: number
  /** MiB. */
  memoryTotal: number
}

/** What the model is doing right now, as far as the endpoint will say. */
export interface MonitorStatusDto {
  /** Which source produced the numbers. */
  source: 'engine' | 'slots' | 'none'
  phase: 'idle' | 'prompt' | 'generate'
  /** Prompt tokens processed so far, when reading the prompt. */
  promptProcessed: number | null
  /** Total prompt tokens for the current request. */
  promptTotal: number | null
  /** Tokens generated for the current request. */
  generated: number | null
  /** Configured output ceiling for the active model. */
  maxOutput: number | null
  /** Seconds elapsed on the current request. */
  elapsedSeconds: number | null
}

/** Everything the Monitor tab renders. */
export interface MonitorSnapshotDto {
  /** ISO 8601 timestamp of the snapshot. */
  at: string
  engine: {
    /** Whether the inference endpoint answered a metrics request. */
    available: boolean
    model: string | null
    endpoint: string | null
    contextWindow: number | null
    error: string | null
  }
  speed: {
    answerTokensPerSecond: number | null
    promptTokensPerSecond: number | null
    /** Answer speed history, oldest first, for the sparkline. */
    history: number[]
  }
  kvCache: {
    usageRatio: number | null
    tokens: number | null
  }
  requests: {
    processing: number
    deferred: number
  }
  gpus: MonitorGpuDto[]
  /** CPU and memory of the machine running the app. */
  system: MonitorSystemDto
  /** Current request state, for the Monitor's Status section. */
  status: MonitorStatusDto
}

/** Where the self-updater is in its cycle. */
export type UpdatePhaseDto =
  | 'idle'
  | 'checking'
  | 'available'
  | 'downloading'
  | 'ready'
  | 'up-to-date'
  | 'unsupported'
  | 'error'

/** Self-update state, mirrored into the renderer so it can prompt. */
export interface UpdateStateDto {
  phase: UpdatePhaseDto
  /** Version on offer, or the one that was downloaded. */
  version: string | null
  /** Version currently running. */
  currentVersion: string
  /** Download progress, 0-100. */
  percent: number | null
  /** Why updates are unavailable, or what went wrong. */
  message: string | null
  /** Release notes for the offered version, when the release carries any. */
  notes: string | null
  /** False for portable and development builds, which cannot self-update. */
  canInstall: boolean
}

/**
 * The API surface PiUI exposes to the renderer as `window.piui`.
 * Every method is implemented in the preload script and backed by IPC.
 */
export interface PiUiApi {
  getAppInfo(): Promise<AppInfo>
  getRuntimeInfo(): Promise<RuntimeInfoDto>
  getStatus(): Promise<SessionStatusDto>
  getMessages(): Promise<ChatItemDto[]>
  getModels(): Promise<ModelDto[]>
  getStats(): Promise<SessionStatsDto>
  /** List slash commands from skills, prompt templates, and PiUI itself. */
  getCommands(): Promise<CommandDto[]>
  prompt(input: PromptInput): Promise<void>
  steer(text: string): Promise<void>
  followUp(text: string): Promise<void>
  abort(): Promise<void>
  clearQueue(): Promise<{ steering: string[]; followUp: string[] }>
  newSession(): Promise<void>
  compact(customInstructions?: string): Promise<void>
  /** Replace a sent message and re-run the turn from there. */
  editMessage(entryId: string, text: string): Promise<void>
  /** Re-run the turn that produced an assistant message. */
  retryMessage(entryId: string): Promise<void>
  setModel(provider: string, id: string): Promise<void>
  cycleModel(): Promise<void>
  setThinkingLevel(level: ThinkingLevelDto): Promise<void>
  /** Subscribe to session events. Returns an unsubscribe function. */
  onAgentEvent(listener: (event: AgentEventDto) => void): () => void
  /** Subscribe to transient notices. Returns an unsubscribe function. */
  onNotice(listener: (notice: NoticeDto) => void): () => void
  /** Subscribe to extension-UI dialogs. Returns an unsubscribe function. */
  onUiRequest(listener: (request: UiRequestDto) => void): () => void
  /** Answer a pending extension-UI dialog. */
  respondToUi(response: UiResponseDto): Promise<void>
  /** Read the current tool-approval rules. */
  getApprovalConfig(): Promise<ApprovalConfig>
  /** Replace the tool-approval rules. */
  setApprovalConfig(config: ApprovalConfig): Promise<void>
  /** List saved sessions for the current workspace, newest first. */
  listSessions(): Promise<SessionSummaryDto[]>
  /** List saved sessions across every workspace, newest first. */
  listAllSessions(): Promise<SessionSummaryDto[]>
  /** Open an existing session file. */
  switchSession(path: string): Promise<void>
  /** Set the display name of the current session. */
  renameSession(name: string): Promise<void>
  /** Duplicate the current session into a new one. */
  forkSession(): Promise<void>
  /** Read the current workspace. */
  getWorkspace(): Promise<WorkspaceDto>
  /** Show a folder picker; resolves to null when the user cancels. */
  pickWorkspace(): Promise<string | null>
  /** Switch the agent to a different working directory. */
  setWorkspace(path: string): Promise<WorkspaceDto>
  /** List a directory for the file explorer. */
  listDirectory(path: string): Promise<FsListingDto>
  /** Read a file as text for the editor. */
  readFile(path: string): Promise<FsFileDto>
  /** Write file contents back to disk. */
  writeFile(path: string, content: string): Promise<void>
  /** Format a buffer the way the file's language wants it. */
  formatFile(path: string, content: string): Promise<FormatResultDto>
  /** Workspace-relative paths of every indexable file, sorted. */
  listWorkspaceFiles(): Promise<string[]>
  /** Rename or move a path. Refuses to overwrite an existing one. */
  renamePath(from: string, to: string): Promise<FsResultDto>
  /** Delete a file, or a directory and everything under it. */
  deletePath(path: string): Promise<FsResultDto>
  /** Copy or move paths into a directory. */
  transferPaths(paths: string[], targetDir: string, mode: 'copy' | 'move'): Promise<FsResultDto>
  /** Create an empty file, or a directory. */
  createEntry(path: string, kind: 'file' | 'directory'): Promise<FsResultDto>
  /** Show a path in the operating system's file manager. */
  revealPath(path: string): Promise<void>
  /** Install the `pi` CLI by running its official installer. */
  installPi(): Promise<PiInstallResultDto>
  /** Restart PiUI. */
  relaunchApp(): Promise<void>
  /** Take back the last file operation the explorer performed. */
  undoFileOperation(): Promise<FsResultDto>
  /** Files the agent has changed, still awaiting Keep or Undo. */
  getChanges(): Promise<ChangesStateDto>
  /** Accept one change, or every change when the path is null. */
  keepChanges(path: string | null): Promise<ChangesStateDto>
  /** Revert one change, or every change when the path is null. */
  undoChanges(path: string | null): Promise<FsResultDto>
  /** Record the agent's edits without offering them for review. */
  setAutoKeep(enabled: boolean): Promise<void>
  /** Ask where to write a buffer that has no path yet; null when cancelled. */
  pickSavePath(defaultName?: string): Promise<string | null>
  /** Delete a saved session file. */
  deleteSession(path: string): Promise<void>
  /** Read the current monitor snapshot. */
  getMonitor(): Promise<MonitorSnapshotDto>
  /** Read the provider and model definitions PiUI can edit. */
  getModelsConfig(): Promise<ModelsConfigDto>
  /** Replace the provider and model definitions and reload the agent catalogue. */
  setModelsConfig(config: ModelsConfigDto): Promise<ModelsUpdateResultDto>
  /** Every model a provider publishes, so one can be added without guessing. */
  listProviderModels(provider: string): Promise<ModelDefDto[]>
  /** Start (or re-attach to) a terminal session. */
  terminalCreate(input: TerminalCreateInput): Promise<TerminalSessionDto>
  /** Send keystrokes to a terminal session. */
  terminalWrite(id: string, data: string): Promise<void>
  /** Tell the shell it has a new viewport size. */
  terminalResize(id: string, cols: number, rows: number): Promise<void>
  /** Kill a terminal session. */
  terminalDispose(id: string): Promise<void>
  /** Subscribe to terminal output. Returns an unsubscribe function. */
  onTerminalData(listener: (payload: TerminalDataDto) => void): () => void
  /** Subscribe to terminal exit events. Returns an unsubscribe function. */
  onTerminalExit(listener: (payload: TerminalExitDto) => void): () => void
  /** Search the pi.dev package catalogue. */
  searchPackages(query: string, type: string, page: number): Promise<CatalogPageDto>
  /** List the packages configured in settings. */
  listInstalledPackages(): Promise<InstalledPackageDto[]>
  /** Install a package from the catalogue and persist it. */
  installPackage(source: string): Promise<InstalledPackageDto[]>
  /** Remove a package and forget it in settings. */
  removePackage(source: string): Promise<InstalledPackageDto[]>
  /** Subscribe to install progress. Returns an unsubscribe function. */
  onPackageProgress(listener: (payload: PackageProgressDto) => void): () => void
  /** Current state of the updater. */
  getUpdateState(): Promise<UpdateStateDto>
  /** Ask the release feed whether a newer version exists. */
  checkForUpdates(): Promise<UpdateStateDto>
  /** Download the offered version in the background. */
  downloadUpdate(): Promise<UpdateStateDto>
  /** Quit, install the downloaded version, and relaunch. */
  installUpdate(): Promise<void>
  /** Subscribe to updater state changes. Returns an unsubscribe function. */
  onUpdate(listener: (state: UpdateStateDto) => void): () => void
}
