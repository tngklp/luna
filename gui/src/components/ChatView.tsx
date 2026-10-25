import { useLuna } from '../store'
import { Composer } from './Composer'
import { Transcript } from './Transcript'

/**
 * Tell the user when no provider is usable yet.
 *
 * PiUI's banner offered to install the `pi` CLI. Luna has no external CLI — the
 * engine is inside the app — so the equivalent problem is a missing API key, and
 * the equivalent fix is pointing at the config file rather than running an
 * installer.
 */
function ProviderBanner() {
  const runtime = useLuna((state) => state.runtime)

  if (!runtime || runtime.cliInstalled) return null

  return (
    <div className="banner warn install" role="status">
      <div className="install__text">
        <b>No provider is configured.</b> Luna needs an API key for at least one provider. Set{' '}
        <code>ANTHROPIC_API_KEY</code>, <code>OPENAI_API_KEY</code> or <code>GEMINI_API_KEY</code>,
        or add a key to <code>config.toml</code>, then restart Luna. A local model needs no key —
        point the <code>local</code> provider at your Ollama or llama.cpp server.
      </div>
    </div>
  )
}

/** Chat body: banners, notices, transcript, and composer. */
export function ChatView() {
  const runtime = useLuna((state) => state.runtime)
  const error = useLuna((state) => state.error)
  const notices = useLuna((state) => state.notices)
  const dismissNotice = useLuna((state) => state.dismissNotice)

  return (
    <>
      <ProviderBanner />

      {runtime && !runtime.versionMatch ? (
        <div className="banner warn">
          Running on Luna <b>{runtime.sdkVersion}</b>
          {runtime.cliVersion
            ? `, but the installed CLI is ${runtime.cliVersion}`
            : ', and no installed CLI was detected'}
          . Luna runs its own engine, so this mismatch only affects the model catalogue.
        </div>
      ) : null}

      {error ? (
        <div className="banner bad" role="alert">
          {error}
        </div>
      ) : null}

      {notices.length > 0 ? (
        <div className="notices">
          {notices.map((notice) => (
            <button
              className={`notice ${notice.level}`}
              key={notice.id}
              onClick={() => dismissNotice(notice.id)}
              title="Dismiss"
            >
              {notice.message}
            </button>
          ))}
        </div>
      ) : null}

      <div className="scroll">
        <Transcript />
      </div>

      <Composer />
    </>
  )
}
