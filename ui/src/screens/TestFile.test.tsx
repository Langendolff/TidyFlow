import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import type { Inspect } from '../api'
import { callsOf, mockBackend } from '../test/tauri'
import { TestFile } from './TestFile'

const data: Inspect = {
  facts: {
    path: '/h/Downloads/fatura.pdf',
    path_display: '~/Downloads/fatura.pdf',
    name: 'fatura.pdf',
    ext: 'pdf',
    mime: 'application/pdf',
    origin_url: 'https://app.nubank.com.br/x',
    origin_host: 'app.nubank.com.br',
    size: 184_000,
    modified: Math.floor(Date.now() / 1000) - 240,
    keep: false,
  },
  rules: [
    {
      name: 'Notas fiscais',
      enabled: true,
      verdict: 'fails',
      checks: [{ key: 'name_regex', expected: '(?i)nota', ok: false }],
    },
    {
      name: 'Faturas Nubank',
      enabled: true,
      verdict: 'wins',
      checks: [
        { key: 'origin', expected: '*.nubank.com.br', ok: true },
        { key: 'mime', expected: 'application/pdf', ok: true },
      ],
    },
    {
      name: 'Limpeza geral',
      enabled: true,
      verdict: 'untested',
      checks: [{ key: 'older_than', expected: '> 30 dias', ok: false }],
    },
  ],
  result: { rule: 'Faturas Nubank', kind: 'move', dst: '/h/Docs/fatura.pdf', dst_display: '~/Docs/fatura.pdf' },
}

describe('TestFile', () => {
  it('shows facts, verdicts and the result', async () => {
    mockBackend({ inspect: () => data })
    render(<TestFile file="/h/Downloads/fatura.pdf" onFile={vi.fn()} />)
    expect(await screen.findByText('app.nubank.com.br')).toBeInTheDocument()
    expect(screen.getByText('184 KB')).toBeInTheDocument()
    expect(screen.getByText('vence')).toBeInTheDocument()
    expect(screen.getByText('não casa')).toBeInTheDocument()
    expect(screen.getByText('não testada')).toBeInTheDocument()
    expect(screen.getByText('✓ origem *.nubank.com.br')).toBeInTheDocument()
    expect(screen.getByText('× > 30 dias')).toBeInTheDocument()
    expect(screen.getByText('mover → ~/Docs/fatura.pdf')).toBeInTheDocument()
  })

  it('apply organizes the file', async () => {
    mockBackend({
      inspect: () => data,
      organize: () => ({ id: 42, name: 'fatura.pdf', dst_display: '~/Docs/fatura.pdf' }),
    })
    render(<TestFile file="/h/Downloads/fatura.pdf" onFile={vi.fn()} />)
    await userEvent.click(await screen.findByText('Aplicar agora'))
    expect(callsOf('organize')[0].args).toEqual({ path: '/h/Downloads/fatura.pdf' })
    expect(await screen.findByText('Feito (#42)')).toBeDisabled()
  })

  it('no rule matches', async () => {
    mockBackend({ inspect: () => ({ ...data, rules: [], result: null }) })
    render(<TestFile file="/h/x" onFile={vi.fn()} />)
    expect(await screen.findByText('Nenhuma regra casaria: o arquivo fica onde está.')).toBeInTheDocument()
    expect(screen.queryByText('Aplicar agora')).toBeNull()
  })
})
