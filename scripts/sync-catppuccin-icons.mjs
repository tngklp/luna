/**
 * Build the Catppuccin icon pack for the explorer.
 *
 * `@iconify-json/catppuccin` ships the Catppuccin Icons artwork as Iconify icon
 * bodies (16x16, coloured from the Mocha flavour). The upstream VS Code theme
 * also carries the file-name/extension associations, but those live in the
 * theme repository rather than the npm package, so the lookups below are PiUI's
 * own: a curated table for the files and folders an explorer actually meets,
 * plus a plain file/folder fallback. Every name is validated against the set, so
 * a typo degrades to the fallback instead of a broken image.
 *
 * Generated output is gitignored. Wired into `npm run icons`, which `predev`
 * and `pretypecheck` call.
 */
import { mkdir, readFile, rm, writeFile } from 'node:fs/promises'
import { dirname, join } from 'node:path'

const root = process.cwd()
const source = join(root, 'node_modules', '@iconify-json', 'catppuccin', 'icons.json')
const iconsOut = join(root, 'public', 'catppuccin-icons')
const manifestOut = join(root, 'src', 'lib', 'catppuccin-icons.json')

/** Exact file names, lower-cased. */
const FILE_NAMES = {
  'package.json': 'package-json',
  'package-lock.json': 'npm-lock',
  'npm-shrinkwrap.json': 'npm-lock',
  'yarn.lock': 'yarn-lock',
  'pnpm-lock.yaml': 'pnpm-lock',
  'bun.lockb': 'bun-lock',
  'bun.lock': 'bun-lock',
  'cargo.toml': 'cargo',
  'cargo.lock': 'cargo-lock',
  'go.mod': 'go-mod',
  'go.sum': 'go-mod',
  'tsconfig.json': 'typescript-config',
  'jsconfig.json': 'javascript-config',
  'eslint.config.js': 'eslint',
  'eslint.config.mjs': 'eslint',
  'stylelint.config.js': 'stylelint',
  'vite.config.ts': 'vite',
  'vitest.config.ts': 'vitest',
  'jest.config.js': 'jest',
  'nuxt.config.ts': 'nuxt',
  'next.config.js': 'next',
  'next.config.mjs': 'next',
  'astro.config.mjs': 'astro-config',
  'svelte.config.js': 'svelte-config',
  'tailwind.config.js': 'tailwind',
  'tailwind.config.ts': 'tailwind',
  'postcss.config.js': 'postcss',
  'rollup.config.js': 'rollup',
  'webpack.config.js': 'webpack',
  'angular.json': 'angular',
  'serverless.yml': 'serverless',
  'netlify.toml': 'netlify',
  'vercel.json': 'vercel',
  'firebase.json': 'firebase',
  'docker-compose.yml': 'docker-compose',
  'docker-compose.yaml': 'docker-compose',
  'compose.yml': 'docker-compose',
  'compose.yaml': 'docker-compose',
  dockerfile: 'docker',
  makefile: 'makefile',
  'cmakelists.txt': 'cmake',
  'nginx.conf': 'nginx',
  caddyfile: 'caddy',
  gemfile: 'ruby-gem',
  'gemfile.lock': 'ruby-gem-lock',
  rakefile: 'ruby',
  'composer.json': 'php',
  'pom.xml': 'apache',
  'build.gradle': 'gradle',
  'build.gradle.kts': 'gradle',
  'pyproject.toml': 'python-config',
  'setup.py': 'python-config',
  'requirements.txt': 'python-config',
  'poetry.lock': 'poetry-lock',
  'readme.md': 'readme',
  readme: 'readme',
  license: 'license',
  'license.md': 'license',
  licence: 'license',
  'licence.md': 'license',
  'changelog.md': 'changelog',
  'contributing.md': 'contributing',
  'code_of_conduct.md': 'code-of-conduct',
  codeowners: 'codeowners',
  'robots.txt': 'robots',
  'humans.txt': 'humans',
  '.gitignore': 'git',
  '.gitattributes': 'git',
  '.gitmodules': 'git',
  '.gitkeep': 'git',
  '.dockerignore': 'docker-ignore',
  '.npmignore': 'npm-ignore',
  '.npmrc': 'npm',
  '.nvmrc': 'npm',
  '.node-version': 'npm',
  '.python-version': 'python',
  '.ruby-version': 'ruby',
  '.eslintignore': 'eslint-ignore',
  '.prettierignore': 'prettier-ignore',
  '.stylelintignore': 'stylelint-ignore',
  '.prettierrc': 'prettier',
  '.editorconfig': 'editorconfig',
  '.env': 'env',
  '.env.local': 'env',
  '.env.development': 'env',
  '.env.production': 'env',
  '.envrc': 'envrc',
  '.browserslistrc': 'browserslist',
  '.babelrc': 'babel',
  'babel.config.js': 'babel',
  '.gitlab-ci.yml': 'gitlab',
  '.pre-commit-config.yaml': 'pre-commit',
  'renovate.json': 'renovate',
  'dependabot.yml': 'dependabot',
  'commitlint.config.js': 'commitlint',
  'workflow.yml': 'workflow'
}

/** File extensions, without the leading dot. */
const FILE_EXTENSIONS = {
  ts: 'typescript',
  mts: 'typescript',
  cts: 'typescript',
  d: 'typescript-def',
  tsx: 'typescript-react',
  js: 'javascript',
  mjs: 'javascript',
  cjs: 'javascript',
  jsx: 'javascript-react',
  json: 'json',
  jsonc: 'json',
  json5: 'json',
  md: 'markdown',
  markdown: 'markdown',
  mdx: 'markdown-mdx',
  yml: 'yaml',
  yaml: 'yaml',
  toml: 'toml',
  ini: 'config',
  cfg: 'config',
  conf: 'config',
  properties: 'properties',
  env: 'env',
  py: 'python',
  pyi: 'python-compiled',
  rb: 'ruby',
  gem: 'ruby-gem',
  rake: 'ruby',
  php: 'php',
  phtml: 'phtml',
  java: 'java',
  class: 'java-class',
  jar: 'java-jar',
  kt: 'kotlin',
  kts: 'kotlin',
  scala: 'scala',
  groovy: 'groovy',
  clj: 'clojure',
  cljs: 'clojure',
  c: 'c',
  h: 'c-header',
  cpp: 'cpp',
  cc: 'cpp',
  cxx: 'cpp',
  hpp: 'cpp-header',
  hh: 'cpp-header',
  cs: 'csharp',
  go: 'go',
  rs: 'rust',
  swift: 'swift',
  zig: 'zig',
  lua: 'lua',
  luau: 'luau',
  dart: 'dart',
  ex: 'elixir',
  exs: 'elixir',
  erl: 'erlang',
  hrl: 'erlang',
  hs: 'haskell',
  haxe: 'haxe',
  gleam: 'gleam',
  nim: 'nim',
  cr: 'crystal',
  r: 'r',
  rdata: 'rdata',
  rproj: 'rproj',
  jl: 'julia',
  f: 'fortran',
  f90: 'fortran',
  f95: 'fortran',
  cob: 'cobol',
  pl: 'perl',
  pm: 'perl',
  asm: 'assembly',
  s: 'assembly',
  ps1: 'powershell',
  psm1: 'powershell',
  bat: 'batch',
  cmd: 'batch',
  sh: 'bash',
  bash: 'bash',
  zsh: 'bash',
  fish: 'bash',
  html: 'html',
  htm: 'html',
  css: 'css',
  scss: 'sass',
  sass: 'sass',
  less: 'less',
  styl: 'css',
  vue: 'vue',
  svelte: 'svelte',
  astro: 'astro',
  hbs: 'handlebars',
  handlebars: 'handlebars',
  mustache: 'handlebars',
  ejs: 'ejs',
  pug: 'pug',
  jade: 'pug',
  twig: 'twig',
  liquid: 'liquid',
  njk: 'nunjucks',
  jinja: 'jinja',
  jinja2: 'jinja',
  j2: 'jinja',
  razor: 'razor',
  cshtml: 'razor',
  xaml: 'xaml',
  xml: 'xml',
  svg: 'svg',
  org: 'org',
  tex: 'latex',
  latex: 'latex',
  bib: 'latex',
  rmd: 'rmd',
  ipynb: 'jupyter',
  sql: 'database',
  db: 'database',
  dat: 'database',
  sqlite: 'database',
  sqlite3: 'database',
  prisma: 'prisma',
  graphql: 'graphql',
  gql: 'graphql',
  proto: 'proto',
  tf: 'terraform',
  tfvars: 'terraform',
  hcl: 'terraform',
  nix: 'nix',
  kdl: 'kdl',
  cue: 'cue',
  vim: 'vim',
  sln: 'visual-studio',
  csproj: 'visual-studio',
  vbproj: 'visual-studio',
  gd: 'godot',
  tscn: 'godot',
  tres: 'godot-assets',
  unity: 'unity',
  asset: 'unity',
  http: 'http',
  rest: 'http',
  wasm: 'web-assembly',
  png: 'image',
  jpg: 'image',
  jpeg: 'image',
  gif: 'image',
  webp: 'image',
  avif: 'image',
  bmp: 'image',
  ico: 'favicon',
  tiff: 'image',
  psd: 'image',
  sketch: 'sketch',
  fig: 'figma',
  ai: 'adobe-ai',
  mp3: 'audio',
  wav: 'audio',
  flac: 'audio',
  ogg: 'audio',
  m4a: 'audio',
  aac: 'audio',
  mp4: 'video',
  mov: 'video',
  webm: 'video',
  avi: 'video',
  mkv: 'video',
  woff: 'font',
  woff2: 'font',
  ttf: 'font',
  otf: 'font',
  eot: 'font',
  zip: 'zip',
  tar: 'zip',
  gz: 'zip',
  '7z': 'zip',
  rar: 'zip',
  bz2: 'zip',
  xz: 'zip',
  exe: 'exe',
  dll: 'exe',
  msi: 'exe',
  so: 'binary',
  dylib: 'binary',
  bin: 'binary',
  pdf: 'pdf',
  csv: 'csv',
  tsv: 'csv',
  xls: 'ms-excel',
  xlsx: 'ms-excel',
  doc: 'ms-word',
  docx: 'ms-word',
  ppt: 'ms-powerpoint',
  pptx: 'ms-powerpoint',
  txt: 'text',
  log: 'log',
  lock: 'lock',
  diff: 'diff',
  patch: 'diff',
  pem: 'certificate',
  crt: 'certificate',
  cer: 'certificate',
  key: 'key'
}

/** Folder names, lower-cased. The `-open` twin is derived automatically. */
const FOLDER_NAMES = {
  node_modules: 'folder-node',
  src: 'folder-src',
  source: 'folder-src',
  lib: 'folder-lib',
  libs: 'folder-lib',
  dist: 'folder-dist',
  build: 'folder-dist',
  out: 'folder-dist',
  target: 'folder-dist',
  test: 'folder-tests',
  tests: 'folder-tests',
  __tests__: 'folder-tests',
  spec: 'folder-tests',
  e2e: 'folder-tests',
  docs: 'folder-docs',
  doc: 'folder-docs',
  documentation: 'folder-docs',
  public: 'folder-public',
  static: 'folder-public',
  assets: 'folder-assets',
  images: 'folder-images',
  img: 'folder-images',
  fonts: 'folder-fonts',
  styles: 'folder-styles',
  css: 'folder-styles',
  components: 'folder-components',
  hooks: 'folder-hooks',
  utils: 'folder-utils',
  helpers: 'folder-utils',
  types: 'folder-types',
  typings: 'folder-types',
  api: 'folder-api',
  server: 'folder-server',
  backend: 'folder-server',
  client: 'folder-client',
  frontend: 'folder-client',
  app: 'folder-app',
  apps: 'folder-app',
  config: 'folder-config',
  configs: 'folder-config',
  settings: 'folder-config',
  scripts: 'folder-scripts',
  shared: 'folder-shared',
  common: 'folder-shared',
  packages: 'folder-packages',
  vendor: 'folder-packages',
  templates: 'folder-templates',
  examples: 'folder-examples',
  example: 'folder-examples',
  samples: 'folder-examples',
  locales: 'folder-locales',
  i18n: 'folder-locales',
  routes: 'folder-routes',
  router: 'folder-routes',
  views: 'folder-views',
  pages: 'folder-views',
  screens: 'folder-views',
  layouts: 'folder-layouts',
  store: 'folder-redux',
  stores: 'folder-redux',
  redux: 'folder-redux',
  middleware: 'folder-middleware',
  models: 'folder-database',
  database: 'folder-database',
  db: 'folder-database',
  data: 'folder-database',
  migrations: 'folder-database',
  content: 'folder-content',
  coverage: 'folder-coverage',
  tmp: 'folder-temp',
  temp: 'folder-temp',
  cache: 'folder-temp',
  downloads: 'folder-download',
  uploads: 'folder-upload',
  kubernetes: 'folder-kubernetes',
  helm: 'folder-kubernetes',
  charts: 'folder-kubernetes',
  plugins: 'folder-plugins',
  extensions: 'folder-plugins',
  graphql: 'folder-graphql',
  proto: 'folder-proto',
  workflows: 'folder-workflows',
  '.git': 'folder-git',
  '.github': 'folder-github',
  '.gitlab': 'folder-gitlab',
  '.vscode': 'folder-vscode',
  '.devcontainer': 'folder-devcontainer',
  '.husky': 'folder-husky',
  '.config': 'folder-config',
  '.cursor': 'folder-cursor',
  '.circleci': 'folder-circle-ci',
  '.changeset': 'folder-config'
}

const collection = JSON.parse(await readFile(source, 'utf8'))
const bodies = new Map(Object.entries(collection.icons).map(([name, icon]) => [name, icon.body]))

// Iconify stores redirects separately; inline them so every name is a real body.
for (const [alias, entry] of Object.entries(collection.aliases ?? {})) {
  const parent = bodies.get(entry.parent)
  if (parent) bodies.set(alias, parent)
}

const available = new Set(bodies.keys())
const missing = new Set()

/** Keep only names the set actually contains, so a typo cannot break a row. */
const resolve = (name) => {
  if (available.has(name)) return name
  missing.add(name)
  return null
}

/** Drop the unknown names from a lookup table. */
const keepKnown = (table) => {
  const out = {}
  for (const [key, value] of Object.entries(table)) {
    const name = resolve(value)
    if (name) out[key] = name
  }
  return out
}

const fileNames = keepKnown(FILE_NAMES)
const fileExtensions = keepKnown(FILE_EXTENSIONS)
const folderNames = keepKnown(FOLDER_NAMES)
const folderNamesExpanded = {}

for (const [key, name] of Object.entries(folderNames)) {
  folderNamesExpanded[key] = resolve(`${name}-open`) ?? name
}

const trimmed = {
  file: resolve('file') ?? 'file',
  folder: resolve('folder') ?? 'folder',
  folderExpanded: resolve('folder-open') ?? 'folder',
  fileNames,
  fileExtensions,
  folderNames,
  folderNamesExpanded
}

// Emit every icon as a standalone SVG so the renderer can use plain <img>.
await rm(iconsOut, { recursive: true, force: true })
await mkdir(iconsOut, { recursive: true })

let written = 0
for (const [name, body] of bodies) {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16">${body}</svg>\n`
  await writeFile(join(iconsOut, `${name}.svg`), svg, 'utf8')
  written += 1
}

await mkdir(dirname(manifestOut), { recursive: true })
await writeFile(manifestOut, JSON.stringify(trimmed), 'utf8')

const mapped =
  Object.keys(trimmed.fileNames).length +
  Object.keys(trimmed.fileExtensions).length +
  Object.keys(trimmed.folderNames).length

console.log(`catppuccin icons: ${written} icons written to public/catppuccin-icons`)
console.log(
  `catppuccin icons: ${mapped} lookups written to src/lib/catppuccin-icons.json`
)

if (missing.size > 0) {
  console.warn(`catppuccin icons: skipped unknown names: ${[...missing].sort().join(', ')}`)
}
