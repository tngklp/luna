import { useEffect, useRef, useState, type RefObject } from 'react'
import type { ChatBlockDto, ChatItemDto, UiRequestDto } from '@shared/ipc'
import { answerMarkdown, copyText } from '../lib/clipboard'
import { formatDuration, toolDescription } from '../lib/format'
import { streamedPreview, streamedToolDescription } from '../lib/partial-args'
import { useLuna, type RunningTool } from '../store'
import { Collapsible } from './Collapsible'
import { DiffView } from './DiffView'
import { Markdown } from './Markdown'

/**
 * Scroll position of the transcript, kept outside React so it survives the tab
 * switch that unmounts this component.
 *
 * `atBottom` is stored rather than inferred on the way back, so a conversation
 * that grew while the chat was hidden still lands at the end.
 */
const scrollMemory = { top: 0, atBottom: true }

/** Distance from the bottom, in pixels, that still counts as "at the bottom". */
const BOTTOM_SLACK = 64

/** Reasoning block with an animated open/close. */
function Thinking({ text, live }: { text: string; live?: boolean }) {
  const expandThinking = useLuna((state) => state.prefs.expandThinking)
  const [open, setOpen] = useState(Boolean(live) || expandThinking)

  return (
    <div className="think">
      <button
        className="think__summary"
        aria-expanded={open}
        onClick={() => setOpen((wasOpen) => !wasOpen)}
      >
        <span className={`chev${open ? ' o' : ''}`}>›</span>
        {live ? 'Thinking…' : 'Thought'}
      </button>
      <Collapsible open={open}>
        <pre>{text}</pre>
      </Collapsible>
    </div>
  )
}

/**
 * Tool call and its result in one line, expanded on click. A finished call is
 * collapsed, so a long run reads as a list of what happened rather than a wall
 * of output.
 */
function ToolCard({ item }: { item: Extract<ChatItemDto, { kind: 'tool' }> }) {
  const expandToolOutput = useLuna((state) => state.prefs.expandToolOutput)
  const [open, setOpen] = useState(expandToolOutput)

  const args = (item.arguments ?? {}) as Record<string, unknown>
  const command = typeof args.command === 'string' ? args.command : null
  const description = toolDescription(item.name, item.arguments)

  return (
    <div className="card">
      <button className="hd" aria-expanded={open} onClick={() => setOpen((was) => !was)}>
        <span className={`chev${open ? ' o' : ''}`}>›</span>
        <span className="tool__desc" title={command ?? description}>
          {description}
        </span>
        <span className="tag">{item.name}</span>

        <span className="hd__end">
          {item.addedLines ? <span className="ok">+{item.addedLines}</span> : null}
          {item.removedLines ? <span className="del">−{item.removedLines}</span> : null}
          {item.running ? (
            <span className="spinner" />
          ) : (
            <>
              <span className={item.isError ? 'del' : 'ok'}>{item.isError ? '✕' : '✓'}</span>
              {item.durationMs ? (
                <span className="took">{formatDuration(item.durationMs)}</span>
              ) : null}
            </>
          )}
        </span>
      </button>

      <Collapsible open={open}>
        {command ? <pre className="cmd__full">{command}</pre> : null}
        {item.diff ? <DiffView diff={item.diff} /> : null}
        {!item.diff && item.text ? <pre className="out">{item.text}</pre> : null}
        {!item.diff && !item.text && item.running ? <pre className="out">running…</pre> : null}
      </Collapsible>
    </div>
  )
}

/**
 * A tool call the model is still writing out.
 *
 * The arguments are not valid JSON yet, so the header has to be built from a
 * tolerant read of the text rather than from parsed arguments. For a `write` or
 * an `edit` there is no result to stream — the file only lands at the end — so
 * the body previews the content being produced, which is the part that is
 * actually arriving. Showing the raw argument text instead, as this used to, left
 * a card reading `{"edits":`.
 */
function StreamingToolCard({ tool }: { tool: { id: string; name: string; argsText: string } }) {
  const [open, setOpen] = useState(true)
  const description = streamedToolDescription(tool.name, tool.argsText)
  const preview = streamedPreview(tool.name, tool.argsText)
  const outputRef = useFollowOutput(preview)

  return (
    <div className="card">
      <button className="hd" aria-expanded={open} onClick={() => setOpen((was) => !was)}>
        <span className={`chev${open ? ' o' : ''}`}>›</span>
        <span className="tool__desc" title={description}>
          {description}
        </span>
        <span className="tag">{tool.name}</span>
        <span className="hd__end">
          <span className="spinner" />
        </span>
      </button>

      <Collapsible open={open}>
        {preview ? (
          <pre className="out" ref={outputRef}>
            {preview}
          </pre>
        ) : null}
      </Collapsible>
    </div>
  )
}

/**
 * A tool that is still running. Output streams in through `tool_execution_update`,
 * so the card is expanded while the command runs and collapses once it lands in
 * the transcript as a finished card.
 */
function RunningToolCard({ tool }: { tool: RunningTool }) {
  const [open, setOpen] = useState(true)
  const args = (tool.args ?? {}) as Record<string, unknown>
  const command = typeof args.command === 'string' ? args.command : null
  const description = toolDescription(tool.name, tool.args)
  const outputRef = useFollowOutput(tool.text ?? '')

  return (
    <div className="card">
      <button className="hd" aria-expanded={open} onClick={() => setOpen((was) => !was)}>
        <span className={`chev${open ? ' o' : ''}`}>›</span>
        <span className="tool__desc" title={command ?? description}>
          {description}
        </span>
        <span className="tag">{tool.name}</span>
        <span className="hd__end">
          <span className="spinner" />
        </span>
      </button>

      <Collapsible open={open}>
        {command ? <pre className="cmd__full">{command}</pre> : null}
        <pre className="out" ref={outputRef}>
          {tool.text ? tool.text : 'running…'}
        </pre>
      </Collapsible>
    </div>
  )
}

/** Icons for the actions under a message. Sized to sit beside 12px labels. */
const ICONS = {
  copy: 'M6 2.6h5.2L13.4 4.8V11a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V3.6a1 1 0 0 1 1-1ZM3 6v7a1 1 0 0 0 1 1h5',
  edit: 'M10.5 3.2 12.8 5.5 6.4 11.9l-3 .6.6-3 6.5-6.3Z',
  retry: 'M13 8a5 5 0 1 1-1.6-3.7M13 2.8V6h-3.2'
} as const

function ActionIcon({ path }: { path: string }) {
  return (
    <svg
      width="13"
      height="13"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d={path} />
    </svg>
  )
}

/**
 * Copy / Edit / Try again, sitting under the message they belong to.
 *
 * Copy is what people reach for most, so it is the one that confirms itself
 * rather than opening something.
 */
function MessageActions({
  markdown,
  disabled,
  onEdit,
  onRetry
}: {
  markdown: string
  /** A turn is in flight, so editing or retrying would race it. */
  disabled?: boolean
  onEdit?: () => void
  onRetry?: () => void
}) {
  const [copied, setCopied] = useState(false)

  const copy = async (): Promise<void> => {
    if (!(await copyText(markdown))) return
    setCopied(true)
  }

  // The confirmation is the button's own label, so it has to revert on its own.
  useEffect(() => {
    if (!copied) return
    const timer = setTimeout(() => setCopied(false), 1600)
    return () => clearTimeout(timer)
  }, [copied])

  return (
    <div className="macts">
      <button
        className={`mact${copied ? ' done' : ''}`}
        onClick={() => void copy()}
        title="Copy as markdown"
      >
        <ActionIcon path={ICONS.copy} />
        {copied ? 'Copied' : 'Copy'}
      </button>

      {onEdit ? (
        <button
          className="mact"
          disabled={disabled}
          onClick={onEdit}
          title="Edit and resend this message"
        >
          <ActionIcon path={ICONS.edit} />
          Edit
        </button>
      ) : null}

      {onRetry ? (
        <button className="mact" disabled={disabled} onClick={onRetry} title="Run this turn again">
          <ActionIcon path={ICONS.retry} />
          Try again
        </button>
      ) : null}
    </div>
  )
}

/**
 * Keep a scrolling output box pinned to the bottom as text arrives.
 *
 * The box has its own scrollbar (a `write` of a long file would otherwise push
 * the rest of the transcript off screen), so without this the newest lines are
 * below the fold and the card appears frozen while it is actually working.
 */
function useFollowOutput(dependency: string): RefObject<HTMLPreElement | null> {
  const ref = useRef<HTMLPreElement | null>(null)

  useEffect(() => {
    const node = ref.current
    if (node) node.scrollTop = node.scrollHeight
  }, [dependency])

  return ref
}

/**
 * Copy and Try again belong to the message the agent finished last.
 *
 * On every reply they turn a long conversation into a column of identical
 * buttons, and "try again" only ever means the most recent turn — the older ones
 * have been answered over. The user's own messages are different: editing an
 * older one is a real thing to want, so those keep their actions.
 */
function AssistantItem({
  item,
  showActions
}: {
  item: Extract<ChatItemDto, { kind: 'assistant' }>
  showActions: boolean
}) {
  const retryMessage = useLuna((state) => state.retryMessage)
  const busy = useLuna((state) => state.busy)
  const entryId = item.entryId

  return (
    <div className="asst">
      {item.blocks.map((block: ChatBlockDto, index) => {
        if (block.type === 'text') {
          return <Markdown text={block.text} key={`text-${index}`} />
        }
        if (block.type === 'thinking') {
          return <Thinking text={block.text} key={`think-${index}`} />
        }
        return null
      })}
      {item.stopped ? <p className="note">Stopped by user</p> : null}
      {item.error ? <p className="note bad">{item.error}</p> : null}

      {showActions ? (
        <MessageActions
          markdown={answerMarkdown(item.blocks)}
          disabled={busy}
          {...(entryId ? { onRetry: () => void retryMessage(entryId) } : {})}
        />
      ) : null}
    </div>
  )
}

function UserItem({ item }: { item: Extract<ChatItemDto, { kind: 'user' }> }) {
  const beginEdit = useLuna((state) => state.beginEdit)
  const busy = useLuna((state) => state.busy)
  const entryId = item.entryId

  return (
    <div className="turn">
      <div className="user">
        {item.text}
        {item.imageCount > 0 ? (
          <div className="user__meta">
            {item.imageCount} image{item.imageCount === 1 ? '' : 's'}
          </div>
        ) : null}
      </div>

      <MessageActions
        markdown={item.text}
        disabled={busy}
        {...(entryId ? { onEdit: () => beginEdit({ id: item.id, entryId, text: item.text }) } : {})}
      />
    </div>
  )
}

function TranscriptItem({ item, showActions }: { item: ChatItemDto; showActions: boolean }) {
  switch (item.kind) {
    case 'user':
      return <UserItem item={item} />

    case 'assistant':
      return <AssistantItem item={item} showActions={showActions} />

    case 'tool':
      return <ToolCard item={item} />

    case 'bash':
      return (
        <div className="card">
          <div className="hd">
            <span className="tag">bash</span>
            <span className="cmd__text mono">{item.command}</span>
            <span className="hd__end">
              <span className={item.exitCode === 0 ? 'ok' : 'del'}>
                {item.exitCode === 0 ? '✓' : `exit ${item.exitCode ?? '?'}`}
              </span>
            </span>
          </div>
          <pre className="out">{item.output || '(no output)'}</pre>
        </div>
      )

    default:
      return null
  }
}

/** Inline approval prompt for a tool the agent wants to run. */
function ApprovalCard({ request }: { request: Extract<UiRequestDto, { method: 'approval' }> }) {
  const respond = useLuna((state) => state.respondToDialog)

  return (
    <div className="card appro">
      <div className="hd">
        <span className="tag">{request.tool}</span>
        <span className="appro__title">{request.title}</span>
        <span className="hd__end">
          <span className="appro__wait">waiting</span>
        </span>
      </div>

      <pre className="cmd__full">{request.detail}</pre>

      <div className="appro__acts">
        <button
          className="b pri"
          autoFocus
          onClick={() => void respond({ id: request.id, decision: 'allow' })}
        >
          Allow
        </button>
        <button className="b" onClick={() => void respond({ id: request.id, decision: 'deny' })}>
          Deny
        </button>
      </div>
    </div>
  )
}

/** Scrolling transcript with a live streaming bubble. */
export function Transcript() {
  const items = useLuna((state) => state.items)
  const streaming = useLuna((state) => state.streaming)
  const runningTools = useLuna((state) => state.runningTools)
  const dialog = useLuna((state) => state.dialog)
  const endRef = useRef<HTMLDivElement | null>(null)
  const colRef = useRef<HTMLDivElement | null>(null)
  /** Whether the transcript was pinned to the bottom before the last update. */
  const pinned = useRef(scrollMemory.atBottom)
  /** Drives the jump-to-end button, which only shows once the user scrolls away. */
  const [showJump, setShowJump] = useState(false)
  const sessionId = useLuna((state) => state.status?.sessionId)
  /** The session the scroll memory belongs to, so a switch can reset it once. */
  const rememberedSession = useRef(sessionId)

  /**
   * Recompute the bottom state and whether there is anywhere to jump to. A
   * transcript that already fits on screen has no bottom, so the button hides
   * itself on an empty session instead of hovering over nothing.
   */
  const syncScroll = (): void => {
    const scroller = colRef.current?.closest('.scroll')
    if (!scroller) return

    const distance = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight
    const scrollable = scroller.scrollHeight - scroller.clientHeight > BOTTOM_SLACK
    pinned.current = distance < BOTTOM_SLACK
    scrollMemory.top = scroller.scrollTop
    scrollMemory.atBottom = pinned.current
    setShowJump(scrollable && !pinned.current)
  }

  // Track the scroll position of the surrounding `.scroll` container so new
  // output only follows the bottom when the user is already there. The position
  // is remembered across the unmount that happens when another tab is shown.
  useEffect(() => {
    const scroller = colRef.current?.closest('.scroll')
    if (!scroller) return

    // Coming back to a transcript that was at the bottom should end at the
    // bottom again, even if the conversation grew while the chat was hidden.
    scroller.scrollTop = scrollMemory.atBottom ? scroller.scrollHeight : scrollMemory.top
    pinned.current = scrollMemory.atBottom
    syncScroll()

    scroller.addEventListener('scroll', syncScroll, { passive: true })
    return () => scroller.removeEventListener('scroll', syncScroll)
    // The listener only reads refs and setters, so it is safe to bind once.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // A different session starts at the top: a saved offset belongs to a
  // conversation that is no longer on screen. Skipped on mount, so switching
  // tabs still restores where the reader was.
  useEffect(() => {
    if (rememberedSession.current === sessionId) return
    rememberedSession.current = sessionId
    scrollMemory.top = 0
    scrollMemory.atBottom = true
    pinned.current = true
    const scroller = colRef.current?.closest('.scroll')
    if (scroller) scroller.scrollTop = 0
    setShowJump(false)
  }, [sessionId])

  useEffect(() => {
    if (!pinned.current) return
    endRef.current?.scrollIntoView({ block: 'end' })
    syncScroll()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [items, streaming, runningTools, dialog])

  const isEmpty = items.length === 0 && !streaming && runningTools.length === 0

  /**
   * A tool call that already has a card of its own must not also be drawn from
   * `streaming.tools`. That overlap is what showed the same call twice: once
   * while the model was emitting it and again once it was executing.
   *
   * Counted rather than looked up in a set, because a provider is free to omit
   * tool-call ids and several of them then share the same empty one. A set would
   * collapse those into a single entry and suppress the wrong cards; counting
   * pairs each live card off against one pending call.
   */
  const liveCalls = new Map<string, number>()
  for (const tool of runningTools) {
    liveCalls.set(tool.id, (liveCalls.get(tool.id) ?? 0) + 1)
  }
  const pendingTools = (streaming?.tools ?? []).filter((tool) => {
    const remaining = liveCalls.get(tool.id) ?? 0
    if (remaining === 0) return true
    liveCalls.set(tool.id, remaining - 1)
    return false
  })

  /**
   * A tool that is executing has a live card at the end of the transcript, so the
   * placeholder the message carries for the same call is hidden while it runs.
   * Otherwise the call is drawn twice: once in message order, once live.
   */
  const liveToolIds = new Set(runningTools.map((tool) => tool.id))
  const visibleItems = items.filter(
    (item) => !(item.kind === 'tool' && item.running && liveToolIds.has(item.toolCallId))
  )

  /**
   * The reply that carries Copy and Try again.
   *
   * Nothing carries them while one is being written: the last thing on screen is
   * then the live bubble, so buttons on the message above it would not be on the
   * last message, which is the whole point.
   */
  const lastAssistantId = streaming
    ? null
    : ([...visibleItems].reverse().find((item) => item.kind === 'assistant')?.id ?? null)

  return (
    <div className="col" ref={colRef}>
      {isEmpty ? (
        <div className="empty">
          <b>Ask Luna to work on your project.</b>
          Message Luna below. Type <code>/</code> for commands.
        </div>
      ) : null}

      {visibleItems.map((item) => (
        <TranscriptItem item={item} key={item.id} showActions={item.id === lastAssistantId} />
      ))}

      {runningTools.map((tool) => (
        <RunningToolCard tool={tool} key={tool.id} />
      ))}

      {streaming ? (
        <div className="asst">
          {streaming.thinking ? (
            <Thinking text={streaming.thinking} live={streaming.thinkingLive} />
          ) : null}
          {streaming.text ? <Markdown text={streaming.text} /> : null}
          {pendingTools.map((tool) => (
            <StreamingToolCard tool={tool} key={tool.id || tool.name} />
          ))}
          <p className="stream" />
        </div>
      ) : null}

      {dialog && dialog.method === 'approval' ? <ApprovalCard request={dialog} /> : null}

      <div ref={endRef} />

      {showJump ? (
        <button
          className="jump"
          title="Jump to the latest"
          aria-label="Jump to the latest"
          onClick={() => {
            const scroller = colRef.current?.closest('.scroll')
            if (scroller) scroller.scrollTop = scroller.scrollHeight
            pinned.current = true
            scrollMemory.atBottom = true
            setShowJump(false)
          }}
        >
          <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor">
            <path d="M8 3v10M3.5 8.5 8 13l4.5-4.5" strokeWidth="1.8" strokeLinecap="round" />
          </svg>
        </button>
      ) : null}
    </div>
  )
}
