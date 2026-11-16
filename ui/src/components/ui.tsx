// Peças visuais básicas, com os tokens de src/index.css.
import type { ButtonHTMLAttributes, ReactNode } from 'react'
import type { Entry, Kind } from '../api'
import { KIND_LABEL } from '../format'

type Variant = 'primary' | 'secondary' | 'danger' | 'ghost'

const VARIANT: Record<Variant, string> = {
  primary: 'bg-brand text-white border-brand hover:brightness-95',
  secondary: 'bg-card text-ink border-input hover:bg-subtle',
  danger: 'bg-card text-danger border-transparent hover:bg-danger-soft',
  ghost: 'bg-transparent text-brand border-transparent hover:bg-brand-soft',
}

export function Button({
  variant = 'secondary',
  size = 'md',
  className = '',
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant; size?: 'sm' | 'md' }) {
  const sz = size === 'sm' ? 'h-7 px-2.5 text-xs gap-1' : 'h-9 px-3.5 text-[13px] gap-1.5'
  return (
    <button
      type="button"
      className={`inline-flex shrink-0 items-center justify-center rounded-lg border font-medium shadow-xs transition disabled:cursor-not-allowed disabled:opacity-50 ${sz} ${VARIANT[variant]} ${className}`}
      {...props}
    />
  )
}

export function Card({ className = '', children }: { className?: string; children: ReactNode }) {
  return (
    <div className={`rounded-xl border border-line bg-card shadow-xs ${className}`}>{children}</div>
  )
}

export function ScreenHeader({
  title,
  subtitle,
  actions,
}: {
  title: string
  subtitle: ReactNode
  actions?: ReactNode
}) {
  return (
    <header className="flex items-start justify-between gap-4">
      <div className="min-w-0">
        <h1 className="text-2xl font-semibold tracking-tight text-ink">{title}</h1>
        <p className="mt-1 text-[13px] text-ink-2">{subtitle}</p>
      </div>
      {actions && <div className="flex gap-2.5">{actions}</div>}
    </header>
  )
}

const KIND_STYLE: Record<Kind, string> = {
  move: 'bg-brand-soft text-brand',
  copy: 'bg-info-soft text-info',
  rename: 'bg-purple-soft text-purple',
  trash: 'bg-danger-soft text-danger',
}

/** Selo da coluna Ação: tipo, ou "erro"/"desfeito". */
export function KindBadge({ entry }: { entry: Pick<Entry, 'kind' | 'undone' | 'error'> }) {
  const [label, style] = entry.error
    ? ['erro', 'bg-danger text-white']
    : entry.undone
      ? ['desfeito', 'bg-line-soft text-ink-2']
      : [KIND_LABEL[entry.kind], KIND_STYLE[entry.kind]]
  return (
    <span className={`inline-block rounded-full px-2 py-0.5 text-[11px] font-medium ${style}`}>
      {label}
    </span>
  )
}

export function Chip({
  active,
  tone = 'default',
  onClick,
  children,
}: {
  active: boolean
  tone?: 'default' | 'danger'
  onClick: () => void
  children: ReactNode
}) {
  const base = 'h-7 rounded-full border px-3 text-xs font-medium transition'
  const style = active
    ? 'border-ink bg-ink text-white'
    : tone === 'danger'
      ? 'border-danger/30 bg-card text-danger hover:bg-danger-soft'
      : 'border-line bg-card text-ink-2 hover:bg-subtle'
  return (
    <button type="button" aria-pressed={active} className={`${base} ${style}`} onClick={onClick}>
      {children}
    </button>
  )
}

export function Toggle({
  checked,
  onChange,
  label,
}: {
  checked: boolean
  onChange: (v: boolean) => void
  label: string
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      onClick={(e) => {
        e.stopPropagation()
        onChange(!checked)
      }}
      className={`relative h-5 w-9 shrink-0 rounded-full transition ${checked ? 'bg-brand' : 'bg-input'}`}
    >
      <span
        className={`absolute top-0.5 size-4 rounded-full bg-white shadow transition-all ${checked ? 'left-[18px]' : 'left-0.5'}`}
      />
    </button>
  )
}

export function Empty({ children }: { children: ReactNode }) {
  return <p className="px-5 py-10 text-center text-[13px] text-ink-3">{children}</p>
}
