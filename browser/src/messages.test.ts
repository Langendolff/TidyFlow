import { createHash } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import { APPROVAL_NOTES, GECKO_ID, metadata } from '../amo.mjs'
import { onChanged, onCreated, titleFor, type DownloadLike } from './messages'

const item = (over: Partial<DownloadLike> = {}): DownloadLike => ({
  id: 7,
  filename: '/home/u/Downloads/a.pdf',
  url: 'https://example.com/get?id=1',
  finalUrl: 'https://cdn.example.com/a.pdf',
  referrer: 'https://example.com/page',
  mime: 'application/pdf',
  incognito: false,
  startTime: '2026-11-21T10:00:00.000Z',
  fileSize: 1234,
  ...over,
})

describe('onCreated', () => {
  it('never reports private windows', () => {
    expect(onCreated(item({ incognito: true }), 'firefox', null)).toBeNull()
  })

  it('waits for a file name', () => {
    expect(onCreated(item({ filename: '' }), 'chromium', null)).toBeNull()
  })

  it('builds started', () => {
    expect(onCreated(item(), 'firefox', 'Página')).toEqual({
      type: 'started',
      browser: 'firefox',
      id: 7,
      path: '/home/u/Downloads/a.pdf',
      url: 'https://example.com/get?id=1',
      final_url: 'https://cdn.example.com/a.pdf',
      referrer: 'https://example.com/page',
      page_title: 'Página',
      mime: 'application/pdf',
      started_at: 1795255200,
    })
  })

  it('drops empty and redundant fields', () => {
    const m = onCreated(item({ finalUrl: 'https://example.com/get?id=1', referrer: '', mime: '' }), 'firefox', '')
    expect(m).toMatchObject({ final_url: null, referrer: null, mime: null, page_title: null })
  })
})

describe('onChanged', () => {
  it('complete → finished with the size', () => {
    expect(onChanged({ id: 7, state: { current: 'complete' } }, item(), 'firefox', true, null)).toEqual([
      { type: 'finished', browser: 'firefox', id: 7, path: '/home/u/Downloads/a.pdf', size: 1234 },
    ])
    const [m] = onChanged({ id: 7, state: { current: 'complete' } }, item({ fileSize: undefined, totalBytes: 9 }), 'firefox', true, null)
    expect(m).toMatchObject({ size: 9 })
  })

  it('interrupted carries the error', () => {
    expect(
      onChanged({ id: 7, state: { current: 'interrupted' }, error: { current: 'USER_CANCELED' } }, item(), 'chromium', true, null),
    ).toEqual([{ type: 'interrupted', browser: 'chromium', id: 7, error: 'USER_CANCELED' }])
  })

  it('rename then finish, in that order', () => {
    const msgs = onChanged(
      { id: 7, filename: { current: '/d/b.pdf' }, state: { current: 'complete' } },
      item(),
      'chromium',
      true,
      null,
    )
    expect(msgs.map((m) => m.type)).toEqual(['renamed', 'finished'])
    expect(msgs[1]).toMatchObject({ path: '/d/b.pdf' })
  })

  it('first file name on Chromium starts the download', () => {
    const msgs = onChanged({ id: 7, filename: { current: '/d/b.pdf' } }, item({ filename: '' }), 'chromium', false, 'T')
    expect(msgs).toHaveLength(1)
    expect(msgs[0]).toMatchObject({ type: 'started', path: '/d/b.pdf', page_title: 'T' })
  })

  it('finish without a recorded start sends started first', () => {
    const msgs = onChanged({ id: 7, state: { current: 'complete' } }, item(), 'firefox', false, null)
    expect(msgs.map((m) => m.type)).toEqual(['started', 'finished'])
  })

  it('ignores private and unknown items', () => {
    expect(onChanged({ id: 7, state: { current: 'complete' } }, item({ incognito: true }), 'firefox', true, null)).toEqual([])
    expect(onChanged({ id: 7, state: { current: 'complete' } }, undefined, 'firefox', true, null)).toEqual([])
    expect(onChanged({ id: 7 }, item(), 'firefox', true, null)).toEqual([])
  })
})

describe('titleFor', () => {
  const tabs = [{ url: 'https://other.com/' , title: 'Outra' }, { url: 'https://example.com/page#top', title: 'Página' }]

  it('finds the tab by URL, ignoring the fragment', () => {
    expect(titleFor('https://example.com/page', tabs)).toBe('Página')
    expect(titleFor('https://example.com/page#x', tabs)).toBe('Página')
  })

  it('no referrer or no tab → null', () => {
    expect(titleFor(undefined, tabs)).toBeNull()
    expect(titleFor('https://nope.com/', tabs)).toBeNull()
  })
})

describe('Chromium extension ID', () => {
  it('matches the key in the manifest and the constant in the host', () => {
    const manifest = JSON.parse(readFileSync(new URL('../manifest.base.json', import.meta.url), 'utf8'))
    const hex = createHash('sha256').update(Buffer.from(manifest.key, 'base64')).digest('hex').slice(0, 32)
    const id = [...hex].map((c) => String.fromCharCode(97 + parseInt(c, 16))).join('')
    const rust = readFileSync(new URL('../../crates/cli/src/browser.rs', import.meta.url), 'utf8')
    expect(rust).toContain(`pub const CHROMIUM_ID: &str = "${id}";`)
  })
})

describe('AMO', () => {
  it('uses the same Firefox ID as the host', () => {
    const rust = readFileSync(new URL('../../crates/cli/src/browser.rs', import.meta.url), 'utf8')
    expect(rust).toContain(`pub const FIREFOX_ID: &str = "${GECKO_ID}";`)
  })

  it('tells the reviewer about the native host and the source build', () => {
    const m = metadata('texto da licença')
    expect(m.version.approval_notes).toBe(APPROVAL_NOTES)
    expect(m.version.custom_license.name['en-US']).toBe('PolyForm Noncommercial 1.0.0')
    expect(m.version.custom_license.text['en-US']).toBe('texto da licença')
    // Resumo do AMO: até 250 caracteres.
    for (const s of Object.values(m.summary)) expect(s.length).toBeLessThanOrEqual(250)
    expect(metadata().version.custom_license.text['en-US']).toContain('PolyForm Noncommercial')
    expect(APPROVAL_NOTES).toContain('io.github.langendolff.tidyflow')
    expect(APPROVAL_NOTES).toContain('npm ci && npm run build')
  })

  it('listed builds have no self-hosted update URL', () => {
    const build = readFileSync(new URL('../build.mjs', import.meta.url), 'utf8')
    expect(build).not.toContain('update_url')
  })
})
