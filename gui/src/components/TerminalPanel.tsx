import { useEffect, useRef, useState } from 'react'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import '@xterm/xterm/css/xterm.css'

/**
 * The integrated terminal.
 *
 * A real PTY lives in the Rust side; this component is only the view. It keeps
 * one xterm instance for the life of the panel and resizes it with the pane, so
 * a long-running command survives a window resize.
 */

interface TerminalPanelProps {
  /** Called once the terminal is ready, with a writer for incoming output. */
  onReady?: (write: (data: string) => void) => void
  /** Called when the user types, with the raw bytes. */
  onInput?: (data: string) => void
  /** Called when the pane is resized, with the new dimensions. */
  onResize?: (cols: number, rows: number) => void
}

export function TerminalPanel({ onReady, onInput, onResize }: TerminalPanelProps) {
  const hostRef = useRef<HTMLDivElement | null>(null)
  const termRef = useRef<Terminal | null>(null)
  const fitRef = useRef<FitAddon | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    const host = hostRef.current
    if (!host) return

    const term = new Terminal({
      fontFamily: "'JetBrains Mono', ui-monospace, Consolas, monospace",
      fontSize: 12.5,
      lineHeight: 1.35,
      cursorBlink: true,
      convertEol: true,
      scrollback: 5000,
      theme: {
        background: '#16141c',
        foreground: '#e2e1e6',
        cursor: '#c08ae0',
        selectionBackground: '#242c4a',
        black: '#16141c',
        red: '#e88a7a',
        green: '#7dd6a8',
        yellow: '#e0d07a',
        blue: '#8aa8e0',
        magenta: '#c08ae0',
        cyan: '#7dd6d6',
        white: '#e2e1e6'
      }
    })
    const fit = new FitAddon()
    term.loadAddon(fit)

    try {
      term.open(host)
      // xterm measures its container on open; a pane that has not been laid out
      // yet reports zero size and throws. Fit on the next frame instead.
      requestAnimationFrame(() => {
        try {
          fit.fit()
        } catch {
          // The observer below will fit once the pane has a size.
        }
      })
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause))
      return
    }

    termRef.current = term
    fitRef.current = fit

    const disposable = term.onData((data) => onInput?.(data))
    onReady?.((data) => term.write(data))
    onResize?.(term.cols, term.rows)

    // Resize with the pane, coalesced to one fit per frame.
    let frame = 0
    const observer = new ResizeObserver(() => {
      cancelAnimationFrame(frame)
      frame = requestAnimationFrame(() => {
        try {
          fit.fit()
          onResize?.(term.cols, term.rows)
        } catch {
          // A zero-sized pane cannot be fitted; the next resize will succeed.
        }
      })
    })
    observer.observe(host)

    return () => {
      cancelAnimationFrame(frame)
      observer.disconnect()
      disposable.dispose()
      term.dispose()
      termRef.current = null
      fitRef.current = null
    }
    // The callbacks are stable for the panel's lifetime; re-creating the
    // terminal on every render would lose the scrollback.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  if (error) {
    return (
      <div className="term term--error">
        <p>The terminal could not start: {error}</p>
      </div>
    )
  }

  return <div className="term" ref={hostRef} />
}
