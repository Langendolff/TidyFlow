import { Plus } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'
import { api, errorMessage, isCmdError, type RuleDraft, type RulesState } from '../api'
import { Dialog } from '../components/feedback'
import { useToast } from '../components/toast'
import { Button, Card, Empty, ScreenHeader } from '../components/ui'
import { RuleEditor } from './RuleEditor'
import { RuleList } from './RuleList'

const NEW_RULE: RuleDraft = {
  name: 'Nova regra',
  enabled: true,
  watch: ['~/Downloads'],
  when: [{ key: 'ext', value: '' }],
  action: { kind: 'move', value: '' },
}

/**
 * A regra aberta é identificada pelo nome com que está salva (`null` = regra nova):
 * o índice muda quando a lista é reordenada, o nome salvo não.
 */
interface Editing {
  saved: string | null
  draft: RuleDraft
}

interface Model {
  state: RulesState | null
  editing: Editing | null
}

const same = (a: RuleDraft, b: RuleDraft) => JSON.stringify(a) === JSON.stringify(b)
const find = (s: RulesState | null, name: string | null) =>
  name === null ? undefined : s?.drafts.find((d) => d.name === name)

/** O que fica aberto depois que o disco mudou para `s`. */
function next(m: Model, s: RulesState, open?: string): Editing | null {
  const pick = (d: RuleDraft | undefined) => (d ? { saved: d.name, draft: d } : null)
  if (open !== undefined) return pick(find(s, open))
  const cur = m.editing
  if (cur) {
    if (cur.saved === null) return cur // regra nova ainda não salva
    const before = find(m.state, cur.saved)
    const after = find(s, cur.saved)
    if (before && after && !same(cur.draft, before)) {
      // Alterações não salvas: ficam, mas o interruptor segue o disco.
      return { ...cur, draft: { ...cur.draft, enabled: after.enabled } }
    }
    if (after) return pick(after)
  }
  return pick(s.drafts[0])
}

export function Rules({ version }: { version: number }) {
  const toast = useToast()
  const [{ state, editing }, setModel] = useState<Model>({ state: null, editing: null })
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [confirmDelete, setConfirmDelete] = useState(false)
  const [pendingOpen, setPendingOpen] = useState<string | null>(null)

  const apply = useCallback(
    (s: RulesState, open?: string) => setModel((m) => ({ state: s, editing: next(m, s, open) })),
    [],
  )
  const setEditing = (e: Editing | null) => setModel((m) => ({ ...m, editing: e }))

  useEffect(() => {
    api.rules().then(
      (s) => apply(s),
      (e) => toast(errorMessage(e), 'error'),
    )
  }, [apply, toast, version])

  const saved = find(state, editing?.saved ?? null)
  const index = saved && state ? state.drafts.indexOf(saved) : null
  const dirty = editing ? (saved ? !same(editing.draft, saved) : true) : false

  /** Roda uma edição; em conflito, recarrega o disco e mantém o rascunho. */
  const run = async (op: (mtime: string | null) => Promise<RulesState>, open?: string) => {
    setBusy(true)
    setError(null)
    try {
      apply(await op(state?.mtime ?? null), open)
      return true
    } catch (e) {
      if (isCmdError(e) && e.kind === 'conflict') {
        toast('O config.toml mudou no disco. Recarreguei; salve de novo.', 'error')
        const fresh = await api.rules()
        setModel((m) => ({ ...m, state: fresh }))
      } else if (isCmdError(e) && e.kind === 'invalid') {
        setError(e.message)
      } else {
        toast(errorMessage(e), 'error')
      }
      return false
    } finally {
      setBusy(false)
    }
  }

  const save = async () => {
    if (!editing) return
    const name = editing.draft.name.trim()
    if (await run((m) => api.saveRule(m, index, editing.draft), name)) {
      toast(`Regra "${name}" salva. O daemon recarrega sozinho.`)
    }
  }

  const open = (name: string) => {
    setError(null)
    setEditing({ saved: name, draft: find(state, name)! })
  }

  if (state?.error) {
    return (
      <div className="flex flex-col gap-6">
        <ScreenHeader
          title="Regras"
          subtitle={`O ${state.config_path} tem um erro e precisa ser corrigido à mão.`}
          actions={<Button onClick={() => api.openConfig()}>Abrir config.toml</Button>}
        />
        <Card className="p-5">
          <p role="alert" className="font-mono text-[13px] text-danger">
            {errorMessage(state.error)}
          </p>
          <p className="mt-3 text-xs text-ink-3">
            Enquanto o arquivo estiver inválido, o daemon continua usando a última versão válida.
          </p>
        </Card>
      </div>
    )
  }

  return (
    <div className="flex flex-col gap-6">
      <ScreenHeader
        title="Regras"
        subtitle="A primeira regra que casar vence. Arraste para mudar a ordem."
        actions={
          <>
            <Button onClick={() => api.openConfig().catch((e) => toast(errorMessage(e), 'error'))}>
              Abrir config.toml
            </Button>
            <Button
              variant="primary"
              onClick={() => {
                setError(null)
                setEditing({ saved: null, draft: { ...NEW_RULE, name: freeName(state?.drafts ?? []) } })
              }}
            >
              <Plus size={14} /> Nova regra
            </Button>
          </>
        }
      />

      <div className="grid grid-cols-[340px_1fr] items-start gap-4">
        <div className="min-w-0">
          {state && state.drafts.length === 0 && !editing && (
            <Card>
              <Empty>Nenhuma regra ainda. Crie a primeira com "Nova regra".</Empty>
            </Card>
          )}
          {state && (
            <RuleList
              drafts={state.drafts}
              selected={index}
              onSelect={(i) => {
                const name = state.drafts[i].name
                if (dirty && name !== editing?.saved) setPendingOpen(name)
                else open(name)
              }}
              onToggle={(i, enabled) => run((m) => api.setEnabled(m, i, enabled))}
              onMove={(from, to) => run((m) => api.moveRule(m, from, to))}
            />
          )}
        </div>
        {editing && (
          <RuleEditor
            draft={editing.draft}
            savedName={saved?.name ?? null}
            isNew={editing.saved === null}
            dirty={dirty}
            error={error}
            busy={busy}
            onChange={(draft) => setEditing({ ...editing, draft })}
            onSave={save}
            onDiscard={() => {
              setError(null)
              if (saved) setEditing({ ...editing, draft: saved })
              else setEditing(state?.drafts[0] ? { saved: state.drafts[0].name, draft: state.drafts[0] } : null)
            }}
            onDelete={() => setConfirmDelete(true)}
          />
        )}
      </div>

      {pendingOpen && (
        <Dialog
          title="Descartar alterações?"
          onClose={() => setPendingOpen(null)}
          footer={
            <>
              <Button onClick={() => setPendingOpen(null)}>Continuar editando</Button>
              <Button
                variant="primary"
                onClick={() => {
                  open(pendingOpen)
                  setPendingOpen(null)
                }}
              >
                Descartar
              </Button>
            </>
          }
        >
          <p className="text-[13px]">A regra aberta tem alterações que ainda não foram salvas.</p>
        </Dialog>
      )}

      {confirmDelete && index !== null && (
        <Dialog
          title="Excluir regra"
          onClose={() => setConfirmDelete(false)}
          footer={
            <>
              <Button onClick={() => setConfirmDelete(false)}>Cancelar</Button>
              <Button
                variant="primary"
                className="!border-danger !bg-danger"
                onClick={async () => {
                  setConfirmDelete(false)
                  await run((m) => api.deleteRule(m, index))
                }}
              >
                Excluir
              </Button>
            </>
          }
        >
          <p className="text-[13px]">
            Excluir a regra <strong>{saved?.name}</strong> do config.toml? O histórico das ações dela
            continua guardado.
          </p>
        </Dialog>
      )}
    </div>
  )
}

function freeName(drafts: RuleDraft[]): string {
  const names = new Set(drafts.map((d) => d.name))
  if (!names.has('Nova regra')) return 'Nova regra'
  let n = 2
  while (names.has(`Nova regra ${n}`)) n++
  return `Nova regra ${n}`
}
