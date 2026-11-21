import { homeDir } from '@tauri-apps/api/path'
import { open } from '@tauri-apps/plugin-dialog'
import { Plus, X } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { api, type Condition, type DraftAction, type Kind, type Preview, type RuleDraft } from '../api'
import { Button, Card } from '../components/ui'
import { CONDITION_KEYS } from '../format'
import { ConditionRow, inputClass } from './ConditionRow'

const TABS: { kind: Kind; label: string }[] = [
  { kind: 'move', label: 'Mover' },
  { kind: 'copy', label: 'Copiar' },
  { kind: 'rename', label: 'Renomear' },
  { kind: 'trash', label: 'Lixeira' },
]

const VARS = ['{year}', '{month}', '{day}', '{name}', '{ext}', '{origin_host}', '{site}']

const TARGET_LABEL: Record<Exclude<Kind, 'trash'>, string> = {
  move: 'Pasta de destino',
  copy: 'Pasta de destino',
  rename: 'Novo nome',
}

const PREVIEW_DEBOUNCE = 400

/** Pasta escolhida no seletor, com `~` no lugar do home. */
async function pickFolder(): Promise<string | null> {
  const dir = await open({ directory: true, multiple: false })
  if (typeof dir !== 'string') return null
  const home = (await homeDir()).replace(/\/$/, '')
  return dir === home ? '~' : dir.startsWith(`${home}/`) ? `~${dir.slice(home.length)}` : dir
}

export function RuleEditor({
  draft,
  savedName,
  isNew,
  dirty,
  error,
  busy,
  onChange,
  onSave,
  onDiscard,
  onDelete,
}: {
  draft: RuleDraft
  /** nome com que a regra está salva (a prévia procura no histórico por ele) */
  savedName: string | null
  isNew: boolean
  dirty: boolean
  error: string | null
  busy: boolean
  onChange: (d: RuleDraft) => void
  onSave: () => void
  onDiscard: () => void
  onDelete: () => void
}) {
  const target = useRef<HTMLInputElement>(null)
  const lastValue = useRef('')
  const [preview, setPreview] = useState<Preview | null>(null)

  useEffect(() => {
    if (!savedName) return
    let alive = true
    const t = setTimeout(() => {
      api.preview(savedName, draft).then(
        (p) => alive && setPreview(p),
        () => alive && setPreview(null),
      )
    }, PREVIEW_DEBOUNCE)
    return () => {
      alive = false
      clearTimeout(t)
    }
  }, [savedName, draft])

  const set = (patch: Partial<RuleDraft>) => onChange({ ...draft, ...patch })
  const setCondition = (i: number, c: Condition) =>
    set({ when: draft.when.map((x, j) => (j === i ? c : x)) })
  const used = draft.when.map((c) => c.key)
  const free = CONDITION_KEYS.find((k) => !used.includes(k))

  const setKind = (kind: Kind) => {
    if (kind === draft.action.kind) return
    if (draft.action.kind !== 'trash') lastValue.current = draft.action.value
    const value = kind === 'rename' && !lastValue.current.includes('{') ? '{name}.{ext}' : lastValue.current
    const action: DraftAction = kind === 'trash' ? { kind } : { kind, value }
    set({ action })
  }

  const insertVar = (v: string) => {
    if (draft.action.kind === 'trash') return
    const el = target.current
    const value = draft.action.value
    const start = el?.selectionStart ?? value.length
    const end = el?.selectionEnd ?? value.length
    set({ action: { ...draft.action, value: value.slice(0, start) + v + value.slice(end) } })
    requestAnimationFrame(() => {
      el?.focus()
      el?.setSelectionRange(start + v.length, start + v.length)
    })
  }

  return (
    <Card className="flex min-w-0 flex-col gap-5 p-6">
      {error && (
        <div role="alert" className="rounded-lg border border-danger/30 bg-danger-soft px-3 py-2 text-[13px] text-danger">
          {error}
        </div>
      )}

      <Field label="Nome da regra">
        <input
          aria-label="Nome da regra"
          className={inputClass}
          value={draft.name}
          onChange={(e) => set({ name: e.target.value })}
        />
      </Field>

      <Field label="Pastas monitoradas">
        <div className="flex flex-wrap gap-2">
          {draft.watch.map((w, i) => (
            <span key={w} className="inline-flex min-h-7 max-w-full items-center gap-1.5 rounded-md border border-line bg-subtle py-1 pr-1 pl-2.5 text-xs break-all">
              {w}
              <button
                type="button"
                aria-label={`Remover ${w}`}
                className="text-ink-3 hover:text-ink"
                onClick={() => set({ watch: draft.watch.filter((_, j) => j !== i) })}
              >
                <X size={12} />
              </button>
            </span>
          ))}
          <button
            type="button"
            className="inline-flex h-7 items-center gap-1 rounded-md border border-dashed border-input px-2.5 text-xs text-ink-2 hover:bg-subtle"
            onClick={async () => {
              const dir = await pickFolder()
              if (dir && !draft.watch.includes(dir)) set({ watch: [...draft.watch, dir] })
            }}
          >
            <Plus size={12} /> Adicionar pasta
          </button>
        </div>
      </Field>

      <Field label="Quando" hint="todas as condições casarem">
        <div className="flex flex-col gap-2">
          {draft.when.map((c, i) => (
            <ConditionRow
              key={i}
              condition={c}
              used={used.filter((_, j) => j !== i)}
              onChange={(c) => setCondition(i, c)}
              onRemove={() => set({ when: draft.when.filter((_, j) => j !== i) })}
            />
          ))}
          {free && (
            <button
              type="button"
              className="self-start text-[13px] font-medium text-brand hover:underline"
              onClick={() => set({ when: [...draft.when, { key: free, value: '' }] })}
            >
              + Adicionar condição
            </button>
          )}
        </div>
      </Field>

      <Field label="Então">
        <div role="tablist" className="inline-flex self-start rounded-lg bg-line-soft p-0.5">
          {TABS.map((t) => (
            <button
              key={t.kind}
              type="button"
              role="tab"
              aria-selected={draft.action.kind === t.kind}
              onClick={() => setKind(t.kind)}
              className={`h-8 rounded-md px-3.5 text-[13px] font-medium ${
                draft.action.kind === t.kind ? 'bg-card text-ink shadow-xs' : 'text-ink-2 hover:text-ink'
              }`}
            >
              {t.label}
            </button>
          ))}
        </div>
        {draft.action.kind === 'trash' ? (
          <p className="text-xs text-ink-3">
            O arquivo vai para a lixeira do sistema e pode ser restaurado pelo Desfazer.
          </p>
        ) : (
          <>
            <input
              ref={target}
              aria-label={TARGET_LABEL[draft.action.kind]}
              className={`${inputClass} font-mono text-xs`}
              value={draft.action.value}
              placeholder={draft.action.kind === 'rename' ? '{year}-{name}.{ext}' : '~/Documentos/{year}/'}
              onChange={(e) =>
                draft.action.kind !== 'trash' &&
                set({ action: { ...draft.action, value: e.target.value } })
              }
            />
            <div className="flex flex-wrap items-center gap-1.5 text-xs">
              <span className="text-ink-3">Inserir:</span>
              {VARS.map((v) => (
                <button
                  key={v}
                  type="button"
                  onMouseDown={(e) => e.preventDefault()}
                  onClick={() => insertVar(v)}
                  className="rounded-md bg-purple-soft px-1.5 py-0.5 font-mono text-[11px] text-purple hover:brightness-95"
                >
                  {v}
                </button>
              ))}
            </div>
          </>
        )}
      </Field>

      {preview && (
        <div className="rounded-lg bg-subtle px-4 py-3 font-mono text-xs">
          <p className="mb-1.5 font-sans text-[10px] font-semibold tracking-wide text-ink-3 uppercase">
            Prévia com o último arquivo que casou
          </p>
          <p className="text-ink-2">{preview.src_display}</p>
          <p className="text-brand">→ {preview.dst_display}</p>
        </div>
      )}

      <div className="flex-1" />
      <div className="flex items-center gap-2.5 border-t border-line pt-4">
        {!isNew && (
          <Button variant="danger" onClick={onDelete} disabled={busy}>
            Excluir regra
          </Button>
        )}
        <div className="flex-1" />
        <Button onClick={onDiscard} disabled={busy || !dirty}>
          Descartar
        </Button>
        <Button variant="primary" onClick={onSave} disabled={busy || !dirty}>
          {isNew ? 'Criar regra' : 'Salvar regra'}
        </Button>
      </div>
    </Card>
  )
}

function Field({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-2">
      <span className="text-xs font-medium text-ink-2">
        {label}
        {hint && <span className="ml-1.5 font-normal text-ink-3">{hint}</span>}
      </span>
      {children}
    </div>
  )
}
