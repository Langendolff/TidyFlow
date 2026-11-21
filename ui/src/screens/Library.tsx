import { Copy, ExternalLink, FileSearch, FolderOpen, Play, Search, Sparkles } from 'lucide-react'
import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react'
import { api, errorMessage, type Category, type LibraryItem, type LibraryQuery, type Place, type Site } from '../api'
import type { Go } from '../App'
import { useToast } from '../components/toast'
import { Card, Chip, Empty, KindBadge, ScreenHeader } from '../components/ui'
import { UndoButton } from '../components/UndoButton'
import { CATEGORY_LABEL, libraryWhere, size, whenLabel } from '../format'

const PLACES: { id: Place; label: string }[] = [
  { id: 'all', label: 'Todos' },
  { id: 'in_watch', label: 'Na pasta de downloads' },
  { id: 'organized', label: 'Organizados' },
  { id: 'downloading', label: 'Baixando' },
  { id: 'trashed', label: 'Na lixeira' },
  { id: 'missing', label: 'Não encontrados' },
]

type Period = 'any' | 'today' | 'week' | 'month'

const PERIODS: { id: Period; label: string }[] = [
  { id: 'any', label: 'Qualquer data' },
  { id: 'today', label: 'Hoje' },
  { id: 'week', label: 'Últimos 7 dias' },
  { id: 'month', label: 'Últimos 30 dias' },
]

function sinceOf(p: Period, now = new Date()): number | null {
  const day = 86_400
  const midnight = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime() / 1000
  switch (p) {
    case 'any':
      return null
    case 'today':
      return midnight
    case 'week':
      return midnight - 6 * day
    case 'month':
      return midnight - 29 * day
  }
}

const DEBOUNCE = 300

/** Páginas desde o início até ter pelo menos `want` itens, mais os sites do filtro. */
async function fetchLibrary(q: Omit<LibraryQuery, 'beforeId'>, want: number) {
  let page = await api.library({ ...q, beforeId: null })
  const items = [...page.items]
  while (page.has_more && items.length < want) {
    page = await api.library({ ...q, beforeId: items[items.length - 1].id })
    items.push(...page.items)
  }
  return { items, hasMore: page.has_more, sites: await api.librarySites() }
}

const selectClass =
  'h-7 rounded-full border border-line bg-card px-3 text-xs font-medium text-ink-2 outline-none focus:border-brand'

/** Só http(s) abre no navegador; `blob:` e afins não. */
const webUrl = (u: string | null) => (u && /^https?:\/\//i.test(u) ? u : null)

function IconButton({ label, onClick, children }: { label: string; onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      className="grid size-7 place-items-center rounded-md text-ink-2 transition hover:bg-subtle hover:text-ink"
    >
      {children}
    </button>
  )
}

export function Library({ version, go }: { version: number; go: Go }) {
  const toast = useToast()
  const [place, setPlace] = useState<Place>('all')
  const [category, setCategory] = useState<Category | ''>('')
  const [site, setSite] = useState<string | null>(null)
  const [period, setPeriod] = useState<Period>('any')
  const [search, setSearch] = useState('')
  const [query, setQuery] = useState('')
  const [items, setItems] = useState<LibraryItem[] | null>(null)
  const [hasMore, setHasMore] = useState(false)
  const [sites, setSites] = useState<Site[]>([])
  const loaded = useRef(0)

  useEffect(() => {
    const t = setTimeout(() => setQuery(search), DEBOUNCE)
    return () => clearTimeout(t)
  }, [search])

  const base = useCallback(
    () => ({ place, category: category || null, site, since: sinceOf(period), search: query }),
    [place, category, site, period, query],
  )

  /** Recarrega desde o início, mantendo quantos itens já estavam abertos. */
  const reload = useCallback(
    () =>
      fetchLibrary(base(), loaded.current).then(
        (r) => {
          loaded.current = r.items.length
          setItems(r.items)
          setHasMore(r.hasMore)
          setSites(r.sites)
        },
        (e) => toast(errorMessage(e), 'error'),
      ),
    [base, toast],
  )

  useEffect(() => {
    loaded.current = 0
  }, [base])

  useEffect(() => {
    void reload()
  }, [reload, version])

  const more = async () => {
    if (!items?.length) return
    try {
      const page = await api.library({ ...base(), beforeId: items[items.length - 1].id })
      loaded.current = items.length + page.items.length
      setItems([...items, ...page.items])
      setHasMore(page.has_more)
    } catch (e) {
      toast(errorMessage(e), 'error')
    }
  }

  const run = (p: Promise<unknown>) => p.catch((e) => toast(errorMessage(e), 'error'))

  const organize = async (i: LibraryItem) => {
    try {
      const e = await api.organize(i.path)
      toast(`${i.name} organizado pela regra "${e.rule}"`)
      void reload()
    } catch (e) {
      toast(errorMessage(e), 'error')
    }
  }

  const copyUrl = async (url: string) => {
    try {
      await navigator.clipboard.writeText(url)
      toast('URL copiada')
    } catch {
      toast('Não foi possível copiar a URL', 'error')
    }
  }

  // Downloads em andamento primeiro; o resto na ordem de chegada.
  const rows = items ? [...items].sort((a, b) => Number(b.state === 'in_progress') - Number(a.state === 'in_progress')) : []
  const filtered = place !== 'all' || category || site || period !== 'any' || query

  return (
    <div className="flex flex-col gap-6">
      <ScreenHeader
        title="Biblioteca"
        subtitle="Tudo o que chegou nas pastas monitoradas: de onde veio, onde está agora e o que o TidyFlow fez."
      />

      <div className="flex flex-col gap-3">
        <div className="flex flex-wrap items-center gap-2">
          <label className="relative mr-2 w-72">
            <Search size={14} className="absolute top-1/2 left-3 -translate-y-1/2 text-ink-3" />
            <input
              type="search"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Buscar nome, URL ou página…"
              className="h-9 w-full rounded-lg border border-input bg-card pr-3 pl-8 text-[13px] outline-none focus:border-brand"
            />
          </label>
          {PLACES.map((p) => (
            <Chip key={p.id} active={place === p.id} onClick={() => setPlace(p.id)}>
              {p.label}
            </Chip>
          ))}
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <select
            aria-label="Tipo"
            value={category}
            onChange={(e) => setCategory(e.target.value as Category | '')}
            className={selectClass}
          >
            <option value="">Todos os tipos</option>
            {(Object.keys(CATEGORY_LABEL) as Category[]).map((c) => (
              <option key={c} value={c}>
                {CATEGORY_LABEL[c]}
              </option>
            ))}
          </select>
          <select
            aria-label="Período"
            value={period}
            onChange={(e) => setPeriod(e.target.value as Period)}
            className={selectClass}
          >
            {PERIODS.map((p) => (
              <option key={p.id} value={p.id}>
                {p.label}
              </option>
            ))}
          </select>
          {sites.length > 0 && <span className="ml-2 text-xs text-ink-3">Sites:</span>}
          {sites.map((s) => (
            <Chip key={s.site} active={site === s.site} onClick={() => setSite(site === s.site ? null : s.site)}>
              {s.site} {s.count}
            </Chip>
          ))}
        </div>
      </div>

      <Card className="overflow-hidden">
        <table className="w-full table-fixed text-left text-[13px]">
          <thead>
            <tr className="border-b border-line text-[11px] font-medium tracking-wide text-ink-3 uppercase">
              <th className="px-5 py-3 font-medium">Arquivo</th>
              <th className="w-28 py-3 font-medium">Tipo</th>
              <th className="w-24 py-3 font-medium">Tamanho</th>
              <th className="w-32 py-3 font-medium">Chegou</th>
              <th className="w-48 py-3 font-medium">Onde está</th>
              <th className="w-40 py-3 font-medium">Última ação</th>
              <th className="w-76 px-5 py-3" />
            </tr>
          </thead>
          <tbody>
            {rows.map((i) => {
              const here = i.state === 'complete'
              const page = webUrl(i.referrer) ?? webUrl(i.url)
              const action = i.last_action
              return (
                <tr key={i.id} className="border-b border-line-soft last:border-0">
                  <td className="min-w-0 px-5 py-3">
                    <p className={`truncate font-medium ${i.state === 'gone' ? 'text-ink-3' : ''}`} title={i.path_display}>
                      {i.name}
                    </p>
                    <p className="truncate text-xs text-ink-3" title={i.url ?? undefined}>
                      {[i.site ?? 'origem desconhecida', i.page_title].filter(Boolean).join(' · ')}
                    </p>
                  </td>
                  <td className="py-3 text-ink-2">{CATEGORY_LABEL[i.category]}</td>
                  <td className="py-3 text-ink-2">{i.size != null ? size(i.size) : '—'}</td>
                  <td className="py-3 text-ink-2">{whenLabel(i.started_at)}</td>
                  <td className="min-w-0 py-3 pr-4">
                    <p
                      className={`truncate ${i.state === 'in_progress' ? 'font-medium text-brand' : i.state === 'gone' ? 'text-ink-3' : 'text-ink-2'}`}
                      title={i.path_display}
                    >
                      {libraryWhere(i)}
                    </p>
                    {i.state === 'complete' && i.in_watch && (
                      <p className="truncate text-xs text-ink-3">ainda não organizado</p>
                    )}
                  </td>
                  <td className="min-w-0 py-3 pr-4">
                    {action ? (
                      <div className="flex min-w-0 items-center gap-1.5">
                        <KindBadge entry={action} />
                        <span className="truncate text-xs text-ink-3" title={action.rule}>
                          {action.rule}
                        </span>
                      </div>
                    ) : (
                      <span className="text-xs text-ink-3">nenhuma</span>
                    )}
                  </td>
                  <td className="px-5 py-3">
                    <div className="flex items-center justify-end gap-0.5">
                      {here && (
                        <>
                          <IconButton label={`Abrir ${i.name}`} onClick={() => run(api.openFile(i.path))}>
                            <Play size={14} />
                          </IconButton>
                          <IconButton label="Abrir a pasta" onClick={() => run(api.reveal(i.path))}>
                            <FolderOpen size={14} />
                          </IconButton>
                          <IconButton label="Testar no TidyFlow" onClick={() => go('test', { file: i.path })}>
                            <FileSearch size={14} />
                          </IconButton>
                          <IconButton label="Organizar agora" onClick={() => void organize(i)}>
                            <Sparkles size={14} />
                          </IconButton>
                        </>
                      )}
                      {page && (
                        <IconButton label="Abrir a página de origem" onClick={() => run(api.openUrl(page))}>
                          <ExternalLink size={14} />
                        </IconButton>
                      )}
                      {i.url && (
                        <IconButton label="Copiar a URL" onClick={() => void copyUrl(i.url!)}>
                          <Copy size={14} />
                        </IconButton>
                      )}
                      {action && (here || i.state === 'trashed') && (
                        <span className="ml-1">
                          <UndoButton entry={action} onDone={reload} />
                        </span>
                      )}
                    </div>
                  </td>
                </tr>
              )
            })}
          </tbody>
        </table>
        {items && items.length === 0 && (
          <Empty>
            {filtered
              ? 'Nenhum arquivo encontrado.'
              : 'Nada ainda. Os arquivos aparecem aqui quando chegam numa pasta monitorada.'}
          </Empty>
        )}
        {hasMore && (
          <div className="flex justify-end border-t border-line px-5 py-3 text-xs">
            <button type="button" className="font-medium text-brand hover:underline" onClick={more}>
              Carregar mais
            </button>
          </div>
        )}
      </Card>
    </div>
  )
}
