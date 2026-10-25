/**
 * Stage the TUI binary as a Tauri sidecar.
 *
 * Tauri's `externalBin` expects `binaries/<name>-<target-triple><ext>`, so the
 * release build of `luna` has to be copied there before `tauri build` runs. The
 * installer then ships it next to the GUI, which is what makes `luna` available
 * in a terminal after installing the desktop app.
 *
 * Usage: node scripts/stage-tui.mjs [--triple <triple>]
 */
import { copyFile, mkdir, stat } from 'node:fs/promises'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { execFileSync } from 'node:child_process'

const here = dirname(fileURLToPath(import.meta.url))
const root = resolve(here, '..')

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
const source = join(root, 'target', 'release', isWindows ? 'luna.exe' : 'luna')
const destination = join(
  root,
  'gui',
  'src-tauri',
  'binaries',
  `luna-${triple}${isWindows ? '.exe' : ''}`
)

try {
  await stat(source)
} catch {
  console.error(`stage-tui: ${source} is missing — run \`cargo build --release -p luna-tui\` first`)
  process.exit(1)
}

await mkdir(dirname(destination), { recursive: true })
await copyFile(source, destination)
console.log(`stage-tui: ${source} -> ${destination}`)
