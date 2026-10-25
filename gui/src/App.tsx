import { useEffect } from 'react'
import { DialogHost } from './components/DialogHost'
import { MainPane } from './components/MainPane'
import { SettingsDialog } from './components/SettingsDialog'
import { Sidebar } from './components/Sidebar'
import { StatusFooter } from './components/StatusFooter'
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

  useEffect(() => {
    void initialize()
  }, [initialize])

  /**
   * Application shortcuts. The native menu is removed in the main process, so
   * nothing else claims these accelerators.
   */
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent): void => {
      if (!event.ctrlKey && !event.metaKey) return
      const key = event.key.toLowerCase()

      if (key === 'n') {
        event.preventDefault()
        void newSession()
        return
      }
      if (key === 'k') {
        event.preventDefault()
        focusSearch()
      }
    }

    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [focusSearch, newSession])

  return (
    <>
      <div className="app rc">
        <Sidebar />
        <MainPane />
        <StatusFooter />
      </div>

      <DialogHost />
      <SettingsDialog />
      <UpdateToast />
    </>
  )
}
