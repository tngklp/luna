/**
 * Luna theme system.
 *
 * A theme is a named palette. Adding one is a single entry in `THEMES` — the
 * stylesheet only reads CSS custom properties, so no CSS changes are needed.
 */

/** Every colour a theme must define. */
export interface ThemeTokens {
  /** Window background. */
  bg: string
  /** Raised surfaces: sidebar, cards, composer. */
  panel: string
  /** Hover and inset surfaces. */
  raise: string
  /** Borders and dividers. */
  line: string
  /** Primary text. */
  text: string
  /** Secondary text. */
  dim: string
  /** Accent fill for buttons and bubbles. */
  accent: string
  /** Accent for text, icons, and active indicators. */
  accentStrong: string
  /** Tinted accent background. */
  accentSoft: string
  /** Text drawn on `accent`. */
  onAccent: string
  /** Integrated terminal background. */
  term: string
  add: string
  addBg: string
  del: string
  delBg: string
  mod: string
}

/** The three variants every theme ships in. */
export type ThemeVariant = 'normal' | 'dark' | 'light'

export interface Theme {
  id: string
  /** Full name, e.g. `Rose Dark`. */
  name: string
  /** Palette family the variant belongs to, e.g. `Rose`. */
  family: string
  variant: ThemeVariant
  /** Drives `color-scheme` and scrollbar rendering. */
  appearance: 'light' | 'dark'
  tokens: ThemeTokens
}

/** The four colours that differ between a family's variants. */
interface AccentPalette {
  accent: string
  accentStrong: string
  accentSoft: string
  onAccent: string
}

/** One accent colour in three sets of surfaces. */
interface ThemeFamily {
  id: string
  name: string
  accent: AccentPalette
  /** Surfaces the family is known for: dark, with the theme's colour in them. */
  normal: ThemeTokens
  /** Light surfaces, keeping the family's accent. */
  light: ThemeTokens
}

/** Token -> CSS custom property used by the stylesheet. */
const CSS_VARIABLES: Record<keyof ThemeTokens, string> = {
  bg: '--bg',
  panel: '--panel',
  raise: '--raise',
  line: '--line',
  text: '--text',
  dim: '--dim',
  accent: '--lilac',
  accentStrong: '--lilac-strong',
  accentSoft: '--lilac-soft',
  onAccent: '--on-lilac',
  term: '--term',
  add: '--add',
  addBg: '--add-bg',
  del: '--del',
  delBg: '--del-bg',
  mod: '--mod'
}

/**
 * Diff colours and changed-file markers. They read the same on every dark
 * surface and every light surface, so they sit outside the families.
 */
const DARK_SIGNALS = {
  add: '#6FD6A2',
  addBg: '#16281f',
  del: '#F08AA3',
  delBg: '#34202a',
  mod: '#E2C08D'
}

const LIGHT_SIGNALS = {
  add: '#1F8A5B',
  addBg: '#E1F4EA',
  del: '#C23A5A',
  delBg: '#FBE4EA',
  mod: '#B7791F'
}

/**
 * The general dark surface: near-black neutral greys where only the accent
 * carries the theme's colour. Every `<Name> Dark` variant is this plus a family
 * accent, and it always sits visibly darker than the family's normal palette.
 */
const GENERAL_DARK = {
  bg: '#0a0a0c',
  panel: '#0f0f12',
  raise: '#17171b',
  line: '#202026',
  text: '#e6e6ea',
  dim: '#8a8a94',
  term: '#050506',
  ...DARK_SIGNALS
}

/** The general dark surface, wearing one family's accent. */
function darkVariant(accent: AccentPalette): ThemeTokens {
  return { ...GENERAL_DARK, ...accent }
}

const WISTERIA_DARK: ThemeTokens = {
  bg: '#181322',
  panel: '#1f1830',
  raise: '#2b2240',
  line: '#3a2e55',
  text: '#ede9f5',
  dim: '#a79fbc',
  accent: '#BEA4F0',
  accentStrong: '#C9B2F5',
  accentSoft: 'rgba(190,164,240,.18)',
  onAccent: '#24153F',
  term: '#100c18',
  ...DARK_SIGNALS
}

const WISTERIA_LIGHT: ThemeTokens = {
  bg: '#F6F3FB',
  panel: '#FFFFFF',
  raise: '#EEE9F8',
  line: '#E0D9EF',
  text: '#26203A',
  dim: '#6F6788',
  accent: '#C3A9F3',
  accentStrong: '#7C55CC',
  accentSoft: '#E9DFFB',
  onAccent: '#2B1A52',
  term: '#120F1A',
  ...LIGHT_SIGNALS
}

const GRAPHITE_DARK: ThemeTokens = {
  bg: '#191c21',
  panel: '#20242b',
  raise: '#2b313a',
  line: '#39404b',
  text: '#edeff3',
  dim: '#a2a8b2',
  accent: '#A9B1C0',
  accentStrong: '#D2D9E5',
  accentSoft: 'rgba(169,177,192,.18)',
  onAccent: '#1B1F27',
  term: '#0f1114',
  ...DARK_SIGNALS
}

const GRAPHITE_LIGHT: ThemeTokens = {
  bg: '#F6F6F7',
  panel: '#FFFFFF',
  raise: '#EDEDEF',
  line: '#E0E0E3',
  text: '#232327',
  dim: '#6B6B74',
  accent: '#9AA0AE',
  accentStrong: '#4C5361',
  accentSoft: '#E8EAEF',
  onAccent: '#FFFFFF',
  term: '#101114',
  ...LIGHT_SIGNALS
}

const OCEAN_DARK: ThemeTokens = {
  bg: '#0d1a2e',
  panel: '#122340',
  raise: '#1a3055',
  line: '#234070',
  text: '#e3edfa',
  dim: '#95aecf',
  accent: '#4F9CF0',
  accentStrong: '#8CBEFF',
  accentSoft: 'rgba(79,156,240,.2)',
  onAccent: '#06172B',
  term: '#08111f',
  ...DARK_SIGNALS
}

const OCEAN_LIGHT: ThemeTokens = {
  bg: '#EFF4FB',
  panel: '#FFFFFF',
  raise: '#E1EBF8',
  line: '#D2E0F1',
  text: '#14243B',
  dim: '#5C7089',
  accent: '#3F86D8',
  accentStrong: '#1F63B5',
  accentSoft: '#DFEAF9',
  onAccent: '#FFFFFF',
  term: '#0B1420',
  ...LIGHT_SIGNALS
}

const EMBER_DARK: ThemeTokens = {
  bg: '#1d1209',
  panel: '#26180c',
  raise: '#362310',
  line: '#4a3016',
  text: '#f5eadf',
  dim: '#be9f82',
  accent: '#F08A3C',
  accentStrong: '#FFB067',
  accentSoft: 'rgba(240,138,60,.2)',
  onAccent: '#2A1508',
  term: '#120c06',
  ...DARK_SIGNALS
}

const EMBER_LIGHT: ThemeTokens = {
  bg: '#FDF6F0',
  panel: '#FFFFFF',
  raise: '#F8ECE1',
  line: '#ECDCCD',
  text: '#33231A',
  dim: '#7D6754',
  accent: '#E07A2C',
  accentStrong: '#B85C14',
  accentSoft: '#FBE6D3',
  onAccent: '#FFFFFF',
  term: '#140F0B',
  ...LIGHT_SIGNALS
}

const ROSE_DARK: ThemeTokens = {
  bg: '#1e0f18',
  panel: '#28141f',
  raise: '#3a1d2c',
  line: '#4f2839',
  text: '#f7e8f0',
  dim: '#c79eb1',
  accent: '#F072A8',
  accentStrong: '#FF92BD',
  accentSoft: 'rgba(240,114,168,.16)',
  onAccent: '#2C0D1C',
  term: '#100A0E',
  ...DARK_SIGNALS
}

const ROSE_LIGHT: ThemeTokens = {
  bg: '#FDF2F6',
  panel: '#FFFFFF',
  raise: '#F8E6EE',
  line: '#EED3E0',
  text: '#331C28',
  dim: '#7D5F6D',
  accent: '#DD5C93',
  accentStrong: '#B13A72',
  accentSoft: '#FADFEA',
  onAccent: '#FFFFFF',
  term: '#140D11',
  ...LIGHT_SIGNALS
}

/**
 * Accent colours, one entry per family. A family's `normal` and `light`
 * palettes already carry them; the `dark` variant borrows the accent and puts
 * it on the general dark surface.
 */
const ACCENTS: Record<string, AccentPalette> = {
  wisteria: {
    accent: '#BEA4F0',
    accentStrong: '#C9B2F5',
    accentSoft: 'rgba(190,164,240,.15)',
    onAccent: '#24153F'
  },
  graphite: {
    accent: '#A9B1C0',
    accentStrong: '#C3CBDA',
    accentSoft: 'rgba(169,177,192,.15)',
    onAccent: '#1B1F27'
  },
  ocean: {
    accent: '#4F9CF0',
    accentStrong: '#7DB6F7',
    accentSoft: 'rgba(79,156,240,.16)',
    onAccent: '#06172B'
  },
  ember: {
    accent: '#F08A3C',
    accentStrong: '#FFA257',
    accentSoft: 'rgba(240,138,60,.16)',
    onAccent: '#2A1508'
  },
  rose: {
    accent: '#F072A8',
    accentStrong: '#FF92BD',
    accentSoft: 'rgba(240,114,168,.16)',
    onAccent: '#2C0D1C'
  }
}

/** Palette families, in the order they appear in Settings. */
const FAMILIES: ThemeFamily[] = [
  {
    id: 'graphite',
    name: 'Graphite',
    accent: ACCENTS.graphite as AccentPalette,
    normal: GRAPHITE_DARK,
    light: GRAPHITE_LIGHT
  },
  {
    id: 'wisteria',
    name: 'Wisteria',
    accent: ACCENTS.wisteria as AccentPalette,
    normal: WISTERIA_DARK,
    light: WISTERIA_LIGHT
  },
  {
    id: 'ocean',
    name: 'Ocean',
    accent: ACCENTS.ocean as AccentPalette,
    normal: OCEAN_DARK,
    light: OCEAN_LIGHT
  },
  {
    id: 'ember',
    name: 'Ember',
    accent: ACCENTS.ember as AccentPalette,
    normal: EMBER_DARK,
    light: EMBER_LIGHT
  },
  {
    id: 'rose',
    name: 'Rose',
    accent: ACCENTS.rose as AccentPalette,
    normal: ROSE_DARK,
    light: ROSE_LIGHT
  }
]

/**
 * Every theme, three variants per family and in that order, so the settings
 * grid can lay them out three to a row without any grouping logic. The variants
 * go normal, dark, light: the family's own palette, the same accent on the
 * general near-black surface, then the light palette.
 */
export const THEMES: Theme[] = FAMILIES.flatMap((family): Theme[] => [
  {
    id: family.id,
    name: family.name,
    family: family.name,
    variant: 'normal',
    appearance: 'dark',
    tokens: family.normal
  },
  {
    id: `${family.id}-dark`,
    name: `${family.name} Dark`,
    family: family.name,
    variant: 'dark',
    appearance: 'dark',
    tokens: darkVariant(family.accent)
  },
  {
    id: `${family.id}-light`,
    name: `${family.name} Light`,
    family: family.name,
    variant: 'light',
    appearance: 'light',
    tokens: family.light
  }
])

/** Palette used on first run. */
export const DEFAULT_THEME_ID = 'wisteria-dark'

const THEMES_BY_ID = new Map(THEMES.map((theme) => [theme.id, theme]))
const FALLBACK_THEME = THEMES_BY_ID.get(DEFAULT_THEME_ID) ?? (THEMES[0] as Theme)

/**
 * Look up a theme. An id that no longer exists — an older release's palette, or
 * a hand-edited preference — falls back to the default rather than leaving the
 * app unstyled, which is all a migration table would have bought us.
 */
export function findTheme(id: string): Theme {
  return THEMES_BY_ID.get(id) ?? FALLBACK_THEME
}

/** Resolve a stored preference to a concrete theme. */
export function resolveTheme(themeId: string): Theme {
  return findTheme(themeId)
}

/** Write the theme's tokens onto the document root. */
export function applyTheme(theme: Theme): void {
  const root = document.documentElement
  root.dataset.theme = theme.id
  root.dataset.appearance = theme.appearance
  root.style.colorScheme = theme.appearance

  for (const [token, variable] of Object.entries(CSS_VARIABLES)) {
    root.style.setProperty(variable, theme.tokens[token as keyof ThemeTokens])
  }
}
