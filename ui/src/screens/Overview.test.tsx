import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { callsOf, mockBackend } from '../test/tauri'
import type { Overview as Data } from '../api'
import { Overview } from './Overview'

const data: Data = {
  stats: {
    organized_today: 24,
    organized_yesterday: 16,
    trashed_week: 12,
    freed_week: 3_200_000_000,
    errors_week: 1,
  },
  recent: [
    {
      id: 128,
      ts: Math.floor(Date.now() / 1000) - 240,
      rule: 'Faturas Nubank',
      kind: 'move',
      name: 'fatura-2026-11.pdf',
      src: '/h/Downloads/fatura-2026-11.pdf',
      src_display: '~/Downloads/fatura-2026-11.pdf',
      dst: '/h/Docs/fatura-2026-11.pdf',
      dst_display: '~/Docs/fatura-2026-11.pdf',
      size: 1,
      undone: false,
      error: null,
    },
  ],
  folders: [
    { path: '/h/Downloads', path_display: '~/Downloads', exists: true, rules: 5, files: 38, bytes: 1_900_000_000 },
    { path: '/h/Desktop', path_display: '~/Desktop', exists: false, rules: 1, files: 0, bytes: 0 },
  ],
  config_error: null,
}

describe('Overview', () => {
  it('shows stats, activity and folders', async () => {
    mockBackend({ overview: () => data })
    const go = vi.fn()
    render(<Overview version={0} go={go} />)
    expect(await screen.findByText('24')).toBeInTheDocument()
    expect(screen.getByText('↑ 8 a mais que ontem')).toBeInTheDocument()
    expect(screen.getByText('3,2 GB')).toBeInTheDocument()
    expect(screen.getByText('fatura-2026-11.pdf')).toBeInTheDocument()
    expect(screen.getByText('há 4 min')).toBeInTheDocument()
    expect(screen.getByText('5 regras · 38 arquivos · 1,9 GB')).toBeInTheDocument()
    expect(screen.getByText('aguardando pasta')).toBeInTheDocument()

    await userEvent.click(screen.getByText('Ver no histórico →'))
    expect(go).toHaveBeenCalledWith('history', { filter: 'errors' })
  })

  it('undo calls the backend and reloads', async () => {
    mockBackend({ overview: () => data, undo: () => ({ ...data.recent[0], undone: true }) })
    render(<Overview version={0} go={vi.fn()} />)
    await userEvent.click(await screen.findByRole('button', { name: /Desfazer/ }))
    expect(callsOf('undo')[0].args).toEqual({ id: 128 })
    await vi.waitFor(() => expect(callsOf('overview').length).toBe(2))
  })

  it('dry-run opens the plan', async () => {
    mockBackend({
      overview: () => data,
      scan: () => [
        { path: '/h/Downloads/a.iso', path_display: '~/Downloads/a.iso', rule: 'Limpeza', kind: 'trash', dst_display: null, error: null },
      ],
    })
    render(<Overview version={0} go={vi.fn()} />)
    await userEvent.click(await screen.findByText('Simular (dry-run)'))
    const dialog = await screen.findByRole('dialog')
    expect(callsOf('scan')[0].args).toEqual({ dryRun: true })
    expect(within(dialog).getByText('lixeira · Limpeza')).toBeInTheDocument()
  })
})
