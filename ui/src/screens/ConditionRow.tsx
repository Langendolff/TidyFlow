import { ChevronDown, X } from 'lucide-react'
import type { Condition, ConditionKey } from '../api'
import { CONDITION_KEYS, CONDITION_LABEL, CONDITION_PLACEHOLDER } from '../format'

const CONNECTOR: Record<ConditionKey, string> = {
  ext: 'é',
  name: 'casa com',
  name_regex: 'casa com',
  mime: 'casa com',
  origin: 'casa com',
  site: 'casa com',
  size_gt: '',
  size_lt: '',
  older_than: '',
}

export const inputClass =
  'h-9 w-full rounded-lg border border-input bg-card px-3 text-[13px] outline-none focus:border-brand'

/** Tipo + valor de uma condição. `used`: tipos já usados em outras linhas. */
export function ConditionRow({
  condition,
  used,
  onChange,
  onRemove,
}: {
  condition: Condition
  used: ConditionKey[]
  onChange: (c: Condition) => void
  onRemove: () => void
}) {
  const options = CONDITION_KEYS.filter((k) => k === condition.key || !used.includes(k))
  return (
    <div className="flex items-center gap-2 rounded-lg border border-line bg-subtle p-1.5">
      <div className="relative w-44 shrink-0">
        <select
          aria-label="Tipo de condição"
          value={condition.key}
          onChange={(e) => onChange({ key: e.target.value as ConditionKey, value: '' })}
          className="h-8 w-full appearance-none rounded-md border border-input bg-card pr-7 pl-2.5 text-[13px] font-medium outline-none focus:border-brand"
        >
          {options.map((k) => (
            <option key={k} value={k}>
              {CONDITION_LABEL[k]}
            </option>
          ))}
        </select>
        <ChevronDown
          size={14}
          className="pointer-events-none absolute top-1/2 right-2 -translate-y-1/2 text-ink-3"
        />
      </div>
      {CONNECTOR[condition.key] && (
        <span className="shrink-0 text-xs text-ink-3">{CONNECTOR[condition.key]}</span>
      )}
      <input
        aria-label={`Valor de ${CONDITION_LABEL[condition.key]}`}
        value={condition.value}
        placeholder={CONDITION_PLACEHOLDER[condition.key]}
        onChange={(e) => onChange({ ...condition, value: e.target.value })}
        className="h-8 min-w-0 flex-1 rounded-md border border-input bg-card px-2.5 text-[13px] outline-none focus:border-brand"
      />
      <button
        type="button"
        aria-label="Remover condição"
        onClick={onRemove}
        className="flex size-7 shrink-0 items-center justify-center rounded-md text-ink-3 hover:bg-card hover:text-ink"
      >
        <X size={14} />
      </button>
    </div>
  )
}
