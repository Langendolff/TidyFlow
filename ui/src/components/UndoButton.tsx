import { RotateCcw } from 'lucide-react'
import { useState } from 'react'
import { api, errorMessage, type Entry } from '../api'
import { useToast } from './toast'
import { Button } from './ui'

/** Desfazer de uma linha; "Detalhes" no lugar quando a ação falhou. */
export function UndoButton({
  entry,
  onDone,
  onDetails,
}: {
  entry: Entry
  onDone: () => void
  onDetails?: (e: Entry) => void
}) {
  const toast = useToast()
  const [busy, setBusy] = useState(false)
  if (entry.error) {
    return onDetails ? (
      <Button size="sm" onClick={() => onDetails(entry)}>
        Detalhes
      </Button>
    ) : null
  }
  if (entry.undone) return null
  return (
    <Button
      size="sm"
      disabled={busy}
      onClick={async () => {
        setBusy(true)
        try {
          await api.undo(entry.id)
          toast(`Desfeito: ${entry.name} voltou para a pasta original`)
          onDone()
        } catch (e) {
          toast(`Não foi possível desfazer: ${errorMessage(e)}`, 'error')
        } finally {
          setBusy(false)
        }
      }}
    >
      <RotateCcw size={12} /> Desfazer
    </Button>
  )
}
