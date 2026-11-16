import {
  closestCenter,
  DndContext,
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
} from '@dnd-kit/core'
import {
  SortableContext,
  sortableKeyboardCoordinates,
  useSortable,
  verticalListSortingStrategy,
} from '@dnd-kit/sortable'
import { CSS } from '@dnd-kit/utilities'
import { GripVertical } from 'lucide-react'
import type { RuleDraft } from '../api'
import { Toggle } from '../components/ui'
import { ruleSummary } from '../format'

export function RuleList({
  drafts,
  selected,
  onSelect,
  onToggle,
  onMove,
}: {
  drafts: RuleDraft[]
  selected: number | null
  onSelect: (i: number) => void
  onToggle: (i: number, enabled: boolean) => void
  onMove: (from: number, to: number) => void
}) {
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 4 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  )
  // Nomes são únicos (validação do core), então servem de id estável.
  const ids = drafts.map((d) => d.name)
  const onDragEnd = ({ active, over }: DragEndEvent) => {
    if (!over || active.id === over.id) return
    onMove(ids.indexOf(String(active.id)), ids.indexOf(String(over.id)))
  }
  return (
    <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={onDragEnd}>
      <SortableContext items={ids} strategy={verticalListSortingStrategy}>
        <ol className="flex flex-col gap-2">
          {drafts.map((d, i) => (
            <Item
              key={d.name}
              draft={d}
              index={i}
              active={selected === i}
              onSelect={() => onSelect(i)}
              onToggle={(v) => onToggle(i, v)}
            />
          ))}
        </ol>
      </SortableContext>
    </DndContext>
  )
}

function Item({
  draft,
  index,
  active,
  onSelect,
  onToggle,
}: {
  draft: RuleDraft
  index: number
  active: boolean
  onSelect: () => void
  onToggle: (v: boolean) => void
}) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: draft.name,
  })
  return (
    <li
      ref={setNodeRef}
      style={{ transform: CSS.Transform.toString(transform), transition }}
      className={`flex items-center gap-2.5 rounded-xl border bg-card px-2.5 py-3 shadow-xs ${
        active ? 'border-brand ring-1 ring-brand' : 'border-line'
      } ${isDragging ? 'z-10 shadow-lg' : ''}`}
    >
      <button
        type="button"
        aria-label={`Arrastar ${draft.name}`}
        className="cursor-grab text-ink-3 active:cursor-grabbing"
        {...attributes}
        {...listeners}
      >
        <GripVertical size={14} />
      </button>
      <span
        className={`flex size-6 shrink-0 items-center justify-center rounded-md text-xs font-semibold ${
          active ? 'bg-brand text-white' : 'bg-line-soft text-ink-2'
        }`}
      >
        {index + 1}
      </span>
      <button
        type="button"
        onClick={onSelect}
        className={`min-w-0 flex-1 text-left ${draft.enabled ? '' : 'opacity-50'}`}
      >
        <p className="truncate text-[13px] font-semibold">{draft.name}</p>
        <p className="truncate text-[11px] text-ink-3">{ruleSummary(draft)}</p>
      </button>
      <Toggle
        checked={draft.enabled}
        onChange={onToggle}
        label={draft.enabled ? `Pausar ${draft.name}` : `Ativar ${draft.name}`}
      />
    </li>
  )
}
