/**
 * Tests for the changelog extractor.
 *
 * Run with: node --test scripts/
 */
import assert from 'node:assert/strict'
import { test } from 'node:test'

import { extractNotes, normalizeVersion } from './changelog.mjs'

const CHANGELOG = `# Changelog

Intro text.

## [0.2.0] — 2026-11-02

### Added
- A second release.

## [0.1.0] — 2026-10-25

First release.

### Added
- The first release.

[0.2.0]: https://example.com/v0.2.0
[0.1.0]: https://example.com/v0.1.0
`

test('normalizes the accepted version spellings', () => {
  assert.equal(normalizeVersion('0.1.0'), '0.1.0')
  assert.equal(normalizeVersion('v0.1.0'), '0.1.0')
  assert.equal(normalizeVersion('V0.1.0'), '0.1.0')
  assert.equal(normalizeVersion('[0.1.0]'), '0.1.0')
  assert.equal(normalizeVersion(' 1.2.3-beta.1 '), '1.2.3-beta.1')
})

test('rejects things that are not versions', () => {
  assert.throws(() => normalizeVersion('bad'))
  assert.throws(() => normalizeVersion('1.2'))
  assert.throws(() => normalizeVersion(''))
})

test('extracts only the requested section', () => {
  const notes = extractNotes(CHANGELOG, '0.1.0')
  assert.match(notes, /First release\./)
  assert.match(notes, /The first release\./)
  assert.doesNotMatch(notes, /A second release\./)
})

test('stops before the next heading', () => {
  const notes = extractNotes(CHANGELOG, '0.2.0')
  assert.match(notes, /A second release\./)
  assert.doesNotMatch(notes, /First release\./)
})

test('never leaks the link definitions', () => {
  for (const version of ['0.1.0', '0.2.0']) {
    const notes = extractNotes(CHANGELOG, version)
    assert.doesNotMatch(notes, /example\.com/)
    assert.doesNotMatch(notes, /^\[0\./m)
  }
})

test('returns null for a version that is not there', () => {
  assert.equal(extractNotes(CHANGELOG, '9.9.9'), null)
})

test('handles a section with no body', () => {
  const sparse = '## [1.0.0]\n\n## [0.9.0]\n\n- old\n'
  assert.equal(extractNotes(sparse, '1.0.0'), '')
})
