// Conversão dos eventos de download do navegador nas mensagens do host
// (`tidyflow native-host`). Sem acesso à API do navegador, para ser testável.

export type BrowserName = 'firefox' | 'chromium'

/** O que usamos de `downloads.DownloadItem` (Firefox e Chromium). */
export interface DownloadLike {
  id: number
  filename: string
  url: string
  finalUrl?: string
  referrer?: string
  mime?: string
  incognito: boolean
  startTime: string
  fileSize?: number
  totalBytes?: number
  error?: string
}

/** O que usamos de `downloads.onChanged`. */
export interface DeltaLike {
  id: number
  state?: { current?: string }
  filename?: { current?: string }
  error?: { current?: string }
}

export type HostMessage =
  | {
      type: 'started'
      browser: BrowserName
      id: number
      path: string
      url: string
      final_url: string | null
      referrer: string | null
      page_title: string | null
      mime: string | null
      started_at: number
    }
  | { type: 'finished'; browser: BrowserName; id: number; path: string; size: number }
  | { type: 'interrupted'; browser: BrowserName; id: number; error: string | null }
  | { type: 'renamed'; browser: BrowserName; id: number; path: string }

/** `started` para um download, ou `null` se ele não deve ser informado. */
export function onCreated(d: DownloadLike, browser: BrowserName, pageTitle: string | null): HostMessage | null {
  // Janela privada nunca chega ao host. Sem nome ainda (Chromium antes de decidir), espera.
  if (d.incognito || !d.filename) return null
  const started = Date.parse(d.startTime)
  return {
    type: 'started',
    browser,
    id: d.id,
    path: d.filename,
    url: d.url,
    final_url: d.finalUrl && d.finalUrl !== d.url ? d.finalUrl : null,
    referrer: d.referrer || null,
    page_title: pageTitle || null,
    mime: d.mime || null,
    started_at: Math.floor((Number.isNaN(started) ? Date.now() : started) / 1000),
  }
}

/**
 * Mensagens para uma mudança. `started`: o host já recebeu o início deste download.
 * Sem início registrado (o Chromium só tem o nome depois, ou a extensão foi instalada
 * no meio do download), o `started` vem primeiro.
 */
export function onChanged(
  delta: DeltaLike,
  item: DownloadLike | undefined,
  browser: BrowserName,
  started: boolean,
  pageTitle: string | null,
): HostMessage[] {
  if (!item || item.incognito) return []
  const out: HostMessage[] = []
  const name = delta.filename?.current
  const current = name ? { ...item, filename: name } : item
  if (!started) {
    const first = onCreated(current, browser, pageTitle)
    if (!first) return []
    out.push(first)
  } else if (name) {
    out.push({ type: 'renamed', browser, id: item.id, path: name })
  }
  switch (delta.state?.current) {
    case 'complete':
      out.push({
        type: 'finished',
        browser,
        id: item.id,
        path: current.filename,
        size: current.fileSize ?? current.totalBytes ?? 0,
      })
      break
    case 'interrupted':
      out.push({ type: 'interrupted', browser, id: item.id, error: delta.error?.current ?? item.error ?? null })
      break
  }
  return out
}

const withoutHash = (u: string) => u.split('#')[0]

/** Título da aba aberta na página de onde o download saiu. */
export function titleFor(referrer: string | undefined, tabs: { url?: string; title?: string }[]): string | null {
  if (!referrer) return null
  const want = withoutHash(referrer)
  const tab = tabs.find((t) => t.url && withoutHash(t.url) === want)
  return tab?.title || null
}
