// Formatação para a interface. Só apresentação: nenhuma regra de negócio aqui.
import type { Category, ConditionKey, DraftAction, Entry, Kind, LibraryItem, RuleDraft } from './api'

const MONTHS = ['jan', 'fev', 'mar', 'abr', 'mai', 'jun', 'jul', 'ago', 'set', 'out', 'nov', 'dez']

const pad = (n: number) => String(n).padStart(2, '0')
const hm = (d: Date) => `${pad(d.getHours())}:${pad(d.getMinutes())}`

function dayDiff(d: Date, now: Date): number {
  const a = new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime()
  const b = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime()
  return Math.round((b - a) / 86_400_000)
}

/** "agora", "há 4 min", "há 1 h", "ontem", "12 nov 22:15" (atividade recente). */
export function relativeTime(ts: number, now = new Date()): string {
  const d = new Date(ts * 1000)
  const secs = Math.max(0, Math.floor((now.getTime() - d.getTime()) / 1000))
  const days = dayDiff(d, now)
  if (secs < 60) return 'agora'
  if (secs < 3600) return `há ${Math.floor(secs / 60)} min`
  if (days === 0) return `há ${Math.floor(secs / 3600)} h`
  if (days === 1) return 'ontem'
  return `${d.getDate()} ${MONTHS[d.getMonth()]} ${hm(d)}`
}

/** "hoje 14:32", "ontem 18:47", "12 nov 22:15" (coluna Quando do histórico). */
export function whenLabel(ts: number, now = new Date()): string {
  const d = new Date(ts * 1000)
  const days = dayDiff(d, now)
  if (days === 0) return `hoje ${hm(d)}`
  if (days === 1) return `ontem ${hm(d)}`
  const year = d.getFullYear() === now.getFullYear() ? '' : ` ${d.getFullYear()}`
  return `${d.getDate()} ${MONTHS[d.getMonth()]}${year} ${hm(d)}`
}

/** Igual a `units::format_size` do core: base 1000, vírgula decimal. */
export function size(n: number): string {
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  let v = n
  let i = 0
  while (v >= 1000 && i < units.length - 1) {
    v /= 1000
    i++
  }
  if (i === 0) return `${n} B`
  const s = v.toFixed(1).replace(/\.0$/, '').replace('.', ',')
  return `${s} ${units[i]}`
}

export const KIND_LABEL: Record<Kind, string> = {
  move: 'mover',
  copy: 'copiar',
  rename: 'renomear',
  trash: 'lixeira',
}

export const CONDITION_LABEL: Record<ConditionKey, string> = {
  ext: 'Extensão',
  name: 'Nome (glob)',
  name_regex: 'Nome (regex)',
  mime: 'Tipo (MIME)',
  origin: 'Origem do download',
  site: 'Site (página de origem)',
  size_gt: 'Maior que',
  size_lt: 'Menor que',
  older_than: 'Mais antigo que',
}

export const CONDITION_KEYS = Object.keys(CONDITION_LABEL) as ConditionKey[]

export const CONDITION_PLACEHOLDER: Record<ConditionKey, string> = {
  ext: 'pdf, xml',
  name: '*fatura*',
  name_regex: '(?i)nota|nf-?e',
  mime: 'image/*',
  origin: '*.nubank.com.br',
  site: 'github.com',
  size_gt: '100MB',
  size_lt: '1MB',
  older_than: '30d',
}

/** "100MB" → "100 MB". */
function prettySize(raw: string): string {
  return raw.trim().replace(/^([\d.]+)\s*([A-Za-z]+)$/, '$1 $2')
}

/** "30d" → "30 dias", "12h" → "12 h". */
export function prettyDuration(raw: string): string {
  const m = raw.trim().match(/^(\d+)\s*([smhdw])$/)
  if (!m) return raw
  const n = Number(m[1])
  switch (m[2]) {
    case 'd':
      return n === 1 ? '1 dia' : `${n} dias`
    case 'w':
      return n === 1 ? '1 semana' : `${n} semanas`
    case 'h':
      return `${n} h`
    case 'm':
      return `${n} min`
    default:
      return `${n} s`
  }
}

function conditionSummary(key: ConditionKey, value: string): string {
  switch (key) {
    case 'ext':
      return value
        .split(',')
        .map((s) => s.trim())
        .filter(Boolean)
        .join(', ')
    case 'name':
      return `nome ${value}`
    case 'name_regex':
      return `nome ~ /${value}/`
    case 'mime':
      return value
    case 'origin':
      return `origem ${value}`
    case 'site':
      return `site ${value}`
    case 'size_gt':
      return `> ${prettySize(value)}`
    case 'size_lt':
      return `< ${prettySize(value)}`
    case 'older_than':
      return `> ${prettyDuration(value)}`
  }
}

export function actionLabel(a: DraftAction): string {
  return KIND_LABEL[a.kind]
}

/** "pdf, xml · nome ~ /nota/ → mover", com "· pausada" no fim quando pausada. */
export function ruleSummary(d: RuleDraft): string {
  const conds = d.when.map((c) => conditionSummary(c.key, c.value)).join(' · ')
  const s = `${conds} → ${actionLabel(d.action)}`
  return d.enabled ? s : `${s} · pausada`
}

/** Pasta de um caminho já abreviado: "~/Docs/a.pdf" → "~/Docs/". */
export function dirOf(p: string): string {
  const i = p.lastIndexOf('/')
  return i <= 0 ? p : p.slice(0, i + 1)
}

export function baseOf(p: string): string {
  return p.slice(p.lastIndexOf('/') + 1)
}

/** Linha de baixo da atividade recente: o que aconteceu com o arquivo. */
export function describeEntry(e: Entry): string {
  if (e.error) return `Erro: ${e.error}`
  if (e.undone) return `Desfeito · voltou para ${dirOf(e.src_display)} e não será mais organizado`
  const dst = e.dst_display ?? ''
  switch (e.kind) {
    case 'move':
      return `Movido para ${dirOf(dst)}`
    case 'copy':
      return `Copiado para ${dirOf(dst)}`
    case 'rename':
      return `Renomeado para ${baseOf(dst)}`
    case 'trash':
      return e.size != null ? `Enviado à lixeira · ${size(e.size)}` : 'Enviado à lixeira'
  }
}

const folder = (p: string) => dirOf(p).replace(/\/$/, '') || '/'

/** Nome mostrado no histórico: o novo nome quando foi renomeado. */
export function historyName(e: Entry): string {
  return e.kind === 'rename' && !e.error && !e.undone && e.dst_display ? baseOf(e.dst_display) : e.name
}

/** Linha de baixo do histórico: origem → destino. */
export function historyDetail(e: Entry): string {
  if (e.error) return e.error
  if (e.undone) return `de volta em ${folder(e.src_display)} · marcado para manter`
  switch (e.kind) {
    case 'rename':
      return `antes: ${e.name}`
    case 'trash':
      return `${folder(e.src_display)} → Lixeira${e.size != null ? ` · ${size(e.size)}` : ''}`
    default:
      return `${folder(e.src_display)} → ${folder(e.dst_display ?? '')}`
  }
}

export const CATEGORY_LABEL: Record<Category, string> = {
  document: 'Documento',
  image: 'Imagem',
  video: 'Vídeo',
  audio: 'Áudio',
  archive: 'Compactado',
  installer: 'Instalador',
  other: 'Outro',
}

/** Coluna "Onde está" da biblioteca. */
export function libraryWhere(i: LibraryItem): string {
  switch (i.state) {
    case 'in_progress':
      return 'baixando…'
    case 'trashed':
      return 'na lixeira'
    case 'gone':
      return 'não encontrado'
    case 'interrupted':
      return 'download interrompido'
    default:
      return i.folder_display
  }
}
