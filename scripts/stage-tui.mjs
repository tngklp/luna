/**
 * Stage the TUI binary as a Tauri sidecar.
 *
 * Tauri's `externalBin` expects `binaries/<name>-<target-triple><ext>`, and its
 * build script validates that the file exists — even for `cargo check`. So the
 * release build of `luna` has to be copied there before any Cargo command runs
 * in `gui/src-tauri`. The installer then ships it next to the GUI, which is what
 * makes `luna` available in a terminal after installing the desktop app.
 *
 * Usage:
 *   node scripts/stage-tui.mjs                 # strict: fail if the TUI is missing
 *   node scripts/stage-tui.mjs --optional      # write a placeholder instead
 *   node scripts/stage-tui.mjs --triple <t>    # stage for a specific target
 *
 * `--optional` exists for `cargo check` and `tauri dev`, where the sidecar is
 * never executed. Release builds must use the strict form so a placeholder can
 * never be bundled.
 */
import { copyFile, mkdir, stat, writeFile } from 'node:fs/promises'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { execFileSync } from 'node:child_process'

const here = dirname(fileURLToPath(import.meta.url))
const root = resolve(here, '..')

const optional = process.argv.includes('--optional')

function targetTriple() {
  const flag = process.argv.indexOf('--triple')
  if (flag !== -1 && process.argv[flag + 1]) return process.argv[flag + 1]
  if (process.env.LUNA_TARGET_TRIPLE) return process.env.LUNA_TARGET_TRIPLE
  // Ask rustc, which is authoritative and already required to build the TUI.
  return execFileSync('rustc', ['-vV'], { encoding: 'utf8' })
    .split('\n')
    .find((line) => line.startsWith('host:'))
    .slice('host:'.length)
    .trim()
}

const triple = targetTriple()
const isWindows = triple.includes('windows')
const binary = isWindows ? 'luna.exe' : 'luna'

// A cross-target build lands in `target/<triple>/release/`; a host build in
// `target/release/`. Try the specific directory first so a stale host binary is
// never staged for a different target.
const candidates = [
  join(root, 'target', triple, 'release', binary),
  join(root, 'target', 'release', binary)
]

const destination = join(
  root,
  'gui',
  'src-tauri',
  'binaries',
  `luna-${triple}${isWindows ? '.exe' : ''}`
)

await mkdir(dirname(destination), { recursive: true })

let source = null
for (const candidate of candidates) {
  try {
    await stat(candidate)
    source = candidate
    break
  } catch {
    // Try the next location.
  }
}

if (source) {
  await copyFile(source, destination)
  console.log(`stage-tui: ${source} -> ${destination}`)
} else if (!optional) {
  console.error(
    `stage-tui: no ${binary} for ${triple} — run \`cargo build --release -p luna-tui --target ${triple}\` first`
  )
  process.exit(1)
} else {
  // A placeholder satisfies Tauri's existence check without pretending to be a
  // runnable binary. It is only ever written for check/dev, never for a bundle.
  await writeFile(destination, 'luna sidecar placeholder — run `npm run sidecar` to replace\n')
  console.warn(`stage-tui: no ${binary} for ${triple}; wrote a placeholder at ${destination}`)
  console.warn('stage-tui: run `cargo build --release -p luna-tui` before bundling')
}
