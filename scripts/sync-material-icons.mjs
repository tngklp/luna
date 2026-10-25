/**
 * Copy the Material Icon Theme SVGs into the renderer's public directory and
 * emit a trimmed manifest that the renderer imports.
 *
 * Generated output is gitignored: `material-icon-theme` is the pinned source of
 * truth. Wired into `predev` / `prebuild`, and runnable via `npm run icons`.
 */
import { cp, mkdir, readFile, rm, writeFile } from 'node:fs/promises'
import { dirname, join } from 'node:path'

const root = process.cwd()
const source = join(root, 'node_modules', 'material-icon-theme')
const iconsOut = join(root, 'public', 'material-icons')
const manifestOut = join(root, 'src', 'lib', 'material-icons.json')

const manifest = JSON.parse(await readFile(join(source, 'dist', 'material-icons.json'), 'utf8'))

// Only the lookups the explorer needs, so the bundled JSON stays small.
const trimmed = {
  file: manifest.file,
  folder: manifest.folder,
  folderExpanded: manifest.folderExpanded,
  fileNames: manifest.fileNames,
  fileExtensions: manifest.fileExtensions,
  folderNames: manifest.folderNames,
  folderNamesExpanded: manifest.folderNamesExpanded
}

await rm(iconsOut, { recursive: true, force: true })
await cp(join(source, 'icons'), iconsOut, { recursive: true })

await mkdir(dirname(manifestOut), { recursive: true })
await writeFile(manifestOut, JSON.stringify(trimmed), 'utf8')

const iconCount = Object.keys(manifest.iconDefinitions ?? {}).length
console.log(`material icons: ${iconCount} definitions synced to public/material-icons`)
console.log(`material icons: manifest written to src/lib/material-icons.json`)
