import { useLuna } from '../store'

/** Full-screen welcome overlay shown on launch. */
export function Welcome() {
  const open = useLuna((state) => state.welcomeOpen)
  const close = useLuna((state) => state.closeWelcome)
  const workspace = useLuna((state) => state.workspace)
  const sessions = useLuna((state) => state.sessions)
  const allSessions = useLuna((state) => state.allSessions)
  const changeWorkspace = useLuna((state) => state.changeWorkspace)
  const openWorkspace = useLuna((state) => state.openWorkspace)
  const newSession = useLuna((state) => state.newSession)
  const switchSession = useLuna((state) => state.switchSession)
  const openSettings = useLuna((state) => state.openSettings)

  // Most recent session per working directory across every project.
  const recents: { cwd: string; name: string; modified: string }[] = []
  const seen = new Set<string>()
  for (const session of allSessions) {
    if (!session.cwd || seen.has(session.cwd)) continue
    seen.add(session.cwd)
    recents.push({
      cwd: session.cwd,
      name: session.cwd.split(/[\\/]/).filter(Boolean).pop() ?? session.cwd,
      modified: session.modified
    })
    if (recents.length >= 4) break
  }

  const lastSession = sessions[0] ?? allSessions[0] ?? null

  return (
    <div className={`welcome${open ? '' : ' hide'}`} aria-hidden={!open}>
      <div className="wcol">
        <div className="wlogo" aria-hidden="true">
          ☾
        </div>
        <h1>Welcome to Luna</h1>
        <p className="lead">
          An AI coding agent with a terminal interface and a desktop app. Pick up where you left
          off, or open a project.
        </p>

        <div className="wgrid">
          <section>
            <h2>Start</h2>

            <button className="act main-act" onClick={() => void changeWorkspace()}>
              <span className="ic">＋</span>
              <span className="tx">
                Open project folder
                <small>Choose a folder to work in</small>
              </span>
            </button>

            <button
              className="act"
              onClick={() => {
                close()
                void newSession()
              }}
            >
              <span className="ic">✎</span>
              <span className="tx">
                New session in {workspace?.name ?? 'this folder'}
                <small>Start with a blank conversation</small>
              </span>
            </button>

            {lastSession ? (
              <button
                className="act"
                onClick={() => {
                  close()
                  void switchSession(lastSession.path)
                }}
              >
                <span className="ic">↺</span>
                <span className="tx">
                  Resume last session
                  <small>
                    {lastSession.name ?? lastSession.firstMessage ?? 'Untitled session'}
                  </small>
                </span>
              </button>
            ) : null}

            <button className="act" onClick={openSettings}>
              <span className="ic">⚙</span>
              <span className="tx">
                Settings
                <small>Models, providers, and tool approval</small>
              </span>
            </button>
          </section>

          <section>
            <h2>Recent projects</h2>
            <div className="rec">
              {recents.length === 0 ? (
                <p style={{ color: 'var(--dim)', fontSize: 13, margin: 0, padding: '0 12px' }}>
                  No projects yet. Open a folder to get started.
                </p>
              ) : null}
              {recents.map((project) => (
                <button
                  className="act"
                  key={project.cwd}
                  onClick={() => void openWorkspace(project.cwd)}
                >
                  <span className="tx">
                    <b>{project.name}</b>
                    <small className="mono">{project.cwd}</small>
                  </span>
                  <span className="when">{new Date(project.modified).toLocaleDateString()}</span>
                </button>
              ))}
            </div>
          </section>
        </div>
      </div>
    </div>
  )
}
