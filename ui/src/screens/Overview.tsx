import { AlertCircle, ArrowRight, Copy, RotateCcw, Type, X } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'
import { api, errorMessage, type Entry, type Overview as Data, type ScanItem } from '../api'
import type { Go } from '../App'
import { Dialog } from '../components/feedback'
import { useToast } from '../components/toast'
import { Button, Card, Empty, ScreenHeader } from '../components/ui'
import { UndoButton } from '../components/UndoButton'
import { describeEntry, KIND_LABEL, relativeTime, size } from '../format'

export function Overview({ version, go }: { version: number; go: Go }) {
  const toast = useToast()
  const [data, setData] = useState<Data | null>(null)
  const [plan, setPlan] = useState<ScanItem[] | null>(null)
  const [busy, setBusy] = useState(false)

  const load = useCallback(
    () => api.overview().then(setData, (e) => toast(errorMessage(e), 'error')),
    [toast],
  )
  useEffect(() => {
    void load()
  }, [load, version])

  const scan = async (dryRun: boolean) => {
    setBusy(true)
    try {
      const items = await api.scan(dryRun)
      if (dryRun) {
        setPlan(items)
      } else {
        const done = items.filter((i) => !i.error).length
        const failed = items.length - done
        toast(
          done || failed
            ? `${done} arquivo(s) organizado(s)${failed ? `, ${failed} erro(s)` : ''}`
            : 'Nada a organizar',
          failed ? 'error' : 'ok',
        )
        void load()
      }
    } catch (e) {
      toast(errorMessage(e), 'error')
    } finally {
      setBusy(false)
    }
  }

  const s = data?.stats
  const errors = s?.errors_week ?? 0
  const subtitle = data?.config_error
    ? `O config.toml tem um erro: ${data.config_error}`
    : errors
      ? `${errors} erro(s) nos últimos 7 dias.`
      : 'Tudo em ordem.'

  return (
    <div className="flex flex-col gap-6">
      <ScreenHeader
        title="Visão geral"
        subtitle={subtitle}
        actions={
          <>
            <Button disabled={busy} onClick={() => scan(true)}>
              Simular (dry-run)
            </Button>
            <Button variant="primary" disabled={busy} onClick={() => scan(false)}>
              Varrer agora
            </Button>
          </>
        }
      />

      <div className="grid grid-cols-4 gap-4">
        <Stat label="Organizados hoje" value={s?.organized_today ?? 0}>
          {s && <TodayDelta today={s.organized_today} yesterday={s.organized_yesterday} />}
        </Stat>
        <Stat label="Enviados à lixeira" value={s?.trashed_week ?? 0}>
          últimos 7 dias
        </Stat>
        <Stat label="Espaço liberado" value={size(s?.freed_week ?? 0)}>
          últimos 7 dias, das pastas monitoradas
        </Stat>
        <Stat label="Erros" value={errors} danger={errors > 0}>
          {errors > 0 ? (
            <button
              type="button"
              className="font-medium text-danger hover:underline"
              onClick={() => go('history', { filter: 'errors' })}
            >
              Ver no histórico →
            </button>
          ) : (
            'últimos 7 dias'
          )}
        </Stat>
      </div>

      <div className="grid grid-cols-[1fr_300px] gap-4">
        <Card className="min-w-0 py-1.5">
          <div className="flex items-center justify-between px-5 pt-3 pb-2">
            <h2 className="text-[15px] font-semibold">Atividade recente</h2>
            <button
              type="button"
              className="text-[13px] font-medium text-brand hover:underline"
              onClick={() => go('history')}
            >
              Ver tudo
            </button>
          </div>
          {data && data.recent.length === 0 && <Empty>Nenhuma ação ainda.</Empty>}
          {data?.recent.map((e) => (
            <Activity key={e.id} entry={e} onChange={load} onDetails={() => go('history', { filter: 'errors' })} />
          ))}
        </Card>

        <Card className="flex min-w-0 flex-col gap-3 p-4.5">
          <h2 className="text-[15px] font-semibold">Pastas monitoradas</h2>
          {data && data.folders.length === 0 && (
            <p className="text-xs text-ink-3">Nenhuma regra ainda. Crie uma na tela Regras.</p>
          )}
          {data?.folders.map((f) => (
            <div key={f.path} className="rounded-lg bg-subtle p-3">
              <div className="flex items-center justify-between gap-2">
                <span className="truncate text-[13px] font-medium">{f.path_display}</span>
                {f.exists ? (
                  <span className="rounded-full bg-brand-soft px-2 py-0.5 text-[10px] font-medium text-brand">
                    monitorando
                  </span>
                ) : (
                  <span className="rounded-full bg-warn-soft px-2 py-0.5 text-[10px] font-medium text-warn">
                    aguardando pasta
                  </span>
                )}
              </div>
              <p className="mt-1 text-[11px] text-ink-2">
                {f.exists
                  ? `${f.rules} ${f.rules === 1 ? 'regra' : 'regras'} · ${f.files} ${f.files === 1 ? 'arquivo' : 'arquivos'} · ${size(f.bytes)}`
                  : 'A pasta ainda não existe. O TidyFlow tenta de novo a cada 60 s.'}
              </p>
            </div>
          ))}
          <div className="flex-1" />
          <div className="rounded-lg border border-brand-border bg-brand-soft p-3">
            <p className="text-xs font-semibold text-brand">Atalho de desfazer</p>
            <p className="mt-1 text-[11px] text-ink-2">
              Ligue um atalho (ex.: Super + U) ao comando tidyflow undo para desfazer a última ação em
              qualquer lugar.
            </p>
          </div>
        </Card>
      </div>

      {plan && (
        <Dialog
          title="Simulação: o que a varredura faria"
          onClose={() => setPlan(null)}
          footer={
            <>
              <Button onClick={() => setPlan(null)}>Fechar</Button>
              {plan.length > 0 && (
                <Button
                  variant="primary"
                  onClick={() => {
                    setPlan(null)
                    void scan(false)
                  }}
                >
                  Varrer agora
                </Button>
              )}
            </>
          }
        >
          {plan.length === 0 ? (
            <Empty>Nada a fazer: nenhum arquivo casa com as regras.</Empty>
          ) : (
            <ul className="flex flex-col divide-y divide-line-soft">
              {plan.map((p) => (
                <li key={p.path} className="py-2.5">
                  <p className="text-[13px] font-medium">{p.path_display}</p>
                  <p className={`text-xs ${p.error ? 'text-danger' : 'text-ink-2'}`}>
                    {p.error
                      ? `Erro: ${p.error}`
                      : p.kind === 'trash' || !p.kind
                        ? `lixeira · ${p.rule}`
                        : `${KIND_LABEL[p.kind]} → ${p.dst_display} · ${p.rule}`}
                  </p>
                </li>
              ))}
            </ul>
          )}
        </Dialog>
      )}
    </div>
  )
}

function Stat({
  label,
  value,
  danger,
  children,
}: {
  label: string
  value: number | string
  danger?: boolean
  children: React.ReactNode
}) {
  return (
    <Card className="flex flex-col gap-1.5 p-4.5">
      <span className="text-[13px] text-ink-2">{label}</span>
      <span className={`text-3xl font-semibold tracking-tight ${danger ? 'text-danger' : 'text-ink'}`}>
        {value}
      </span>
      <span className="text-xs text-ink-3">{children}</span>
    </Card>
  )
}

export function TodayDelta({ today, yesterday }: { today: number; yesterday: number }) {
  const d = today - yesterday
  if (d === 0) return <>igual a ontem</>
  return d > 0 ? (
    <span className="font-medium text-brand">↑ {d} a mais que ontem</span>
  ) : (
    <span>↓ {-d} a menos que ontem</span>
  )
}

const ICON = {
  move: { icon: ArrowRight, style: 'bg-brand-soft text-brand' },
  copy: { icon: Copy, style: 'bg-info-soft text-info' },
  rename: { icon: Type, style: 'bg-purple-soft text-purple' },
  trash: { icon: X, style: 'bg-danger-soft text-danger' },
}

function Activity({
  entry,
  onChange,
  onDetails,
}: {
  entry: Entry
  onChange: () => void
  onDetails: () => void
}) {
  const { icon: Icon, style } = entry.error
    ? { icon: AlertCircle, style: 'bg-danger-soft text-danger' }
    : entry.undone
      ? { icon: RotateCcw, style: 'bg-line-soft text-ink-2' }
      : ICON[entry.kind]
  return (
    <div className="flex items-center gap-3 border-t border-line-soft px-5 py-3">
      <div className={`flex size-8 shrink-0 items-center justify-center rounded-lg ${style}`}>
        <Icon size={15} />
      </div>
      <div className="min-w-0 flex-1">
        <p className="truncate text-[13px] font-medium">{entry.name}</p>
        <p className={`truncate text-xs ${entry.error ? 'text-danger' : 'text-ink-2'}`}>
          {describeEntry(entry)}
          {!entry.error && <span className="text-ink-3"> · {entry.rule}</span>}
        </p>
      </div>
      <span className="shrink-0 text-xs text-ink-3">{relativeTime(entry.ts)}</span>
      <div className="w-24 shrink-0 text-right">
        <UndoButton entry={entry} onDone={onChange} onDetails={onDetails} />
      </div>
    </div>
  )
}
