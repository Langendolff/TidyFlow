import { useEffect, useState } from 'react'
import { api } from '../api'

const INTERVAL = 2000

/**
 * Número que muda quando o journal ganha ações novas ou o config.toml muda no disco.
 * As telas usam como dependência do carregamento para se atualizarem sozinhas.
 */
export function useLive(): number {
  const [version, setVersion] = useState(0)
  useEffect(() => {
    let last = ''
    let alive = true
    const tick = async () => {
      try {
        const l = await api.live()
        const key = `${l.max_id}|${l.config_mtime ?? ''}`
        if (alive && last && key !== last) setVersion((v) => v + 1)
        last = key
      } catch {
        // journal travado por um instante: tenta de novo no próximo tick
      }
    }
    void tick()
    const t = setInterval(tick, INTERVAL)
    return () => {
      alive = false
      clearInterval(t)
    }
  }, [])
  return version
}
