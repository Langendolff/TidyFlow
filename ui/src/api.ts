// Chamadas aos comandos Tauri (crates/app/src/commands.rs). Os tipos espelham dto.rs.
import { invoke } from '@tauri-apps/api/core'

export type Kind = 'move' | 'copy' | 'rename' | 'trash'

export interface CmdError {
  kind: 'invalid' | 'conflict' | 'other'
  message: string
  line: number | null
}

export function isCmdError(e: unknown): e is CmdError {
  return typeof e === 'object' && e !== null && 'message' in e && 'kind' in e
}

export function errorMessage(e: unknown): string {
  if (isCmdError(e)) return e.line ? `linha ${e.line}: ${e.message}` : e.message
  return String(e)
}

export interface Entry {
  id: number
  ts: number
  rule: string
  kind: Kind
  name: string
  src: string
  src_display: string
  dst: string | null
  dst_display: string | null
  size: number | null
  undone: boolean
  error: string | null
}

export interface Stats {
  organized_today: number
  organized_yesterday: number
  trashed_week: number
  freed_week: number
  errors_week: number
}

export interface Folder {
  path: string
  path_display: string
  exists: boolean
  rules: number
  files: number
  bytes: number
}

export interface Overview {
  stats: Stats
  recent: Entry[]
  folders: Folder[]
  config_error: string | null
}

export interface ScanItem {
  path: string
  path_display: string
  rule: string
  kind: Kind | null
  dst_display: string | null
  error: string | null
}

export type Filter = 'all' | 'moved' | 'trashed' | 'undone' | 'errors'

export interface Counts {
  all: number
  moved: number
  trashed: number
  undone: number
  errors: number
}

export interface HistoryPage {
  entries: Entry[]
  counts: Counts
  has_more: boolean
}

export type ConditionKey =
  | 'ext'
  | 'name'
  | 'name_regex'
  | 'mime'
  | 'origin'
  | 'site'
  | 'size_gt'
  | 'size_lt'
  | 'older_than'

export interface Condition {
  key: ConditionKey
  value: string
}

export type DraftAction =
  | { kind: 'move'; value: string }
  | { kind: 'copy'; value: string }
  | { kind: 'rename'; value: string }
  | { kind: 'trash' }

export interface RuleDraft {
  name: string
  enabled: boolean
  watch: string[]
  when: Condition[]
  action: DraftAction
}

export interface RulesState {
  drafts: RuleDraft[]
  error: CmdError | null
  config_path: string
  mtime: string | null
}

export interface Preview {
  src_display: string
  dst_display: string
}

export interface Check {
  key: ConditionKey
  expected: string
  ok: boolean
}

export interface RuleCheck {
  name: string
  enabled: boolean
  checks: Check[]
  verdict: 'wins' | 'fails' | 'untested' | 'paused'
}

export interface Facts {
  path: string
  path_display: string
  name: string
  ext: string
  mime: string
  origin_url: string | null
  origin_host: string | null
  referrer_url: string | null
  site: string | null
  size: number
  modified: number
  keep: boolean
}

export interface Inspect {
  facts: Facts
  rules: RuleCheck[]
  result: { rule: string; kind: Kind; dst: string | null; dst_display: string | null } | null
}

export interface DaemonStatus {
  active: boolean
  since: number | null
  folders: number
}

export interface Live {
  max_id: number
  config_mtime: string | null
}

// Os argumentos vão em camelCase; o Tauri converte para os nomes em snake_case do Rust.
export const api = {
  overview: () => invoke<Overview>('overview'),
  scan: (dryRun: boolean) => invoke<ScanItem[]>('scan', { dryRun }),
  history: (filter: Filter, search: string, beforeId: number | null) =>
    invoke<HistoryPage>('history', { filter, search, beforeId }),
  undo: (id: number | null) => invoke<Entry>('undo', { id }),
  rules: () => invoke<RulesState>('rules_list'),
  saveRule: (mtime: string | null, index: number | null, draft: RuleDraft) =>
    invoke<RulesState>('save_rule', { mtime, index, draft }),
  deleteRule: (mtime: string | null, index: number) =>
    invoke<RulesState>('delete_rule', { mtime, index }),
  moveRule: (mtime: string | null, from: number, to: number) =>
    invoke<RulesState>('move_rule', { mtime, from, to }),
  setEnabled: (mtime: string | null, index: number, enabled: boolean) =>
    invoke<RulesState>('set_enabled', { mtime, index, enabled }),
  preview: (journalName: string, draft: RuleDraft) =>
    invoke<Preview | null>('preview', { journalName, draft }),
  openConfig: () => invoke<void>('open_config'),
  inspect: (path: string) => invoke<Inspect>('inspect', { path }),
  organize: (path: string) => invoke<Entry>('organize', { path }),
  initialTestFile: () => invoke<string | null>('initial_test_file'),
  daemonStatus: () => invoke<DaemonStatus>('daemon_status'),
  startDaemon: () => invoke<DaemonStatus>('start_daemon'),
  live: () => invoke<Live>('live'),
}
