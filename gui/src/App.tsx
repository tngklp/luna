import { useEffect } from 'react'
import { DialogHost } from './components/DialogHost'
import { MainPane } from './components/MainPane'
import { Sidebar } from './components/Sidebar'
import { StatusFooter } from './components/StatusFooter'
import { UpdateToast } from './components/UpdateToast'
import { Welcome } from './components/Welcome'
import { useLuna } from './store'

/**
 * The application shell: sidebar, main column, status strip, and the overlays.
 *
 * piui's shell also mounts a right panel, a settings dialog and a quick-open
 * picker; Luna has no editor or file explorer, so those are gone. The layout
 * grid, the overlay order and the shortcuts are unchanged.
 */
export default function App() {
  const initialize = useLuna((state) => state.initialize)
  const newSession = useLuna((state) => state.newSession)
  const focusSearch = useLuna((state) => state.focusSearch)
  const closeWelcome = useLuna((state) => state.closeWelcome)

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
        closeWelcome()
        void newSession()
        return
      }
      if (key === 'k') {
        event.preventDefault()
        closeWelcome()
        focusSearch()
      }
    }

    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [closeWelcome, focusSearch, newSession])

  return (
    <>
      <div className="app rc">
        <Sidebar />
        <MainPane />
        <StatusFooter />
      </div>

      {/* The welcome overlay comes first in the DOM and sits below the dialogs
          in the stacking order, so a dialog opened from it is visible. */}
      <Welcome />
      <DialogHost />
      <UpdateToast />
    </>
  )
}
