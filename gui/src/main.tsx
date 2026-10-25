import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import App from './App'
import { applyStoredTheme, loadThemeId } from './theme/preference'
import { ready } from './lib/bridge'
import './styles/global.css'

applyStoredTheme(loadThemeId())

const container = document.getElementById('root')
if (!container) throw new Error('the root element is missing from index.html')

// The bridge installs `window.luna`; rendering before it exists would leave the
// store talking to nothing.
void ready.then(() => {
  createRoot(container).render(
    <StrictMode>
      <App />
    </StrictMode>
  )
})
