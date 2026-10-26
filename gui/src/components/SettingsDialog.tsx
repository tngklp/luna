import { useEffect } from 'react'
import { useLuna, type SettingsTab } from '../store'
import { CustomizationSettings } from './settings/CustomizationSettings'
import { ExtensionsSettings } from './settings/ExtensionsSettings'
import { GeneralSettings } from './settings/GeneralSettings'
import { ModelsSettings } from './settings/ModelsSettings'
import { ToolsSettings } from './settings/ToolsSettings'

const TABS: { id: SettingsTab; label: string; hint: string }[] = [
  { id: 'general', label: 'General', hint: 'App and runtime defaults' },
  { id: 'models', label: 'Models', hint: 'Providers and models' },
  { id: 'customization', label: 'Customization', hint: 'Theme and appearance' },
  { id: 'tools', label: 'Tools', hint: 'Approval policy' },
  { id: 'extensions', label: 'Extensions', hint: 'Skills, hooks and MCP' }
]

/**
 * Tabbed settings dialog. Nothing here has a save button — every control writes
 * through to its store action as soon as it changes.
 */
export function SettingsDialog() {
  const open = useLuna((state) => state.settingsOpen)
  const close = useLuna((state) => state.closeSettings)
  const tab = useLuna((state) => state.settingsTab)
  const setTab = useLuna((state) => state.setSettingsTab)

  // Escape closes the dialog, the way every other overlay in the app behaves.
  useEffect(() => {
    if (!open) return
    const onKey = (event: KeyboardEvent): void => {
      if (event.key === 'Escape') close()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [open, close])

  if (!open) return null

  return (
    <div className="modal" role="dialog" aria-modal="true" aria-label="Settings">
      <div className="modal__backdrop" onClick={close} />
      <div className="modal__panel wide settings">
        <nav className="settings__nav" role="tablist" aria-orientation="vertical">
          <h2 className="settings__title">Settings</h2>
          {TABS.map((entry) => (
            <button
              key={entry.id}
              role="tab"
              aria-selected={tab === entry.id}
              className={`settings__tab${tab === entry.id ? ' on' : ''}`}
              onClick={() => setTab(entry.id)}
            >
              <b>{entry.label}</b>
              <small>{entry.hint}</small>
            </button>
          ))}
        </nav>

        <div className="settings__body">
          {tab === 'general' ? <GeneralSettings /> : null}
          {tab === 'customization' ? <CustomizationSettings /> : null}
          {tab === 'models' ? <ModelsSettings /> : null}
          {tab === 'tools' ? <ToolsSettings /> : null}
          {tab === 'extensions' ? <ExtensionsSettings /> : null}
        </div>
      </div>
    </div>
  )
}
