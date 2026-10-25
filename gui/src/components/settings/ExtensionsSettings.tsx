import { useEffect, useState } from 'react'
import { useLuna } from '../../store'
import { SettingsHeader } from './rows'

/**
 * Extensions: MCP servers, skills, hooks and plugins.
 *
 * Everything here is read from disk, so the tab is a view rather than an editor:
 * it shows what Luna found and where it came from, which is what a user needs
 * when a skill is not being picked up.
 */

interface ExtensionInfo {
  skills: { name: string; description: string; path: string }[]
  commands: { name: string; description: string; path: string }[]
  hooks: { event: string; command: string; path: string }[]
  plugins: { name: string; description: string; version: string; path: string }[]
  mcp: { name: string; transport: string; tools: number; error: string | null }[]
  problems: string[]
}

const EMPTY: ExtensionInfo = {
  skills: [],
  commands: [],
  hooks: [],
  plugins: [],
  mcp: [],
  problems: []
}

export function ExtensionsSettings() {
  const [info, setInfo] = useState<ExtensionInfo>(EMPTY)
  const [error, setError] = useState<string | null>(null)
  const workspace = useLuna((state) => state.workspace)

  useEffect(() => {
    let cancelled = false
    void window.luna
      .getExtensions()
      .then((value) => {
        if (!cancelled) setInfo(value as ExtensionInfo)
      })
      .catch((cause: unknown) => {
        if (!cancelled) setError(cause instanceof Error ? cause.message : String(cause))
      })
    return () => {
      cancelled = true
    }
  }, [workspace?.cwd])

  return (
    <section className="set-section">
      <SettingsHeader
        title="Extensions"
        subtitle="Skills, commands, hooks, plugins and MCP servers Luna found on disk."
      />

      {error ? <p className="hint bad">{error}</p> : null}

      {info.problems.length > 0 ? (
        <div className="gcard">
          <h3>Problems</h3>
          {info.problems.map((problem) => (
            <p className="hint bad" key={problem}>
              {problem}
            </p>
          ))}
        </div>
      ) : null}

      <h3>MCP servers</h3>
      <div className="gcard">
        {info.mcp.length === 0 ? (
          <p className="hint">
            No servers configured. Add a <code>[[mcp]]</code> table to{' '}
            <code>~/.luna/config.toml</code>.
          </p>
        ) : null}
        {info.mcp.map((server) => (
          <div className="grow" key={server.name}>
            <div className="grow__text">
              <b>{server.name}</b>
              <small>
                {server.transport}
                {server.error ? ` — ${server.error}` : ` · ${server.tools} tools`}
              </small>
            </div>
          </div>
        ))}
      </div>

      <h3>Skills</h3>
      <div className="gcard">
        {info.skills.length === 0 ? (
          <p className="hint">
            No skills. Add a markdown file to <code>~/.luna/skills/</code> or{' '}
            <code>.luna/skills/</code>.
          </p>
        ) : null}
        {info.skills.map((skill) => (
          <div className="grow" key={skill.path}>
            <div className="grow__text">
              <b>{skill.name}</b>
              <small>{skill.description || skill.path}</small>
            </div>
          </div>
        ))}
      </div>

      <h3>Commands</h3>
      <div className="gcard">
        {info.commands.length === 0 ? (
          <p className="hint">
            No custom commands. Add one to <code>~/.luna/commands/</code>.
          </p>
        ) : null}
        {info.commands.map((command) => (
          <div className="grow" key={command.path}>
            <div className="grow__text">
              <b>/{command.name}</b>
              <small>{command.description || command.path}</small>
            </div>
          </div>
        ))}
      </div>

      <h3>Hooks</h3>
      <div className="gcard">
        {info.hooks.length === 0 ? (
          <p className="hint">
            No hooks. Add one to <code>~/.luna/hooks/</code> with an <code>event</code> and a{' '}
            <code>command</code>.
          </p>
        ) : null}
        {info.hooks.map((hook) => (
          <div className="grow" key={hook.path}>
            <div className="grow__text">
              <b>{hook.event}</b>
              <small className="mono">{hook.command}</small>
            </div>
          </div>
        ))}
      </div>

      <h3>Plugins</h3>
      <div className="gcard">
        {info.plugins.length === 0 ? (
          <p className="hint">
            No plugins. A plugin is a directory under <code>~/.luna/plugins/</code> with a{' '}
            <code>plugin.json</code>.
          </p>
        ) : null}
        {info.plugins.map((plugin) => (
          <div className="grow" key={plugin.path}>
            <div className="grow__text">
              <b>{plugin.name}</b>
              <small>
                {plugin.version ? `${plugin.version} · ` : ''}
                {plugin.description || plugin.path}
              </small>
            </div>
          </div>
        ))}
      </div>
    </section>
  )
}
