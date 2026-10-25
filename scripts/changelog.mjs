#!/usr/bin/env node
/**
 * Extract one release's notes from CHANGELOG.md.
 *
 * The changelog is the single source of truth for release notes, so the release
 * workflow reads it rather than keeping a second copy in the YAML.
 *
 * Usage:
 *   node scripts/changelog.mjs 0.1.0            # print the notes
 *   node scripts/changelog.mjs v0.1.0 --check   # exit 1 when the section is missing
 */
import { readFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const root = resolve(here, '..')

/** Accept `0.1.0`, `v0.1.0` and `[0.1.0]`, and return the bare version. */
export function normalizeVersion(raw) {
  const trimmed = String(raw ?? '')
    .trim()
    .replace(/^\[/, '')
    .replace(/\]$/, '')
    .replace(/^v/i, '')
  if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(trimmed)) {
    throw new Error(`"${raw}" is not a version like 0.1.0`)
  }
  return trimmed
}

/**
 * Pull the body of `## [<version>]` out of the changelog.
 *
 * Stops at the next `## ` heading or at the link-definition block at the bottom,
 * so the trailing `[0.1.0]: https://…` lines never leak into the notes.
 */
export function extractNotes(changelog, version) {
  const lines = changelog.split('\n')
  const escaped = version.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  const heading = new RegExp(`^##\\s*\\[?v?${escaped}\\]?(?:\\s|$)`)

  const start = lines.findIndex((line) => heading.test(line))
  if (start === -1) return null

  let end = lines.length
  for (let index = start + 1; index < lines.length; index += 1) {
    if (/^##\s/.test(lines[index])) {
      end = index
      break
    }
    if (/^\[[^\]]+\]:\s/.test(lines[index])) {
      end = index
      break
    }
  }

  return lines.slice(start + 1, end).join('\n').trim()
}

function main() {
  const args = process.argv.slice(2)
  const check = args.includes('--check')
  const raw = args.find((arg) => !arg.startsWith('--'))

  if (!raw) {
    console.error('usage: node scripts/changelog.mjs <version> [--check]')
    process.exit(2)
  }

  let version
  try {
    version = normalizeVersion(raw)
  } catch (error) {
    console.error(`changelog: ${error.message}`)
    process.exit(2)
  }

  const changelog = readFileSync(join(root, 'CHANGELOG.md'), 'utf8')
  const notes = extractNotes(changelog, version)

  if (!notes) {
    console.error(`changelog: CHANGELOG.md has no "## [${version}]" section`)
    process.exit(check ? 1 : 2)
  }

  process.stdout.write(`${notes}\n`)
}

// Only run when invoked directly, so the helpers stay importable from tests.
if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  main()
}
