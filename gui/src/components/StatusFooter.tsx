import { formatCost, formatPercent, formatTokens } from '../lib/format'
import { useLuna } from '../store'

/** Bottom status strip: context usage, cost, tokens, agent state, versions. */
export function StatusFooter() {
  const status = useLuna((state) => state.status)
  const appInfo = useLuna((state) => state.appInfo)
  const runtime = useLuna((state) => state.runtime)

  const percent = status?.contextUsage?.percent ?? null
  const contextWindow = status?.contextUsage?.contextWindow ?? 0

  // The SDK is loaded from the installed pi release, so its version is the one
  // that actually runs the agent. The CuI version is shown as a tooltip.
  const piVersion = runtime?.sdkVersion ?? null
  const runtimeNote = appInfo
    ? `Electron ${appInfo.versions.electron} · Chromium ${appInfo.versions.chrome} · Node ${appInfo.versions.node}`
    : undefined

  return (
    <footer className="foot">
      <span className="stat">
        Context
        <span className="meter">
          <i style={{ width: `${Math.min(100, Math.max(0, percent ?? 0))}%` }} />
        </span>
        {formatPercent(percent)} of {formatTokens(contextWindow)}
      </span>
      <span className="stat hide">Cost {formatCost(status?.cost ?? 0)}</span>
      <span className="stat hide">
        {formatTokens(status?.tokens.input ?? 0)} in · {formatTokens(status?.tokens.output ?? 0)}{' '}
        out
      </span>
      <span className="sp" />
      {status?.isCompacting ? <span className="stat">Compacting…</span> : null}
      {status?.isRetrying ? <span className="stat">Retrying…</span> : null}
      {status && status.pendingMessageCount > 0 ? (
        <span className="stat">{status.pendingMessageCount} queued</span>
      ) : null}

      <span className="stat versions" title={runtimeNote}>
        Luna {appInfo?.version ?? '—'}
        <i />
        Engine {piVersion ?? '—'}
      </span>
    </footer>
  )
}
