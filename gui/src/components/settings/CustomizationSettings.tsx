import { useLuna } from '../../store'
import { ICON_PACKS, fileIconUrl, folderIconUrl } from '../../lib/icon-packs'
import { THEMES, type ThemeVariant } from '../../theme/themes'
import { SettingsHeader } from './rows'

/** Small caption under each family name in the theme grid. */
const VARIANT_LABELS: Record<ThemeVariant, string> = {
  normal: 'Normal',
  dark: 'Dark',
  light: 'Light'
}

/**
 * Files previewed in each icon-pack button: a folder plus the languages and
 * file types a project is most likely to contain.
 */
const SAMPLE_FILES = [
  'main.py',
  'app.js',
  'index.ts',
  'Program.cs',
  'main.cpp',
  'Main.java',
  'main.rs',
  'init.lua',
  'main.go',
  'data.json',
  'config.yaml',
  'photo.png',
  'README.md',
  '.gitignore'
]

/**
 * Appearance: the theme grid and the icon-pack grid.
 *
 * `THEMES` is ordered three variants per family, so the grid lays itself out
 * three to a row: a family's normal, dark, and light palettes side by side. The
 * icon packs follow the same idea, each button previewing the icons a project
 * actually shows in the explorer.
 */
export function CustomizationSettings() {
  const themeId = useLuna((state) => state.themeId)
  const setTheme = useLuna((state) => state.setTheme)
  const iconPack = useLuna((state) => state.prefs.iconPack)
  const setPrefs = useLuna((state) => state.setPrefs)

  return (
    <>
      <section className="set-section">
        <SettingsHeader title="Customization" subtitle="Theme and iconography." />

        <h3>Theme</h3>

        <div className="themes" role="radiogroup" aria-label="Theme">
          {THEMES.map((theme) => (
            <button
              key={theme.id}
              type="button"
              role="radio"
              aria-checked={theme.id === themeId}
              className={`theme${theme.id === themeId ? ' on' : ''}`}
              title={theme.name}
              onClick={() => setTheme(theme.id)}
            >
              <span className="theme__swatch" aria-hidden="true">
                <i style={{ background: theme.tokens.bg }} />
                <i style={{ background: theme.tokens.panel }} />
                <i style={{ background: theme.tokens.raise }} />
                <i style={{ background: theme.tokens.accent }} />
              </span>
              <span className="theme__text">
                <b>{theme.family}</b>
                <small>{VARIANT_LABELS[theme.variant]}</small>
              </span>
            </button>
          ))}
        </div>
      </section>

      <section className="set-section">
        <h3>Icon pack</h3>

        <div className="packs" role="radiogroup" aria-label="Icon pack">
          {ICON_PACKS.map((pack) => (
            <button
              key={pack.id}
              type="button"
              role="radio"
              aria-checked={pack.id === iconPack}
              className={`pack${pack.id === iconPack ? ' on' : ''}`}
              title={pack.name}
              onClick={() => setPrefs({ iconPack: pack.id })}
            >
              <span className="pack__row">
                <img src={folderIconUrl(pack.id, 'src', false)} alt="" title="src/" />
                {SAMPLE_FILES.map((file) => (
                  <img key={file} src={fileIconUrl(pack.id, file)} alt="" title={file} />
                ))}
              </span>
              <span className="pack__name">{pack.name}</span>
            </button>
          ))}
        </div>
      </section>
    </>
  )
}
