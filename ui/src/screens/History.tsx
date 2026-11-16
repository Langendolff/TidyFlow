import { RotateCcw, Search } from 'lucide-react'
import { useCallback, useEffect, useRef, useState } from 'react'
import { api, errorMessage, type Counts, type Entry, type Filter } from '../api'
import { Dialog } from '../components/feedback'
import { useToast } from '../components/toast'
import { Button, Card, Chip, Empty, KindBadge, ScreenHeader } from '../components/ui'
import { UndoButton } from '../components/UndoButton'
import { historyDetail, historyName, whenLabel } from '../format'

const FILTERS: { id: Filter; label: string; count?: boolean; tone?: 'danger' }[] = [
  { id: 'all', label: 'Todas', count: true },
  { id: 'moved', label: 'Movidos' },
  { id: 'trashed', label: 'Lixeira' },
  { id: 'undone', label: 'Desfeitos' },
  { id: 'errors', label: 'Erros', count: true, tone: 'danger' },
]

const DEBOUNCE = 250

/** Páginas desde o início até ter pelo menos `want` linhas. */
async function fetchPages(filter: Filter, query: string, want: number) {
  let page = await api.history(filter, query, null)
  const entries = [...page.entries]
  while (page.has_more && entries.length < want) {
    page = await api.history(filter, query, entries[entries.length - 1].id)
    entries.push(...page.entries)
  }
  return { entries, page }
}

export function History({ version, initialFilter }: { version: number; initialFilter: Filter }) {
  const toast = useToast()
  const [filter, setFilter] = useState<Filter>(initialFilter)
  const [search, setSearch] = useState('')
  const [query, setQuery] = useState('')
  const [entries, setEntries] = useState<Entry[]>([])
  const [counts, setCounts] = useState<Counts | null>(null)
  const [hasMore, setHasMore] = useState(false)
  const [details, setDetails] = useState<Entry | null>(null)
  const loaded = useRef(0)

  useEffect(() => {
    const t = setTimeout(() => setQuery(search), DEBOUNCE)
    return () => clearTimeout(t)
  }, [search])

  /** Recarrega a primeira página, mantendo quantas linhas já estavam abertas. */
  const reload = useCallback(
    () =>
      fetchPages(filter, query, loaded.current).then(
        ({ entries, page }) => {
          loaded.current = entries.length
          setEntries(entries)
          setCounts(page.counts)
          setHasMore(page.has_more)
        },
        (e) => toast(errorMessage(e), 'error'),
      ),
    [filter, query, toast],
  )

  useEffect(() => {
    loaded.current = 0
  }, [filter, query])

  useEffect(() => {
    void reload()
  }, [reload, version])

  const more = async () => {
    const last = entries[entries.length - 1]
    try {
      const page = await api.history(filter, query, last.id)
      loaded.current = entries.length + page.entries.length
      setEntries((e) => [...e, ...page.entries])
      setCounts(page.counts)
      setHasMore(page.has_more)
    } catch (e) {
      toast(errorMessage(e), 'error')
    }
  }

  const undoLast = async () => {
    try {
      const e = await api.undo(null)
      toast(`Desfeito: ${e.name} voltou para a pasta original`)
      void reload()
    } catch (e) {
      toast(`Não foi possível desfazer: ${errorMessage(e)}`, 'error')
    }
  }

  const total = counts?.[filter] ?? 0

  return (
    <div className="flex flex-col gap-6">
      <ScreenHeader
        title="Histórico"
        subtitle="Toda ação fica registrada e pode ser desfeita."
        actions={
          <Button onClick={undoLast}>
            <RotateCcw size={14} /> Desfazer a última
          </Button>
        }
      />

      <div className="flex items-center gap-2">
        <label className="relative mr-2 w-72">
          <Search size={14} className="absolute top-1/2 left-3 -translate-y-1/2 text-ink-3" />
          <input
            type="search"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Buscar arquivo ou regra…"
            className="h-9 w-full rounded-lg border border-input bg-card pr-3 pl-8 text-[13px] outline-none focus:border-brand"
          />
        </label>
        {FILTERS.map((f) => (
          <Chip key={f.id} active={filter === f.id} tone={f.tone} onClick={() => setFilter(f.id)}>
            {f.label}
            {f.count && counts ? ` ${counts[f.id]}` : ''}
          </Chip>
        ))}
      </div>

      <Card className="overflow-hidden">
        <table className="w-full table-fixed text-left text-[13px]">
          <thead>
            <tr className="border-b border-line text-[11px] font-medium tracking-wide text-ink-3 uppercase">
              <th className="w-20 px-5 py-3 font-medium">ID</th>
              <th className="w-32 py-3 font-medium">Quando</th>
              <th className="w-28 py-3 font-medium">Ação</th>
              <th className="py-3 font-medium">Arquivo</th>
              <th className="w-48 py-3 font-medium">Regra</th>
              <th className="w-32 px-5 py-3" />
            </tr>
          </thead>
          <tbody>
            {entries.map((e) => (
              <tr
                key={e.id}
                className={`border-b border-line-soft last:border-0 ${e.error ? 'bg-danger-soft/60' : ''}`}
              >
                <td className="px-5 py-3 text-ink-3">#{e.id}</td>
                <td className="py-3 text-ink">{whenLabel(e.ts)}</td>
                <td className="py-3">
                  <KindBadge entry={e} />
                </td>
                <td className="min-w-0 py-3 pr-4">
                  <p className={`truncate font-medium ${e.undone ? 'text-ink-3 line-through' : ''}`}>
                    {historyName(e)}
                  </p>
                  <p className={`truncate text-xs ${e.error ? 'text-danger' : 'text-ink-3'}`}>
                    {historyDetail(e)}
                  </p>
                </td>
                <td className="truncate py-3 pr-4 text-ink-2">{e.rule}</td>
                <td className="px-5 py-3 text-right">
                  <UndoButton entry={e} onDone={reload} onDetails={setDetails} />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {counts && entries.length === 0 && (
          <Empty>{query || filter !== 'all' ? 'Nenhuma ação encontrada.' : 'Nenhuma ação ainda.'}</Empty>
        )}
        {entries.length > 0 && (
          <div className="flex items-center justify-between border-t border-line px-5 py-3 text-xs text-ink-2">
            <span>
              Mostrando {entries.length} de {total} {total === 1 ? 'ação' : 'ações'}
            </span>
            {hasMore && (
              <button type="button" className="font-medium text-brand hover:underline" onClick={more}>
                Carregar mais
              </button>
            )}
          </div>
        )}
      </Card>

      {details && (
        <Dialog
          title={`Ação #${details.id} falhou`}
          onClose={() => setDetails(null)}
          footer={<Button onClick={() => setDetails(null)}>Fechar</Button>}
        >
          <dl className="grid grid-cols-[120px_1fr] gap-x-4 gap-y-2 text-[13px]">
            <dt className="text-ink-2">Erro</dt>
            <dd className="text-danger select-text">{details.error}</dd>
            <dt className="text-ink-2">Arquivo</dt>
            <dd className="break-all select-text">{details.src}</dd>
            <dt className="text-ink-2">Regra</dt>
            <dd>{details.rule}</dd>
            <dt className="text-ink-2">Quando</dt>
            <dd>{whenLabel(details.ts)}</dd>
          </dl>
          <p className="mt-4 text-xs text-ink-3">
            O arquivo ficou onde estava. Nada foi movido nem sobrescrito.
          </p>
        </Dialog>
      )}
    </div>
  )
}
