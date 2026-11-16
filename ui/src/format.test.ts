import { describe, expect, it } from 'vitest'
import type { RuleDraft } from './api'
import { describeEntry, historyDetail, historyName, prettyDuration, relativeTime, ruleSummary, size, whenLabel } from './format'

const now = new Date(2026, 10, 16, 15, 0, 0) // 16 nov 2026 15:00 local
const ts = (d: Date) => Math.floor(d.getTime() / 1000)
const ago = (secs: number) => ts(now) - secs

describe('relativeTime', () => {
  it('formats', () => {
    expect(relativeTime(ago(10), now)).toBe('agora')
    expect(relativeTime(ago(4 * 60), now)).toBe('há 4 min')
    expect(relativeTime(ago(3600), now)).toBe('há 1 h')
    expect(relativeTime(ts(new Date(2026, 10, 15, 23, 0)), now)).toBe('ontem')
    expect(relativeTime(ts(new Date(2026, 10, 12, 22, 15)), now)).toBe('12 nov 22:15')
  })
})

describe('whenLabel', () => {
  it('formats', () => {
    expect(whenLabel(ts(new Date(2026, 10, 16, 14, 32)), now)).toBe('hoje 14:32')
    expect(whenLabel(ts(new Date(2026, 10, 15, 18, 47)), now)).toBe('ontem 18:47')
    expect(whenLabel(ts(new Date(2026, 10, 12, 22, 15)), now)).toBe('12 nov 22:15')
    expect(whenLabel(ts(new Date(2025, 0, 2, 9, 5)), now)).toBe('2 jan 2025 09:05')
  })
})

describe('size', () => {
  it('mirrors the core', () => {
    expect(size(999)).toBe('999 B')
    expect(size(100_000_000)).toBe('100 MB')
    expect(size(3_200_000_000)).toBe('3,2 GB')
    expect(size(1_500)).toBe('1,5 KB')
  })
})

describe('ruleSummary', () => {
  const base: RuleDraft = {
    name: 'r',
    enabled: true,
    watch: ['~/Downloads'],
    when: [],
    action: { kind: 'move', value: '~/x' },
  }
  it('summarizes conditions and action', () => {
    expect(ruleSummary({ ...base, when: [{ key: 'origin', value: '*.nubank.com.br' }] })).toBe(
      'origem *.nubank.com.br → mover',
    )
    expect(
      ruleSummary({
        ...base,
        when: [
          { key: 'ext', value: 'pdf,xml' },
          { key: 'name_regex', value: 'nota' },
        ],
      }),
    ).toBe('pdf, xml · nome ~ /nota/ → mover')
    expect(
      ruleSummary({
        ...base,
        enabled: false,
        when: [{ key: 'older_than', value: '30d' }],
        action: { kind: 'trash' },
      }),
    ).toBe('> 30 dias → lixeira · pausada')
    expect(ruleSummary({ ...base, when: [{ key: 'size_gt', value: '100MB' }] })).toBe(
      '> 100 MB → mover',
    )
  })
  it('durations', () => {
    expect(prettyDuration('1d')).toBe('1 dia')
    expect(prettyDuration('2w')).toBe('2 semanas')
    expect(prettyDuration('12h')).toBe('12 h')
    expect(prettyDuration('xyz')).toBe('xyz')
  })
})

describe('describe', () => {
  const e = {
    id: 1,
    ts: 0,
    rule: 'r',
    kind: 'move' as const,
    name: 'a.pdf',
    src: '/h/Downloads/a.pdf',
    src_display: '~/Downloads/a.pdf',
    dst: '/h/Docs/2026/a.pdf',
    dst_display: '~/Docs/2026/a.pdf',
    size: 4_700_000_000,
    undone: false,
    error: null,
  }
  it('per kind', () => {
    expect(describeEntry(e)).toBe('Movido para ~/Docs/2026/')
    expect(describeEntry({ ...e, kind: 'copy' })).toBe('Copiado para ~/Docs/2026/')
    expect(describeEntry({ ...e, kind: 'rename', dst_display: '~/Downloads/2026-a.pdf' })).toBe(
      'Renomeado para 2026-a.pdf',
    )
    expect(describeEntry({ ...e, kind: 'trash', dst_display: null })).toBe('Enviado à lixeira · 4,7 GB')
    expect(describeEntry({ ...e, error: 'sem permissão' })).toBe('Erro: sem permissão')
    expect(describeEntry({ ...e, undone: true })).toBe(
      'Desfeito · voltou para ~/Downloads/ e não será mais organizado',
    )
  })
})

describe('history rows', () => {
  const e = {
    id: 1,
    ts: 0,
    rule: 'r',
    kind: 'move' as const,
    name: 'a.pdf',
    src: '/h/Downloads/a.pdf',
    src_display: '~/Downloads/a.pdf',
    dst: '/h/Docs/2026/a.pdf',
    dst_display: '~/Docs/2026/a.pdf',
    size: 4_700_000_000,
    undone: false,
    error: null,
  }
  it('detail', () => {
    expect(historyDetail(e)).toBe('~/Downloads → ~/Docs/2026')
    expect(historyDetail({ ...e, kind: 'trash', dst_display: null })).toBe('~/Downloads → Lixeira · 4,7 GB')
    expect(historyDetail({ ...e, undone: true })).toBe('de volta em ~/Downloads · marcado para manter')
    expect(historyDetail({ ...e, error: 'destino já existe' })).toBe('destino já existe')
    const rn = { ...e, kind: 'rename' as const, dst_display: '~/Downloads/2026-a.pdf' }
    expect(historyDetail(rn)).toBe('antes: a.pdf')
    expect(historyName(rn)).toBe('2026-a.pdf')
    expect(historyName(e)).toBe('a.pdf')
  })
})
