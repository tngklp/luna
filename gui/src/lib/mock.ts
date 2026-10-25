import type {
  AgentEventDto,
  AppInfo,
  ApprovalConfig,
  ChatItemDto,
  ChangesStateDto,
  CommandDto,
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
import type { LunaApi } from './bridge'

/**
 * A stand-in for the Rust core, used when the renderer runs in a plain browser.
 *
 * `npm run dev` opens the real UI without Tauri, which is how the chat surface is
 * developed and how its layout is checked. The mock answers with plausible data
 * and streams a canned reply, so every component exercises its real code path.
 */

const listeners = {
  event: new Set<(event: AgentEventDto) => void>(),
  notice: new Set<(notice: NoticeDto) => void>(),
  ui: new Set<(request: UiRequestDto) => void>(),
  update: new Set<(update: UpdateStateDto) => void>()
}

function emitEvent(event: AgentEventDto): void {
  for (const listener of listeners.event) listener(event)
}

const cost = { input: 3, output: 15, cacheRead: 0.3, cacheWrite: 3.75 }

const models: ModelDto[] = [
  {
    provider: 'anthropic',
    id: 'claude-sonnet-4-5',
    name: 'Claude Sonnet 4.5',
    reasoning: true,
    contextWindow: 200_000,
    maxTokens: 64_000,
    input: ['text', 'image'],
    cost
  },
  {
    provider: 'openai',
    id: 'gpt-5',
    name: 'GPT-5',
    reasoning: true,
    contextWindow: 400_000,
    maxTokens: 128_000,
    input: ['text', 'image'],
    cost: { input: 1.25, output: 10, cacheRead: 0.125, cacheWrite: 0 }
  },
  {
    provider: 'local',
    id: 'llama3.2',
    name: 'llama3.2',
    reasoning: false,
    contextWindow: 128_000,
    maxTokens: 16_384,
    input: ['text'],
    cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 }
  }
]

const modelDefs: ModelDefDto[] = models.map((entry) => ({
  id: entry.id,
  name: entry.name,
  reasoning: entry.reasoning,
  input: entry.input,
  contextWindow: entry.contextWindow,
  maxTokens: entry.maxTokens,
  cost: entry.cost
}))

const commands: CommandDto[] = [
  { name: '/new', description: 'Start a new session', kind: 'builtin' },
  { name: '/compact', description: 'Summarise the conversation', kind: 'builtin' },
  {
    name: '/model',
    description: 'Switch the model',
    kind: 'builtin',
    argumentHint: '<provider/model>'
  }
]

let items: ChatItemDto[] = []
let streaming = false
let thinking: ThinkingLevelDto = 'medium'
let model = models[0]
let counter = 0

const workspace: WorkspaceDto = { cwd: '/home/dev/luna', name: 'luna' }

const sessions: SessionSummaryDto[] = [
  {
    path: '/home/dev/.local/share/luna/sessions/demo.jsonl',
    id: 'demo',
    cwd: workspace.cwd,
    name: 'Refactor the parser',
    created: new Date(Date.now() - 86_400_000).toISOString(),
    modified: new Date(Date.now() - 3_600_000).toISOString(),
    messageCount: 12,
    firstMessage: 'Refactor the parser',
    parentSessionPath: null
  }
]

const modelsConfig: ModelsConfigDto = {
  path: '/home/dev/.luna/config.toml',
  defaultModel: 'anthropic/claude-sonnet-4-5',
  providers: [
    {
      id: 'anthropic',
      preset: 'anthropic',
      kind: 'anthropic',
      baseUrl: 'https://api.anthropic.com',
      apiKeyEnv: 'ANTHROPIC_API_KEY',
      discoverModels: true,
      models: [modelDefs[0]]
    },
    {
      id: 'openai',
      preset: 'openai',
      kind: 'openai',
      baseUrl: 'https://api.openai.com/v1',
      apiKeyEnv: 'OPENAI_API_KEY',
      discoverModels: true,
      models: [modelDefs[1]]
    },
    {
      id: 'ollama',
      preset: 'ollama',
      kind: 'openai',
      baseUrl: 'http://localhost:11434/v1',
      apiKeyEnv: null,
      discoverModels: true,
      models: [modelDefs[2]]
    }
  ]
}

const idleUpdate: UpdateStateDto = {
  phase: 'idle',
  version: null,
  currentVersion: '0.1.0',
  percent: null,
  message: null,
  notes: null,
  canInstall: false
}

function status(): SessionStatusDto {
  return {
    cwd: workspace.cwd,
    model,
    thinkingLevel: thinking,
    availableThinkingLevels: ['off', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max'],
    supportsThinking: model.reasoning,
    isStreaming: streaming,
    isCompacting: false,
    isRetrying: false,
    sessionId: 'demo',
    sessionFile: sessions[0].path,
    sessionName: 'Refactor the parser',
    messageCount: items.length,
    pendingMessageCount: 0,
    steering: [],
    followUp: [],
    contextUsage: { tokens: 18_400, contextWindow: model.contextWindow, percent: 9.2 },
    cost: 0.0412,
    tokens: { input: 18_400, output: 2_100, cacheRead: 9_000, cacheWrite: 0, total: 20_500 }
  }
}

/** Stream a canned reply so the transcript, markdown and tool cards all render. */
function streamReply(prompt: string): void {
  streaming = true
  const id = `a${++counter}`
  items = [...items, { kind: 'user', id: `u${counter}`, text: prompt, imageCount: 0 }]

  emitEvent({ type: 'message_start', message: { role: 'assistant' } })

  const reply = [
    'Here is what I found in the parser.\n\n',
    'The tokenizer drops trailing whitespace, which is why the last token ',
    'in a line is misaligned. I will fix it in `src/parser/tokenize.rs`.\n\n',
    '```rust\nfn tokenize(input: &str) -> Vec<Token> {\n    input\n        .split_whitespace()\n        .map(Token::from)\n        .collect()\n}\n```\n\n',
    'Then I will run the tests.'
  ]

  let index = 0
  const tick = (): void => {
    if (index >= reply.length) {
      emitEvent({
        type: 'tool_execution_start',
        toolCallId: 'call_1',
        toolName: 'bash',
        args: { command: 'cargo test -p parser' }
      })
      emitEvent({
        type: 'tool_execution_update',
        toolCallId: 'call_1',
        toolName: 'bash',
        args: { command: 'cargo test -p parser' },
        partialResult: { content: [{ type: 'text', text: 'running 14 tests\n' }] }
      })
      window.setTimeout(() => {
        emitEvent({
          type: 'tool_execution_update',
          toolCallId: 'call_1',
          toolName: 'bash',
          args: { command: 'cargo test -p parser' },
          partialResult: { content: [{ type: 'text', text: 'test result: ok. 14 passed\n' }] }
        })
        emitEvent({ type: 'tool_execution_end', toolCallId: 'call_1', isError: false })
        items = [
          ...items,
          {
            kind: 'assistant',
            id,
            blocks: [{ type: 'text', text: reply.join('') }],
            entryId: id
          },
          {
            kind: 'bash',
            id: 'b1',
            command: 'cargo test -p parser',
            output: 'running 14 tests\ntest result: ok. 14 passed',
            exitCode: 0
          }
        ]
        streaming = false
        emitEvent({ type: 'message_end' })
        emitEvent({ type: 'agent_settled' })
      }, 700)
      return
    }
    emitEvent({
      type: 'message_update',
      assistantMessageEvent: { type: 'text_delta', delta: reply[index] }
    })
    index += 1
    window.setTimeout(tick, 18)
  }
  tick()
}

export function createMockApi(): LunaApi {
  return {
    getAppInfo: async (): Promise<AppInfo> => ({
      name: 'Luna',
      version: '0.1.0',
      platform: 'browser',
      versions: { electron: 'dev', chrome: 'dev', node: 'dev', v8: 'dev' }
    }),
    getRuntimeInfo: async (): Promise<RuntimeInfoDto> => ({
      sdkVersion: '0.1.0',
      cliVersion: null,
      cliPath: null,
      cliInstalled: true,
      versionMatch: true,
      agentDir: '/home/dev/.local/share/luna',
      node: 'dev',
      sdkPath: null
    }),
    getStatus: async () => status(),
    getMessages: async () => items,
    getModels: async () => models,
    getModelDefs: async () => modelDefs,
    getCommands: async () => commands,

    prompt: async (input: PromptInput) => {
      streamReply(input.text)
    },
    steer: async () => undefined,
    followUp: async () => undefined,
    abort: async () => {
      streaming = false
      emitEvent({ type: 'agent_settled' })
    },
    newSession: async () => {
      items = []
      streaming = false
    },
    compact: async () => undefined,
    editMessage: async (_entryId: string, text: string) => {
      items = []
      streamReply(text)
    },
    retryMessage: async () => undefined,
    setModel: async (provider: string, id: string) => {
      model = models.find((entry) => entry.provider === provider && entry.id === id) ?? model
    },
    setThinkingLevel: async (level: ThinkingLevelDto) => {
      thinking = level
    },

    onAgentEvent: (listener) => {
      listeners.event.add(listener)
      return () => listeners.event.delete(listener)
    },
    onNotice: (listener) => {
      listeners.notice.add(listener)
      return () => listeners.notice.delete(listener)
    },
    onUiRequest: (listener) => {
      listeners.ui.add(listener)
      return () => listeners.ui.delete(listener)
    },
    onUpdate: (listener) => {
      listeners.update.add(listener)
      return () => listeners.update.delete(listener)
    },
    respondToUi: async (_response: UiResponseDto) => undefined,

    getApprovalConfig: async (): Promise<ApprovalConfig> => ({
      defaultPolicy: 'ask',
      tools: { read: 'allow', write: 'ask', bash: 'ask', network: 'allow' }
    }),
    setApprovalConfig: async () => undefined,

    listSessions: async () => sessions,
    listAllSessions: async () => sessions,
    switchSession: async () => undefined,
    renameSession: async () => undefined,
    forkSession: async () => undefined,
    deleteSession: async () => undefined,

    getWorkspace: async () => workspace,
    pickWorkspace: async () => null,
    setWorkspace: async () => workspace,

    getChanges: async (): Promise<ChangesStateDto> => ({ changes: [], revision: 0 }),
    keepChanges: async (): Promise<ChangesStateDto> => ({ changes: [], revision: 0 }),
    undoChanges: async () => undefined,

    getModelsConfig: async () => modelsConfig,
    setModelsConfig: async (): Promise<ModelsUpdateResultDto> => ({
      config: modelsConfig,
      models,
      error: null
    }),

    revealPath: async () => undefined,
    readFile: async (path: string) => ({ path, content: '', error: null }),

    getUpdateState: async () => idleUpdate,
    checkForUpdates: async () => idleUpdate,
    downloadUpdate: async () => idleUpdate,
    installUpdate: async () => undefined
  }
}
