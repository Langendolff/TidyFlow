import { getCurrentWebview } from '@tauri-apps/api/webview'
import { open } from '@tauri-apps/plugin-dialog'
import { Check as CheckIcon, FileText, Minus, Pause, X } from 'lucide-react'
import { useEffect, useState } from 'react'
import { api, errorMessage, type Check, type ConditionKey, type Inspect, type RuleCheck } from '../api'
import { useToast } from '../components/toast'
import { Button, Card, ScreenHeader } from '../components/ui'
import { KIND_LABEL, relativeTime, size } from '../format'

const CHECK_LABEL: Record<ConditionKey, string> = {
  ext: 'ext',
  name: 'nome',
  name_regex: 'nome',
  mime: 'mime',
  origin: 'origem',
  size_gt: '',
  size_lt: '',
  older_than: '',
}

export function TestFile({ file, onFile }: { file: string | null; onFile: (f: string) => void }) {
  const toast = useToast()
  const [data, setData] = useState<Inspect | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [applied, setApplied] = useState<number | null>(null)
  const [over, setOver] = useState(false)

  useEffect(() => {
    if (!file) return
    let alive = true
    api.inspect(file).then(
      (d) => {
        if (!alive) return
        setData(d)
        setError(null)
        setApplied(null)
      },
      (e) => {
        if (!alive) return
        setData(null)
        setError(errorMessage(e))
      },
    )
    return () => {
      alive = false
    }
  }, [file])

  // Arrastar um arquivo para a janela.
  useEffect(() => {
    const off = getCurrentWebview().onDragDropEvent((e) => {
      const p = e.payload
      if (p.type === 'enter' || p.type === 'over') setOver(true)
      else setOver(false)
      if (p.type === 'drop' && p.paths[0]) onFile(p.paths[0])
    })
    return () => {
      void off.then((f) => f())
    }
  }, [onFile])

  const choose = async () => {
    const f = await open({ multiple: false, directory: false })
    if (typeof f === 'string') onFile(f)
  }

  const apply = async () => {
    if (!file) return
    try {
      const e = await api.organize(file)
      setApplied(e.id)
      toast(`#${e.id}: ${e.name} → ${e.dst_display ?? 'lixeira'}`)
    } catch (e) {
      toast(errorMessage(e), 'error')
    }
  }

  const facts = data?.facts
  return (
    <div className="flex flex-col gap-6">
      <ScreenHeader
        title="Testar arquivo"
        subtitle="Veja o que o TidyFlow lê de um arquivo e qual regra casaria, sem mexer em nada."
      />

      <div
        className={`flex items-center gap-3 rounded-xl border-2 border-dashed px-5 py-4 transition ${
          over ? 'border-brand bg-brand-soft' : 'border-input bg-card'
        }`}
      >
        <div className="flex size-10 shrink-0 items-center justify-center rounded-lg bg-danger-soft text-[10px] font-bold text-danger uppercase">
          {facts?.ext ? facts.ext.slice(0, 4) : <FileText size={16} />}
        </div>
        <div className="min-w-0 flex-1">
          <p className="truncate text-[13px] font-medium">
            {facts?.path_display ?? (file ? file : 'Nenhum arquivo escolhido')}
          </p>
          <p className="text-xs text-ink-3">
            {file ? 'Solte outro arquivo aqui para testar' : 'Solte um arquivo aqui ou escolha um'}
          </p>
        </div>
        <Button onClick={choose}>Escolher arquivo…</Button>
      </div>

      {error && (
        <Card className="p-5">
          <p role="alert" className="text-[13px] text-danger">
            {error}
          </p>
        </Card>
      )}

      {data && facts && (
        <div className="grid grid-cols-[320px_1fr] items-start gap-4">
          <Card className="p-5">
            <h2 className="mb-3 text-[15px] font-semibold">O que foi lido do arquivo</h2>
            <dl className="flex flex-col text-[13px]">
              <Row label="Extensão" value={facts.ext || '(nenhuma)'} />
              <Row label="Tipo (pelo conteúdo)" value={facts.mime} />
              <Row
                label="Origem do download"
                value={facts.origin_host ?? 'desconhecida'}
                title={facts.origin_url ?? undefined}
                accent={!!facts.origin_host}
              />
              <Row label="Tamanho" value={size(facts.size)} />
              <Row label="Modificado" value={relativeTime(facts.modified)} />
              <Row label="Marcado para manter" value={facts.keep ? 'sim' : 'não'} />
            </dl>
            {facts.keep && (
              <p className="mt-3 text-xs text-ink-3">
                Um undo marcou este arquivo: o daemon não mexe nele. "Aplicar agora" ignora a marca.
              </p>
            )}
          </Card>

          <Card className="flex min-w-0 flex-col gap-2.5 p-5">
            <h2 className="mb-1 text-[15px] font-semibold">Regras, na ordem em que são testadas</h2>
            {data.rules.length === 0 && <p className="text-xs text-ink-3">Nenhuma regra no config.</p>}
            {data.rules.map((r, i) => (
              <RuleRow key={r.name} rule={r} index={i} />
            ))}
            {data.rules.some((r) => r.verdict === 'untested') && (
              <p className="text-[11px] text-ink-3">
                Depois que uma regra casa, as seguintes não são testadas. As marcas acima mostram o
                que aconteceria se fossem.
              </p>
            )}
            <div className="mt-3 flex items-center gap-4 rounded-xl bg-side px-5 py-4 text-white">
              <div className="min-w-0 flex-1">
                <p className="text-[10px] font-semibold tracking-wide text-brand-light uppercase">
                  Resultado
                </p>
                <p className="mt-1 truncate font-mono text-xs">
                  {data.result
                    ? `${KIND_LABEL[data.result.kind]} → ${data.result.dst_display ?? 'lixeira'}`
                    : 'Nenhuma regra casaria: o arquivo fica onde está.'}
                </p>
              </div>
              {data.result && (
                <Button variant="primary" onClick={apply} disabled={applied !== null}>
                  {applied !== null ? `Feito (#${applied})` : 'Aplicar agora'}
                </Button>
              )}
            </div>
          </Card>
        </div>
      )}
    </div>
  )
}

function Row({
  label,
  value,
  title,
  accent,
}: {
  label: string
  value: string
  title?: string
  accent?: boolean
}) {
  return (
    <div className="flex justify-between gap-4 border-b border-line-soft py-2.5 last:border-0">
      <dt className="text-ink-2">{label}</dt>
      <dd title={title} className={`truncate text-right ${accent ? 'text-brand' : 'text-ink'}`}>
        {value}
      </dd>
    </div>
  )
}

const VERDICT = {
  wins: { badge: 'vence', style: 'bg-brand text-white', box: 'border-brand ring-1 ring-brand bg-brand-soft/40' },
  fails: { badge: 'não casa', style: 'bg-line-soft text-ink-2', box: 'border-line' },
  untested: { badge: 'não testada', style: 'bg-line-soft text-ink-3', box: 'border-line' },
  paused: { badge: 'pausada', style: 'bg-line-soft text-ink-3', box: 'border-line opacity-60' },
}

function RuleRow({ rule, index }: { rule: RuleCheck; index: number }) {
  const v = VERDICT[rule.verdict]
  const Icon = rule.verdict === 'wins' ? CheckIcon : rule.verdict === 'paused' ? Pause : rule.verdict === 'fails' ? X : Minus
  return (
    <div className={`flex items-center gap-3 rounded-xl border px-4 py-3 ${v.box}`}>
      <span
        className={`flex size-6 shrink-0 items-center justify-center rounded-full ${
          rule.verdict === 'wins' ? 'bg-brand text-white' : 'bg-line-soft text-ink-3'
        }`}
      >
        <Icon size={13} strokeWidth={3} />
      </span>
      <div className="min-w-0 flex-1">
        <p className="text-[13px] font-semibold">
          {index + 1} · {rule.name}
        </p>
        <p className="mt-0.5 flex flex-wrap gap-x-3 text-[11px]">
          {rule.checks.map((c) => (
            <CheckMark key={c.key} check={c} strong={rule.verdict === 'wins' || rule.verdict === 'fails'} />
          ))}
        </p>
      </div>
      <span className={`shrink-0 rounded-full px-2 py-0.5 text-[10px] font-medium ${v.style}`}>{v.badge}</span>
    </div>
  )
}

function CheckMark({ check, strong }: { check: Check; strong: boolean }) {
  const color = check.ok ? (strong ? 'text-brand' : 'text-ink-2') : strong ? 'text-danger' : 'text-ink-3'
  const label = CHECK_LABEL[check.key]
  return (
    <span className={color}>
      {check.ok ? '✓' : '×'} {label && `${label} `}
      {check.expected}
    </span>
  )
}
