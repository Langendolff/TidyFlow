import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import type { Entry } from '../api'
import { callsOf, mockBackend } from '../test/tauri'
import { History } from './History'

const entry = (id: number, over: Partial<Entry> = {}): Entry => ({
  id,
  ts: Math.floor(Date.now() / 1000),
  rule: 'Notas fiscais',
  kind: 'move',
  name: `arquivo-${id}.pdf`,
  src: `/h/Downloads/arquivo-${id}.pdf`,
  src_display: `~/Downloads/arquivo-${id}.pdf`,
  dst: `/h/Docs/arquivo-${id}.pdf`,
  dst_display: `~/Docs/arquivo-${id}.pdf`,
  size: 1,
  undone: false,
  error: null,
  ...over,
})

const counts = { all: 3, moved: 1, trashed: 0, undone: 1, errors: 1 }

function backend() {
  mockBackend({
    history: ({ beforeId }) =>
      beforeId == null
        ? {
            entries: [entry(3), entry(2, { undone: true }), entry(1, { error: 'destino já existe' })],
            counts: { ...counts, all: 4 },
            has_more: true,
          }
        : { entries: [entry(0)], counts: { ...counts, all: 4 }, has_more: false },
  })
}

describe('History', () => {
  it('renders rows by state', async () => {
    backend()
    render(<History version={0} initialFilter="all" />)
    expect(await screen.findByText('arquivo-3.pdf')).toBeInTheDocument()
    expect(screen.getByText('arquivo-2.pdf')).toHaveClass('line-through')
    expect(screen.getAllByRole('button', { name: /Desfazer$/ })).toHaveLength(1)
    expect(screen.getByRole('button', { name: 'Detalhes' })).toBeInTheDocument()
    expect(screen.getByText('Mostrando 3 de 4 ações')).toBeInTheDocument()
  })

  it('filters, searches and paginates', async () => {
    backend()
    render(<History version={0} initialFilter="all" />)
    await screen.findByText('arquivo-3.pdf')
    await userEvent.click(screen.getByRole('button', { name: 'Lixeira' }))
    await vi.waitFor(() => expect(callsOf('history').at(-1)?.args.filter).toBe('trashed'))

    await userEvent.type(screen.getByPlaceholderText('Buscar arquivo ou regra…'), 'nubank')
    await vi.waitFor(() => expect(callsOf('history').at(-1)?.args.search).toBe('nubank'))

    await userEvent.click(await screen.findByText('Carregar mais'))
    await vi.waitFor(() => expect(callsOf('history').at(-1)?.args.beforeId).toBe(1))
    expect(await screen.findByText('arquivo-0.pdf')).toBeInTheDocument()
  })

  it('details shows the error', async () => {
    backend()
    render(<History version={0} initialFilter="errors" />)
    await userEvent.click(await screen.findByRole('button', { name: 'Detalhes' }))
    expect(await screen.findByRole('dialog')).toHaveTextContent('destino já existe')
    expect(callsOf('history')[0].args.filter).toBe('errors')
  })
})
