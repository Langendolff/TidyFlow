import { act, render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import type { Entry, LibraryItem } from '../api'
import { callsOf, mockBackend } from '../test/tauri'
import { Library } from './Library'

const now = Math.floor(Date.now() / 1000)

const action: Entry = {
  id: 9,
  ts: now,
  rule: 'Faturas',
  kind: 'move',
  name: 'fatura.pdf',
  src: '/h/Downloads/fatura.pdf',
  src_display: '~/Downloads/fatura.pdf',
  dst: '/h/Docs/fatura.pdf',
  dst_display: '~/Docs/fatura.pdf',
  size: 1,
  undone: false,
  error: null,
}

const item = (id: number, over: Partial<LibraryItem> = {}): LibraryItem => ({
  id,
  name: `arquivo-${id}.pdf`,
  path: `/h/Docs/arquivo-${id}.pdf`,
  path_display: `~/Docs/arquivo-${id}.pdf`,
  folder_display: '~/Docs',
  source: 'browser',
  browser: 'firefox',
  url: 'https://cdn.github.com/x.pdf',
  referrer: 'https://github.com/a/b',
  page_title: 'Releases',
  site: 'github.com',
  category: 'document',
  size: 2000,
  state: 'complete',
  in_watch: false,
  started_at: now,
  last_action: null,
  ...over,
})

function backend(pages?: (args: Record<string, unknown>) => unknown) {
  mockBackend({
    library:
      pages ??
      (({ beforeId }) =>
        beforeId == null
          ? {
              items: [
                item(5, { last_action: action }),
                item(4, { state: 'gone', name: 'sumiu.pdf' }),
                item(3, { state: 'in_progress', name: 'baixando.iso', category: 'installer' }),
              ],
              has_more: true,
            }
          : { items: [item(1)], has_more: false }),
    library_sites: () => [
      { site: 'github.com', count: 3 },
      { site: 'nubank.com.br', count: 1 },
    ],
    open_url: () => null,
    open_file: () => null,
    undo: () => action,
  })
}

const go = vi.fn()

describe('Library', () => {
  it('shows items by state, downloading first', async () => {
    backend()
    render(<Library version={0} go={go} />)
    await screen.findByText('arquivo-5.pdf')
    const rows = screen.getAllByRole('row') // cabeçalho + 3
    expect(within(rows[1]).getByText('baixando.iso')).toBeInTheDocument()
    expect(within(rows[1]).getByText('baixando…')).toBeInTheDocument()
    const gone = screen.getByText('sumiu.pdf').closest('tr')!
    expect(within(gone).getByText('não encontrado')).toBeInTheDocument()
    expect(within(gone).queryByRole('button', { name: 'Abrir sumiu.pdf' })).toBeNull()
    expect(within(gone).queryByRole('button', { name: 'Abrir a pasta' })).toBeNull()
    const done = screen.getByText('arquivo-5.pdf').closest('tr')!
    expect(within(done).getByText('Faturas')).toBeInTheDocument()
    expect(within(done).getByRole('button', { name: /Desfazer/ })).toBeInTheDocument()
  })

  it('filters by place, site and type', async () => {
    backend()
    render(<Library version={0} go={go} />)
    await screen.findByText('arquivo-5.pdf')
    await userEvent.click(screen.getByRole('button', { name: 'Na lixeira' }))
    expect(callsOf('library').at(-1)?.args).toMatchObject({ place: 'trashed', beforeId: null })
    await userEvent.click(screen.getByRole('button', { name: 'github.com 3' }))
    expect(callsOf('library').at(-1)?.args).toMatchObject({ site: 'github.com' })
    await userEvent.click(screen.getByRole('button', { name: 'github.com 3' }))
    expect(callsOf('library').at(-1)?.args).toMatchObject({ site: null })
    await userEvent.selectOptions(screen.getByLabelText('Tipo'), 'installer')
    expect(callsOf('library').at(-1)?.args).toMatchObject({ category: 'installer' })
    await userEvent.selectOptions(screen.getByLabelText('Período'), 'today')
    expect(callsOf('library').at(-1)?.args.since).toEqual(expect.any(Number))
  })

  it('waits before searching', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    backend()
    render(<Library version={0} go={go} />)
    await screen.findByText('arquivo-5.pdf')
    const before = callsOf('library').length
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime })
    await user.type(screen.getByRole('searchbox'), 'fat')
    expect(callsOf('library')).toHaveLength(before)
    await act(async () => {
      vi.advanceTimersByTime(350)
    })
    expect(callsOf('library').at(-1)?.args).toMatchObject({ search: 'fat' })
    vi.useRealTimers()
  })

  it('loads more with the last id', async () => {
    backend()
    render(<Library version={0} go={go} />)
    await screen.findByText('arquivo-5.pdf')
    await userEvent.click(screen.getByRole('button', { name: 'Carregar mais' }))
    expect(await screen.findByText('arquivo-1.pdf')).toBeInTheDocument()
    expect(callsOf('library').at(-1)?.args).toMatchObject({ beforeId: 3 })
    expect(screen.queryByRole('button', { name: 'Carregar mais' })).toBeNull()
  })

  it('opens the source page and tests the file', async () => {
    backend(() => ({ items: [item(5)], has_more: false }))
    render(<Library version={0} go={go} />)
    await screen.findByText('arquivo-5.pdf')
    await userEvent.click(screen.getByRole('button', { name: 'Abrir a página de origem' }))
    expect(callsOf('open_url').at(-1)?.args).toEqual({ url: 'https://github.com/a/b' })
    await userEvent.click(screen.getByRole('button', { name: 'Testar no TidyFlow' }))
    expect(go).toHaveBeenCalledWith('test', { file: '/h/Docs/arquivo-5.pdf' })
  })

  it('never opens blob URLs', async () => {
    backend(() => ({
      items: [item(5, { url: 'blob:https://github.com/uuid', referrer: null })],
      has_more: false,
    }))
    render(<Library version={0} go={go} />)
    await screen.findByText('arquivo-5.pdf')
    expect(screen.queryByRole('button', { name: 'Abrir a página de origem' })).toBeNull()
    expect(screen.getByRole('button', { name: 'Copiar a URL' })).toBeInTheDocument()
  })

  it('empty state', async () => {
    backend(() => ({ items: [], has_more: false }))
    render(<Library version={0} go={go} />)
    expect(await screen.findByText(/Os arquivos aparecem aqui/)).toBeInTheDocument()
  })
})
