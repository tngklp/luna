/**
 * Ready-made provider entries for hosted APIs.
 *
 * The ids deliberately match pi's own provider ids, so an entry in `models.json`
 * overlays the built-in provider: its streaming behaviour is kept and only the
 * credentials come from the user. Because the overlay merges instead of
 * replacing, a hosted provider would otherwise stay listed with pi's entire
 * built-in catalogue (OpenAI alone has 44 models), so PiUI treats these
 * providers specially: the user picks the models they want from the catalogue,
 * and only those are offered in the app.
 *
 * Both the main process (to filter the model list) and the renderer (to build
 * the provider menu) need this, hence the shared module.
 */
export interface HostedProvider {
  /** Provider id in `models.json`; also pi's built-in id. */
  id: string
  /** Name shown in the add-provider menu. */
  label: string
  baseUrl: string
  api: string
}

export const HOSTED_PROVIDERS: HostedProvider[] = [
  {
    id: 'openai',
    label: 'ChatGPT (OpenAI)',
    baseUrl: 'https://api.openai.com/v1',
    api: 'openai-completions'
  },
  {
    id: 'anthropic',
    label: 'Claude (Anthropic)',
    baseUrl: 'https://api.anthropic.com/v1',
    api: 'anthropic-messages'
  },
  {
    id: 'deepseek',
    label: 'DeepSeek',
    baseUrl: 'https://api.deepseek.com/v1',
    api: 'openai-completions'
  },
  {
    id: 'google',
    label: 'Gemini (Google)',
    baseUrl: 'https://generativelanguage.googleapis.com/v1beta',
    api: 'google-generative-ai'
  },
  { id: 'xai', label: 'Grok (xAI)', baseUrl: 'https://api.x.ai/v1', api: 'openai-completions' },
  {
    id: 'mistral',
    label: 'Mistral',
    baseUrl: 'https://api.mistral.ai/v1',
    api: 'mistral-conversations'
  },
  {
    id: 'groq',
    label: 'Groq',
    baseUrl: 'https://api.groq.com/openai/v1',
    api: 'openai-completions'
  },
  {
    id: 'openrouter',
    label: 'OpenRouter',
    baseUrl: 'https://openrouter.ai/api/v1',
    api: 'openai-completions'
  }
]

/**
 * Matches a preset id, plus the `-2`, `-3`, … suffixes added when the same
 * provider is added twice. Kept anchored so a local provider that merely starts
 * with a preset name (`google-vertex`) is not mistaken for a hosted one.
 */
const HOSTED_ID_PATTERN = new RegExp(
  `^(?:${HOSTED_PROVIDERS.map((provider) => provider.id).join('|')})(?:-\\d+)?$`,
  'i'
)

/** Whether a provider id refers to a hosted API service. */
export function isHostedProviderId(id: string): boolean {
  return HOSTED_ID_PATTERN.test(id.trim())
}
