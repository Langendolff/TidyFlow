import { Circle, FileSearch, ListFilter, RotateCcw } from 'lucide-react'
import { useEffect, useState } from 'react'
import { api, errorMessage, type DaemonStatus } from '../api'
import { useToast } from './toast'
import icon from '../assets/icon.svg'

export type Screen = 'overview' | 'rules' | 'history' | 'test'

const NAV: { id: Screen; label: string; icon: typeof Circle }[] = [
  { id: 'overview', label: 'Visão geral', icon: Circle },
  { id: 'rules', label: 'Regras', icon: ListFilter },
  { id: 'history', label: 'Histórico', icon: RotateCcw },
  { id: 'test', label: 'Testar arquivo', icon: FileSearch },
]

function since(ts: number): string {
  const secs = Date.now() / 1000 - ts
  if (secs < 3600) return `há ${Math.max(1, Math.floor(secs / 60))} min`
  if (secs < 86_400) return `há ${Math.floor(secs / 3600)} h`
  const d = Math.floor(secs / 86_400)
  return d === 1 ? 'há 1 dia' : `há ${d} dias`
}

export function Sidebar({ screen, onNavigate }: { screen: Screen; onNavigate: (s: Screen) => void }) {
  const toast = useToast()
  const [status, setStatus] = useState<DaemonStatus | null>(null)
  useEffect(() => {
    const load = () => api.daemonStatus().then(setStatus, () => setStatus(null))
    load()
    const t = setInterval(load, 10_000)
    return () => clearInterval(t)
  }, [])
  const folders = status ? `${status.folders} ${status.folders === 1 ? 'pasta' : 'pastas'}` : ''

  return (
    <aside className="flex w-58 shrink-0 flex-col gap-1 bg-side px-3.5 py-5">
      <div className="flex items-center gap-2.5 px-2 pt-1 pb-5">
        <img src={icon} alt="" className="size-7.5" />
        <span className="text-[15px] font-semibold text-subtle">TidyFlow</span>
      </div>
      <nav className="flex flex-col gap-1">
        {NAV.map(({ id, label, icon: Icon }) => (
          <button
            key={id}
            type="button"
            aria-current={screen === id ? 'page' : undefined}
            onClick={() => onNavigate(id)}
            className={`flex items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-[13px] font-medium transition ${
              screen === id ? 'bg-side-active text-subtle' : 'text-side-text hover:bg-side-active/60'
            }`}
          >
            <Icon size={14} className={screen === id ? 'text-brand-light' : 'text-side-muted'} />
            {label}
          </button>
        ))}
      </nav>
      <div className="flex-1" />
      <div className="rounded-lg bg-side-active p-3">
        <div className="flex items-center gap-2 text-xs font-semibold text-subtle">
          <span className={`size-2 rounded-full ${status?.active ? 'bg-brand-light' : 'bg-danger'}`} />
          {status?.active ? 'Daemon ativo' : 'Daemon parado'}
        </div>
        <p className="mt-1.5 text-[11px] text-side-muted">
          {status?.active
            ? `systemd · ${status.since ? since(status.since) : 'agora'} · ${folders}`
            : 'As pastas não estão sendo monitoradas.'}
        </p>
        {status && !status.active && (
          <button
            type="button"
            className="mt-2 text-[11px] font-semibold text-brand-light hover:underline"
            onClick={() =>
              api.startDaemon().then(setStatus, (e) =>
                toast(`Não foi possível iniciar: ${errorMessage(e)}`, 'error'),
              )
            }
          >
            Iniciar daemon
          </button>
        )}
      </div>
    </aside>
  )
}
