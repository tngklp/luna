/**
 * Persisted interface preferences.
 *
 * These are all renderer-side and read synchronously on startup, so they use
 * `localStorage` rather than going through the main process.
 */
import { DEFAULT_ICON_PACK, findIconPack } from './icon-packs'

export interface UiPreferences {
  /** Editor font size in pixels. */
  editorFontSize: number
  /** Wrap long lines in the editor. */
  editorWrap: boolean
  /** Editor font stack; blank falls back to the bundled monospace face. */
  editorFontFamily: string
  /** Spaces per indent level, or 0 to use the language's own convention. */
  editorTabSize: number
  /** Draw a guide at each indent level. */
  editorIndentGuides: boolean
  /** Run Prettier over the buffer before writing the file. */
  editorFormatOnSave: boolean
  /** Save on a pause in typing, without waiting for Mod-s. */
  editorAutoSave: boolean
  /** How long the pause has to last. */
  editorAutoSaveDelayMs: number
  /** Expand reasoning blocks in the transcript by default. */
  expandThinking: boolean
  /** Show what a tool ran with, instead of keeping it collapsed. */
  expandToolOutput: boolean
  /**
   * Record the agent's edits without offering them for review. On by default:
   * the review step is opt-in, and with it off the Changes tab, the pending bar
   * and the editor's change marks are all absent rather than merely empty.
   */
  autoKeepEdits: boolean
  /** Enter sends the prompt; Ctrl+Enter (or Shift+Enter) is a newline. */
  sendOnEnter: boolean
  /** Icon pack id used by the file explorer and editor tabs. */
  iconPack: string
  /** Shell the integrated terminal runs; empty picks a sensible default. */
  terminalShell: string
  /** Lines of scrollback the terminal keeps. */
  terminalScrollback: number
}

const STORAGE_KEY = 'luna.prefs'

export const EDITOR_FONT_MIN = 10
export const EDITOR_FONT_MAX = 24

export const SCROLLBACK_MIN = 100
export const SCROLLBACK_MAX = 100000

/** Indent widths offered in Settings; 0 means "follow the language". */
export const TAB_SIZE_OPTIONS = [0, 2, 4, 8]

export const AUTOSAVE_MIN_MS = 500
export const AUTOSAVE_MAX_MS = 10000

/** Bundled monospace stack, used when no family is set. */
export const DEFAULT_EDITOR_FONT = 'JetBrains Mono, ui-monospace, monospace'

export const DEFAULT_UI_PREFERENCES: UiPreferences = {
  editorFontSize: 13,
  editorWrap: false,
  editorFontFamily: '',
  editorTabSize: 0,
  editorIndentGuides: true,
  editorFormatOnSave: false,
  editorAutoSave: false,
  editorAutoSaveDelayMs: 1200,
  expandThinking: false,
  expandToolOutput: false,
  autoKeepEdits: true,
  sendOnEnter: true,
  iconPack: DEFAULT_ICON_PACK,
  terminalShell: '',
  terminalScrollback: 2000
}

function clampFontSize(value: unknown): number {
  const parsed = typeof value === 'number' ? value : Number(value)
  if (!Number.isFinite(parsed)) return DEFAULT_UI_PREFERENCES.editorFontSize
  return Math.min(EDITOR_FONT_MAX, Math.max(EDITOR_FONT_MIN, Math.round(parsed)))
}

/** Keep the scrollback inside what xterm can hold comfortably. */
export function clampScrollback(value: unknown): number {
  const parsed = typeof value === 'number' ? value : Number(value)
  if (!Number.isFinite(parsed) || parsed <= 0) return DEFAULT_UI_PREFERENCES.terminalScrollback
  return Math.min(SCROLLBACK_MAX, Math.max(SCROLLBACK_MIN, Math.round(parsed)))
}

/** Indent width, or 0 for the language default. */
export function clampTabSize(value: unknown): number {
  const parsed = typeof value === 'number' ? value : Number(value)
  return TAB_SIZE_OPTIONS.includes(parsed) ? parsed : 0
}

export function clampAutoSaveDelay(value: unknown): number {
  const parsed = typeof value === 'number' ? value : Number(value)
  if (!Number.isFinite(parsed)) return DEFAULT_UI_PREFERENCES.editorAutoSaveDelayMs
  return Math.min(AUTOSAVE_MAX_MS, Math.max(AUTOSAVE_MIN_MS, Math.round(parsed)))
}

/** Coerce anything stored into a complete, valid preference set. */
function normalize(value: unknown): UiPreferences {
  const raw = value !== null && typeof value === 'object' ? (value as Record<string, unknown>) : {}
  const bool = (key: keyof UiPreferences, fallback: boolean): boolean =>
    typeof raw[key] === 'boolean' ? (raw[key] as boolean) : fallback

  return {
    editorFontSize: clampFontSize(raw.editorFontSize),
    editorWrap: bool('editorWrap', false),
    editorFontFamily: typeof raw.editorFontFamily === 'string' ? raw.editorFontFamily : '',
    editorTabSize: clampTabSize(raw.editorTabSize),
    editorIndentGuides: bool('editorIndentGuides', true),
    editorFormatOnSave: bool('editorFormatOnSave', false),
    editorAutoSave: bool('editorAutoSave', false),
    editorAutoSaveDelayMs: clampAutoSaveDelay(raw.editorAutoSaveDelayMs),
    expandThinking: bool('expandThinking', false),
    expandToolOutput: bool('expandToolOutput', false),
    autoKeepEdits: bool('autoKeepEdits', true),
    sendOnEnter: bool('sendOnEnter', true),
    iconPack: findIconPack(typeof raw.iconPack === 'string' ? raw.iconPack : '').id,
    terminalShell: typeof raw.terminalShell === 'string' ? raw.terminalShell : '',
    terminalScrollback: clampScrollback(raw.terminalScrollback)
  }
}

export function loadUiPreferences(): UiPreferences {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    return raw === null ? { ...DEFAULT_UI_PREFERENCES } : normalize(JSON.parse(raw))
  } catch {
    return { ...DEFAULT_UI_PREFERENCES }
  }
}

export function saveUiPreferences(preferences: UiPreferences): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(preferences))
  } catch {
    // Storage may be unavailable; the preferences still apply for this session.
  }
}

export { clampFontSize }
