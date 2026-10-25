import { useEffect, useMemo, useRef, useState } from 'react'
import type { ModelDefDto, ModelsConfigDto, ProviderConfigDto } from '@shared/ipc'
import { useLuna } from '../../store'
import { Select, type SelectOption } from '../Select'
import { SettingsHeader } from './rows'

/** Wire protocols Luna understands. */
const KIND_OPTIONS: SelectOption<string>[] = [
  { value: 'openai', label: 'OpenAI-compatible' },
  { value: 'anthropic', label: 'Anthropic messages' },
  { value: 'gemini', label: 'Google Gemini' }
]

/** Built-in presets, mirroring `luna-core`'s `presets.rs`. */
const PRESETS: { id: string; label: string; kind: string; baseUrl: string; keyEnv: string }[] = [
  { id: 'anthropic', label: 'Anthropic', kind: 'anthropic', baseUrl: 'https://api.anthropic.com', keyEnv: 'ANTHROPIC_API_KEY' },
  { id: 'openai', label: 'OpenAI', kind: 'openai', baseUrl: 'https://api.openai.com/v1', keyEnv: 'OPENAI_API_KEY' },
  { id: 'gemini', label: 'Google Gemini', kind: 'gemini', baseUrl: 'https://generativelanguage.googleapis.com/v1beta', keyEnv: 'GEMINI_API_KEY' },
  { id: 'openrouter', label: 'OpenRouter', kind: 'openai', baseUrl: 'https://openrouter.ai/api/v1', keyEnv: 'OPENROUTER_API_KEY' },
  { id: 'groq', label: 'Groq', kind: 'openai', baseUrl: 'https://api.groq.com/openai/v1', keyEnv: 'GROQ_API_KEY' },
  { id: 'deepseek', label: 'DeepSeek', kind: 'openai', baseUrl: 'https://api.deepseek.com/v1', keyEnv: 'DEEPSEEK_API_KEY' },
  { id: 'mistral', label: 'Mistral', kind: 'openai', baseUrl: 'https://api.mistral.ai/v1', keyEnv: 'MISTRAL_API_KEY' },
  { id: 'xai', label: 'xAI', kind: 'openai', baseUrl: 'https://api.x.ai/v1', keyEnv: 'XAI_API_KEY' },
  { id: 'together', label: 'Together AI', kind: 'openai', baseUrl: 'https://api.together.xyz/v1', keyEnv: 'TOGETHER_API_KEY' },
  { id: 'fireworks', label: 'Fireworks AI', kind: 'openai', baseUrl: 'https://api.fireworks.ai/inference/v1', keyEnv: 'FIREWORKS_API_KEY' },
  { id: 'cerebras', label: 'Cerebras', kind: 'openai', baseUrl: 'https://api.cerebras.ai/v1', keyEnv: 'CEREBRAS_API_KEY' },
  { id: 'perplexity', label: 'Perplexity', kind: 'openai', baseUrl: 'https://api.perplexity.ai', keyEnv: 'PERPLEXITY_API_KEY' },
  { id: 'zai', label: 'Z.ai (GLM)', kind: 'openai', baseUrl: 'https://api.z.ai/api/paas/v4', keyEnv: 'ZAI_API_KEY' },
  { id: 'moonshot', label: 'Moonshot (Kimi)', kind: 'openai', baseUrl: 'https://api.moonshot.ai/v1', keyEnv: 'MOONSHOT_API_KEY' },
  { id: 'ollama', label: 'Ollama (local)', kind: 'openai', baseUrl: 'http://localhost:11434/v1', keyEnv: '' },
  { id: 'llamacpp', label: 'llama.cpp (local)', kind: 'openai', baseUrl: 'http://localhost:8080/v1', keyEnv: '' },
  { id: 'lmstudio', label: 'LM Studio (local)', kind: 'openai', baseUrl: 'http://localhost:1234/v1', keyEnv: '' },
  { id: 'vllm', label: 'vLLM (local)', kind: 'openai', baseUrl: 'http://localhost:8000/v1', keyEnv: '' }
]

/** How long to wait after the last edit before writing `config.toml`. */
const SAVE_DEBOUNCE_MS = 500

function emptyModel(): ModelDefDto {
  return {
    id: '',
    name: '',
    reasoning: false,
    input: ['text'],
    contextWindow: 128_000,
    maxTokens: 16_384,
    cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 }
  }
}

/** A provider entry for a preset, with a free id. */
function presetProvider(preset: (typeof PRESETS)[number], taken: ProviderConfigDto[]): ProviderConfigDto {
  const used = new Set(taken.map((provider) => provider.id))
  let id = preset.id
  let suffix = 2
  while (used.has(id)) {
    id = `${preset.id}-${suffix}`
    suffix += 1
  }
  return {
    id,
    preset: preset.id,
    kind: preset.kind,
    baseUrl: preset.baseUrl,
    apiKeyEnv: preset.keyEnv || null,
    discoverModels: true,
    models: []
  }
}

/** A blank provider, for anything not in the preset list. */
function customProvider(taken: ProviderConfigDto[]): ProviderConfigDto {
  const used = new Set(taken.map((provider) => provider.id))
  let index = taken.length + 1
  while (used.has(`provider-${index}`)) index += 1
  return {
    id: `provider-${index}`,
    preset: null,
    kind: 'openai',
    baseUrl: 'http://127.0.0.1:8080/v1',
    apiKeyEnv: null,
    discoverModels: true,
    models: [emptyModel()]
  }
}

/**
 * Provider and model catalogue editor.
 *
 * Edits are written to `~/.luna/config.toml` shortly after the last change and
 * the running agent reloads the catalogue, so a new model is selectable in the
 * composer without restarting.
 */
export function ModelsSettings() {
  const config = useLuna((state) => state.modelsConfig)
  const modelsError = useLuna((state) => state.modelsError)
  const save = useLuna((state) => state.saveModelsConfig)

  const [draft, setDraft] = useState<ModelsConfigDto | null>(null)
  /** Provider indices whose API key is visible. */
  const [revealed, setRevealed] = useState<number[]>([])
  /** Whether the "add provider" menu is open. */
  const [adding, setAdding] = useState(false)

  const hydrated = useRef(false)
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const addRef = useRef<HTMLDivElement | null>(null)

  // Seed once per dialog open so writes do not fight the user's typing.
  useEffect(() => {
    if (hydrated.current || !config) return
    hydrated.current = true
    setDraft(config)
  }, [config])

  useEffect(() => {
    if (!draft) return
    if (timer.current) clearTimeout(timer.current)
    timer.current = setTimeout(() => void save(draft), SAVE_DEBOUNCE_MS)
    return () => {
      if (timer.current) clearTimeout(timer.current)
    }
  }, [draft, save])

  // Close the add-provider menu on an outside click or Escape.
  useEffect(() => {
    if (!adding) return

    const onPointerDown = (event: globalThis.PointerEvent): void => {
      if (!addRef.current?.contains(event.target as Node)) setAdding(false)
    }
    const onKey = (event: KeyboardEvent): void => {
      if (event.key === 'Escape') setAdding(false)
    }

    window.addEventListener('pointerdown', onPointerDown)
    window.addEventListener('keydown', onKey)
    return () => {
      window.removeEventListener('pointerdown', onPointerDown)
      window.removeEventListener('keydown', onKey)
    }
  }, [adding])

  const modelOptions = useMemo<SelectOption<string>[]>(() => {
    if (!draft) return []
    return draft.providers.flatMap((provider) =>
      provider.models
        .filter((model) => model.id.trim().length > 0)
        .map((model) => ({
          value: `${provider.id}/${model.id}`,
          label: `${provider.id} / ${model.name || model.id}`
        }))
    )
  }, [draft])

  if (!draft) {
    return (
      <section className="set-section">
        <h3>Models</h3>
        <p className="modal__hint">Loading the model catalogue…</p>
      </section>
    )
  }

  const update = (mutate: (next: ModelsConfigDto) => void): void => {
    const next: ModelsConfigDto = {
      path: draft.path,
      defaultModel: draft.defaultModel,
      providers: draft.providers.map((provider) => ({
        ...provider,
        models: provider.models.map((model) => ({ ...model }))
      }))
    }
    mutate(next)
    setDraft(next)
  }

  const updateProvider = (index: number, patch: Partial<ProviderConfigDto>): void =>
    update((next) => {
      next.providers[index] = { ...next.providers[index], ...patch }
    })

  const updateModel = (provider: number, model: number, patch: Partial<ModelDefDto>): void =>
    update((next) => {
      const models = next.providers[provider].models
      models[model] = { ...models[model], ...patch }
    })

  const toggleReveal = (index: number): void =>
    setRevealed((current) =>
      current.includes(index) ? current.filter((entry) => entry !== index) : [...current, index]
    )

  const addProvider = (provider: ProviderConfigDto): void => {
    update((next) => {
      next.providers.push(provider)
    })
    setAdding(false)
  }

  return (
    <section className="set-section">
      <SettingsHeader title="Models" subtitle="Providers, credentials and model definitions." />

      {modelsError ? (
        <p className="hint bad">
          This catalogue could not be loaded, so no model is selectable: {modelsError}
        </p>
      ) : null}

      <h3>Default model</h3>
      <div className="gcard">
        <div className="grow">
          <div className="grow__text">
            <b>Model</b>
            <small>The model a new session starts on.</small>
          </div>
          <div className="grow__ctl">
            {modelOptions.length > 0 ? (
              <Select
                value={draft.defaultModel}
                options={modelOptions}
                title="Default model"
                onChange={(defaultModel) => update((next) => void (next.defaultModel = defaultModel))}
              />
            ) : (
              <span className="hint">Add a model below first.</span>
            )}
          </div>
        </div>
      </div>

      <h3>Providers</h3>

      {draft.providers.map((provider, providerIndex) => (
        <div className="prov" key={providerIndex}>
          <div className="prov__hd">
            <input
              className="prov__id mono"
              value={provider.id}
              aria-label="Provider id"
              placeholder="provider id"
              spellCheck={false}
              onChange={(event) => updateProvider(providerIndex, { id: event.target.value })}
            />
            <span className="sp" />
            <button
              className="b sm bad"
              onClick={() =>
                update((next) => {
                  next.providers.splice(providerIndex, 1)
                })
              }
            >
              Remove provider
            </button>
          </div>

          <div className="prov__row">
            <label>
              <span>Base URL</span>
              <input
                className="inp mono"
                value={provider.baseUrl ?? ''}
                aria-label="Base URL"
                placeholder="https://api.example.com/v1"
                onChange={(event) =>
                  updateProvider(providerIndex, { baseUrl: event.target.value || null })
                }
              />
            </label>
            <label>
              <span>API key variable</span>
              <span className="secret">
                <input
                  className="inp mono"
                  type={revealed.includes(providerIndex) ? 'text' : 'password'}
                  value={provider.apiKeyEnv ?? ''}
                  aria-label="API key variable"
                  placeholder="PROVIDER_API_KEY"
                  autoComplete="off"
                  spellCheck={false}
                  onChange={(event) =>
                    updateProvider(providerIndex, { apiKeyEnv: event.target.value || null })
                  }
                />
                <button
                  type="button"
                  className="secret__eye"
                  title={revealed.includes(providerIndex) ? 'Hide' : 'Show'}
                  aria-label={revealed.includes(providerIndex) ? 'Hide' : 'Show'}
                  aria-pressed={revealed.includes(providerIndex)}
                  onClick={() => toggleReveal(providerIndex)}
                >
                  {revealed.includes(providerIndex) ? '◉' : '◎'}
                </button>
              </span>
            </label>
            <label>
              <span>Protocol</span>
              <Select
                value={provider.kind}
                options={KIND_OPTIONS}
                title="Wire protocol"
                onChange={(kind) => updateProvider(providerIndex, { kind })}
              />
            </label>
          </div>

          <p className="prov__hint">
            The key itself is read from the environment, so it never lands in{' '}
            <code>config.toml</code>. Leave the variable blank for a local endpoint.
          </p>

          <div className="prov__models">
            <div className="prov__models-hd">
              <b>Models</b>
              <span className="sp" />
              <button
                className="b sm"
                onClick={() =>
                  update((next) => {
                    next.providers[providerIndex].models.push(emptyModel())
                  })
                }
              >
                Add model
              </button>
            </div>

            {provider.models.length === 0 ? (
              <p className="hint">
                No models yet. Add one, or leave it empty and Luna will ask the endpoint.
              </p>
            ) : null}

            {provider.models.map((model, modelIndex) => (
              <div className="prov__model" key={modelIndex}>
                <input
                  className="inp mono"
                  value={model.id}
                  aria-label="Model id"
                  placeholder="model-id"
                  spellCheck={false}
                  onChange={(event) =>
                    updateModel(providerIndex, modelIndex, { id: event.target.value })
                  }
                />
                <input
                  className="inp"
                  value={model.name}
                  aria-label="Model name"
                  placeholder="Display name"
                  onChange={(event) =>
                    updateModel(providerIndex, modelIndex, { name: event.target.value })
                  }
                />
                <input
                  className="inp num"
                  type="number"
                  value={model.contextWindow}
                  aria-label="Context window"
                  title="Context window"
                  onChange={(event) =>
                    updateModel(providerIndex, modelIndex, {
                      contextWindow: Number(event.target.value) || 0
                    })
                  }
                />
                <input
                  className="inp num"
                  type="number"
                  value={model.maxTokens}
                  aria-label="Max output tokens"
                  title="Max output tokens"
                  onChange={(event) =>
                    updateModel(providerIndex, modelIndex, {
                      maxTokens: Number(event.target.value) || 0
                    })
                  }
                />
                <button
                  className="b sm bad"
                  aria-label="Remove model"
                  title="Remove model"
                  onClick={() =>
                    update((next) => {
                      next.providers[providerIndex].models.splice(modelIndex, 1)
                    })
                  }
                >
                  ×
                </button>
              </div>
            ))}
          </div>
        </div>
      ))}

      <div className="prov__add" ref={addRef}>
        <button className="b" onClick={() => setAdding((open) => !open)}>
          Add provider
        </button>
        {adding ? (
          <div className="prov__menu">
            <button className="prov__menu-item" onClick={() => addProvider(customProvider(draft.providers))}>
              <b>Custom provider</b>
              <small>Any OpenAI-compatible endpoint</small>
            </button>
            {PRESETS.map((preset) => (
              <button
                key={preset.id}
                className="prov__menu-item"
                onClick={() => addProvider(presetProvider(preset, draft.providers))}
              >
                <b>{preset.label}</b>
                <small className="mono">{preset.baseUrl}</small>
              </button>
            ))}
          </div>
        ) : null}
      </div>

      <p className="set-note">
        Changes are written to <code className="mono">{draft.path}</code> as you type.
      </p>
    </section>
  )
}
