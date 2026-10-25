import { useEffect, useState } from 'react'
import { CommandPalette } from './components/CommandPalette'
import { DialogHost } from './components/DialogHost'
import { MainPane } from './components/MainPane'
import { SettingsDialog } from './components/SettingsDialog'
import { ShortcutSheet } from './components/ShortcutSheet'
import { Sidebar } from './components/Sidebar'
import { StatusFooter } from './components/StatusFooter'
import { TerminalPanel } from './components/TerminalPanel'
import { UpdateToast } from './components/UpdateToast'
import { useLuna } from './store'

/**
 * The application shell: sidebar, main column, status strip, and the overlays.
 *
 * Luna opens straight into the chat. piui's shell also mounted a welcome
 * overlay, a right panel, a settings dialog and a quick-open picker; the welcome
 * screen is gone and the rest serve panes Luna does not ship.
 */
export default function App() {
  const initialize = useLuna((state) => state.initialize)
  const newSession = useLuna((state) => state.newSession)
  const focusSearch = useLuna((state) => state.focusSearch)
  const openQuickOpen = useLuna((state) => state.openQuickOpen)
  const openShortcuts = useLuna((state) => state.openShortcuts)
  const toggleTerminal = useLuna((state) => state.toggleTerminal)
  const toggleSidebar = useLuna((state) => state.toggleSidebar)
  const sidebarOpen = useLuna((state) => state.sidebarOpen)
  const terminalOpen = useLuna((state) => state.terminalOpen)
  const settingsOpen = useLuna((state) => state.settingsOpen)

  const [dragging, setDragging] = useState(false)

  useEffect(() => {
    void initialize()
  }, [initialize])

  /**
   * Application shortcuts. The native menu is removed in the main process, so
   * nothing else claims these accelerators.
   */
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent): void => {
      // `?` opens the shortcut sheet, but only outside a text field.
      const target = event.target as HTMLElement | null
      const typing =
        target?.tagName === 'INPUT' ||
        target?.tagName === 'TEXTAREA' ||
        target?.isContentEditable === true

      if (event.key === '?' && !typing && !event.ctrlKey && !event.metaKey) {
        event.preventDefault()
        openShortcuts()
        return
      }

      if (!event.ctrlKey && !event.metaKey) return
      if (settingsOpen) return
      const key = event.key.toLowerCase()

      if (key === 'n') {
        event.preventDefault()
        void newSession()
        return
      }
      if (key === 'k') {
        event.preventDefault()
        focusSearch()
        return
      }
      if (key === 'p') {
        event.preventDefault()
        openQuickOpen()
        return
      }
      if (key === 'b') {
        event.preventDefault()
        toggleSidebar()
        return
      }
      if (key === '`') {
        event.preventDefault()
        toggleTerminal()
      }
    }

    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [
    focusSearch,
    newSession,
    openQuickOpen,
    openShortcuts,
    settingsOpen,
    toggleSidebar,
    toggleTerminal
  ])

  // Dropping a file anywhere in the window attaches it to the composer.
  useEffect(() => {
    const onDragOver = (event: DragEvent): void => {
      if (!event.dataTransfer?.types.includes('Files')) return
      event.preventDefault()
      setDragging(true)
    }
    const onDragLeave = (event: DragEvent): void => {
      if (event.relatedTarget === null) setDragging(false)
    }
    const onDrop = (event: DragEvent): void => {
      event.preventDefault()
      setDragging(false)
      const files = Array.from(event.dataTransfer?.files ?? [])
      if (files.length === 0) return
      // The composer listens for this, so the drop lands where the user expects.
      window.dispatchEvent(new CustomEvent('luna:drop-files', { detail: files }))
    }

    window.addEventListener('dragover', onDragOver)
    window.addEventListener('dragleave', onDragLeave)
    window.addEventListener('drop', onDrop)
    return () => {
      window.removeEventListener('dragover', onDragOver)
      window.removeEventListener('dragleave', onDragLeave)
      window.removeEventListener('drop', onDrop)
    }
  }, [])

  return (
    <>
      <div className={`app rc${sidebarOpen ? '' : ' noside'}`}>
        {sidebarOpen ? <Sidebar /> : null}
        <MainPane />
        {terminalOpen ? (
          <div className="termwrap">
            <TerminalPanel
              onReady={(write) => {
                void window.luna.terminalStart().catch(() => undefined)
                window.luna.onTerminalData(write)
              }}
              onInput={(data) => void window.luna.terminalWrite(data)}
            />
          </div>
        ) : null}
        <StatusFooter />
      </div>

      {dragging ? <div className="dropveil">Drop to attach</div> : null}

      <DialogHost />
      <SettingsDialog />
      <CommandPalette />
      <ShortcutSheet />
      <UpdateToast />
    </>
  )
}
