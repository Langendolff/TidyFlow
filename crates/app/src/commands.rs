//! Comandos chamados pelo front. São finos: toda regra de negócio está no core.
//! Cada comando abre o journal e lê o config na hora, para nunca mostrar estado velho
//! quando o daemon ou o usuário mudam algo por fora.

use crate::{
    dto::{
        CmdError, CmdResult, EntryDto, FactsDto, FolderDto, Outcome, RuleCheck, mtime_token, tilde,
    },
    system::{self, DaemonStatus},
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::State;
use tidyflow_core::{
    actions::{self, kind_of},
    config::{
        self, Config,
        edit::{EditError, Editor, RuleDraft},
    },
    engine::{self, Report},
    facts::Facts,
    journal::{Counts, Filter, Journal, Periods, Stats},
    rules,
};

pub struct AppState {
    pub config_path: PathBuf,
    pub journal_path: PathBuf,
    /// `tidyflow-app --test <arquivo>` na primeira execução
    pub initial_test_file: Option<String>,
}

impl AppState {
    fn journal(&self) -> CmdResult<Journal> {
        Ok(Journal::open(&self.journal_path)?)
    }

    fn config(&self) -> CmdResult<Config> {
        if !self.config_path.exists() {
            return Ok(config::parse("")?);
        }
        Ok(config::load(&self.config_path)?)
    }
}

fn watch_dirs(cfg: &Config) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = cfg.rules.iter().flat_map(|r| r.watch.clone()).collect();
    dirs.sort();
    dirs.dedup();
    dirs
}

// ---------- Visão geral ----------

#[derive(Serialize)]
pub struct Overview {
    stats: Stats,
    recent: Vec<EntryDto>,
    folders: Vec<FolderDto>,
    config_error: Option<String>,
}

#[tauri::command(async)]
pub fn overview(state: State<AppState>) -> CmdResult<Overview> {
    let journal = state.journal()?;
    let (cfg, config_error) = match state.config() {
        Ok(c) => (Some(c), None),
        Err(e) => (None, Some(e.message)),
    };
    let dirs = cfg.as_ref().map(watch_dirs).unwrap_or_default();
    let folders = dirs
        .iter()
        .map(|d| {
            let (files, bytes) = system::folder_contents(d);
            FolderDto {
                path: d.display().to_string(),
                path_display: tilde(d),
                exists: d.is_dir(),
                rules: cfg.as_ref().map_or(0, |c| {
                    c.rules
                        .iter()
                        .filter(|r| r.enabled && r.watch.contains(d))
                        .count()
                }),
                files,
                bytes,
            }
        })
        .collect();
    Ok(Overview {
        stats: journal.stats(&Periods::local(SystemTime::now()), &dirs)?,
        recent: journal
            .sweep_gone()
            .and_then(|_| journal.recent(6))?
            .iter()
            .map(EntryDto::from)
            .collect(),
        folders,
        config_error,
    })
}

#[derive(Serialize)]
pub struct ScanItem {
    path: String,
    path_display: String,
    rule: String,
    kind: Option<&'static str>,
    dst_display: Option<String>,
    error: Option<String>,
}

/// Varredura (ou simulação). Só devolve os arquivos em que alguma regra casou ou que deram erro.
#[tauri::command(async)]
pub fn scan(state: State<AppState>, dry_run: bool) -> CmdResult<Vec<ScanItem>> {
    let cfg = state.config()?;
    let journal = state.journal()?;
    let items = engine::scan(&cfg, &journal, dry_run, SystemTime::now())
        .into_iter()
        .filter_map(|(path, report)| {
            let item = |rule: String, kind, dst: Option<&Path>, error| ScanItem {
                path: path.display().to_string(),
                path_display: tilde(&path),
                rule,
                kind,
                dst_display: dst.map(tilde),
                error,
            };
            match report {
                Ok(Report::NoMatch) => None,
                Ok(Report::Planned { rule, kind, dst }) => {
                    Some(item(rule, Some(kind.as_str()), dst.as_deref(), None))
                }
                Ok(Report::Done { rule, done, .. }) => Some(item(
                    rule,
                    Some(done.kind.as_str()),
                    done.dst.as_deref(),
                    None,
                )),
                Ok(Report::Failed { rule, error }) => Some(item(rule, None, None, Some(error))),
                Err(e) => Some(item(String::new(), None, None, Some(e.to_string()))),
            }
        })
        .collect();
    Ok(items)
}

// ---------- Histórico ----------

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum FilterArg {
    All,
    Moved,
    Trashed,
    Undone,
    Errors,
}

impl From<FilterArg> for Filter {
    fn from(f: FilterArg) -> Self {
        match f {
            FilterArg::All => Filter::All,
            FilterArg::Moved => Filter::Moved,
            FilterArg::Trashed => Filter::Trashed,
            FilterArg::Undone => Filter::Undone,
            FilterArg::Errors => Filter::Errors,
        }
    }
}

#[derive(Serialize)]
pub struct HistoryPage {
    entries: Vec<EntryDto>,
    counts: Counts,
    has_more: bool,
}

const PAGE: usize = 50;

#[tauri::command(async)]
pub fn history(
    state: State<AppState>,
    filter: FilterArg,
    search: String,
    before_id: Option<i64>,
) -> CmdResult<HistoryPage> {
    let journal = state.journal()?;
    journal.sweep_gone()?;
    let mut entries = journal.page(filter.into(), &search, before_id, PAGE + 1)?;
    let has_more = entries.len() > PAGE;
    entries.truncate(PAGE);
    Ok(HistoryPage {
        entries: entries.iter().map(EntryDto::from).collect(),
        counts: journal.counts(&search)?,
        has_more,
    })
}

#[tauri::command(async)]
pub fn undo(state: State<AppState>, id: Option<i64>) -> CmdResult<EntryDto> {
    let journal = state.journal()?;
    let e = engine::undo(&journal, id)?;
    let fresh = journal.get(e.id)?.unwrap_or(e);
    Ok(EntryDto::from(&fresh))
}

// ---------- Regras ----------

#[derive(Serialize)]
pub struct RulesState {
    drafts: Vec<RuleDraft>,
    error: Option<CmdError>,
    config_path: String,
    /// devolvido nas edições, para detectar mudança no disco
    mtime: Option<String>,
}

fn rules_state(path: &Path) -> CmdResult<RulesState> {
    let base = |drafts, error, mtime| RulesState {
        drafts,
        error,
        config_path: tilde(path),
        mtime,
    };
    let ed = match Editor::open(path) {
        Ok(ed) => ed,
        Err(e @ EditError::Invalid(_)) => {
            let mtime = fs::metadata(path).and_then(|m| m.modified()).ok();
            return Ok(base(Vec::new(), Some(e.into()), mtime_token(mtime)));
        }
        Err(e) => return Err(e.into()),
    };
    let mtime = mtime_token(ed.mtime());
    if let Err(e) = config::parse(&ed.to_string()) {
        return Ok(base(Vec::new(), Some(e.into()), mtime));
    }
    Ok(base(ed.drafts()?, None, mtime))
}

/// Abre, confere que o arquivo é o mesmo que o front carregou, aplica e grava.
fn edit(
    state: &AppState,
    mtime: Option<String>,
    f: impl FnOnce(&mut Editor) -> Result<(), EditError>,
) -> CmdResult<RulesState> {
    let mut ed = Editor::open(&state.config_path)?;
    if mtime_token(ed.mtime()) != mtime {
        return Err(EditError::Conflict.into());
    }
    f(&mut ed)?;
    ed.write()?;
    rules_state(&state.config_path)
}

#[tauri::command(async)]
pub fn rules_list(state: State<AppState>) -> CmdResult<RulesState> {
    rules_state(&state.config_path)
}

#[tauri::command(async)]
pub fn save_rule(
    state: State<AppState>,
    mtime: Option<String>,
    index: Option<usize>,
    draft: RuleDraft,
) -> CmdResult<RulesState> {
    edit(&state, mtime, |ed| ed.save_rule(index, &draft).map(drop))
}

#[tauri::command(async)]
pub fn delete_rule(
    state: State<AppState>,
    mtime: Option<String>,
    index: usize,
) -> CmdResult<RulesState> {
    edit(&state, mtime, |ed| ed.delete_rule(index))
}

#[tauri::command(async)]
pub fn move_rule(
    state: State<AppState>,
    mtime: Option<String>,
    from: usize,
    to: usize,
) -> CmdResult<RulesState> {
    edit(&state, mtime, |ed| ed.move_rule(from, to))
}

#[tauri::command(async)]
pub fn set_enabled(
    state: State<AppState>,
    mtime: Option<String>,
    index: usize,
    enabled: bool,
) -> CmdResult<RulesState> {
    edit(&state, mtime, |ed| ed.set_enabled(index, enabled))
}

#[derive(Serialize)]
pub struct Preview {
    src_display: String,
    dst_display: String,
}

/// Prévia do editor: a última ação da regra (`journal_name`, o nome salvo) passada
/// pelo template do rascunho. `None` se não houver ação ou o rascunho for inválido.
#[tauri::command(async)]
pub fn preview(
    state: State<AppState>,
    journal_name: String,
    draft: RuleDraft,
) -> CmdResult<Option<Preview>> {
    let Some(entry) = state.journal()?.last_for_rule(&journal_name)? else {
        return Ok(None);
    };
    let mut ed = Editor::in_memory("").map_err(CmdError::from)?;
    let mut draft = draft;
    draft.enabled = true;
    if ed.save_rule(None, &draft).is_err() {
        return Ok(None);
    }
    let Ok(cfg) = config::parse(&ed.to_string()) else {
        return Ok(None);
    };
    let rule = &cfg.rules[0];
    // O arquivo costuma estar no destino agora; os fatos vêm dele, com o caminho de origem.
    let mut facts = entry
        .dst
        .as_deref()
        .and_then(|d| Facts::read(d).ok())
        .or_else(|| Facts::read(&entry.src).ok())
        .unwrap_or_else(|| {
            Facts::assumed(
                &entry.src,
                UNIX_EPOCH + std::time::Duration::from_secs(entry.ts.max(0) as u64),
            )
        });
    facts.path = entry.src.clone();
    let dst = actions::destination(&rule.action, &facts);
    Ok(Some(Preview {
        src_display: tilde(&entry.src),
        dst_display: dst.as_deref().map_or("lixeira".into(), tilde),
    }))
}

#[tauri::command(async)]
pub fn open_config(state: State<AppState>, app: tauri::AppHandle) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let path = &state.config_path;
    if !path.exists() {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(
            path,
            "# Regras do TidyFlow. Exemplos: config.example.toml no repositório.\n",
        )?;
    }
    app.opener()
        .open_path(path.display().to_string(), None::<&str>)
        .map_err(CmdError::other)
}

// ---------- Testar arquivo ----------

#[derive(Serialize)]
pub struct Inspect {
    facts: FactsDto,
    rules: Vec<RuleCheck>,
    result: Option<Outcome>,
}

#[tauri::command(async)]
pub fn inspect(state: State<AppState>, path: String) -> CmdResult<Inspect> {
    let path = PathBuf::from(path);
    let journal = state.journal()?;
    let facts = engine::facts(&journal, &path)
        .map_err(|e| CmdError::other(format!("{}: {e}", tilde(&path))))?;
    let keep = facts.keep || journal.was_undone_src(&path)?;
    let cfg = state.config()?;
    let now = SystemTime::now();
    let mut won = false;
    let mut result = None;
    let rules = cfg
        .rules
        .iter()
        .map(|r| {
            let checks = rules::explain(&r.when, &facts, now);
            let verdict = if !r.enabled {
                "paused"
            } else if won {
                "untested"
            } else if checks.iter().all(|c| c.ok) {
                won = true;
                let dst = actions::destination(&r.action, &facts);
                result = Some(Outcome {
                    rule: r.name.clone(),
                    kind: kind_of(&r.action).as_str(),
                    dst_display: dst.as_deref().map(tilde),
                    dst: dst.map(|d| d.display().to_string()),
                });
                "wins"
            } else {
                "fails"
            };
            RuleCheck {
                name: r.name.clone(),
                enabled: r.enabled,
                checks,
                verdict,
            }
        })
        .collect();
    Ok(Inspect {
        facts: FactsDto::new(&facts, keep),
        rules,
        result,
    })
}

#[tauri::command(async)]
pub fn organize(state: State<AppState>, path: String) -> CmdResult<EntryDto> {
    let cfg = state.config()?;
    let journal = state.journal()?;
    match engine::organize(&cfg, &journal, Path::new(&path), SystemTime::now())? {
        Report::Done { id, .. } => {
            let e = journal
                .get(id)?
                .ok_or_else(|| CmdError::other("ação sumiu do histórico"))?;
            Ok(EntryDto::from(&e))
        }
        Report::Failed { error, .. } => Err(CmdError::other(error)),
        _ => Err(CmdError::other("nenhuma regra casou com este arquivo")),
    }
}

#[tauri::command]
pub fn initial_test_file(state: State<AppState>) -> Option<String> {
    state.initial_test_file.clone()
}

// ---------- Daemon e atualização ao vivo ----------

#[tauri::command(async)]
pub fn daemon_status(state: State<AppState>) -> DaemonStatus {
    let folders = state.config().map(|c| watch_dirs(&c).len()).unwrap_or(0);
    system::daemon_status(folders)
}

#[tauri::command(async)]
pub fn start_daemon(state: State<AppState>) -> CmdResult<DaemonStatus> {
    system::start_daemon().map_err(CmdError::other)?;
    Ok(daemon_status(state))
}

#[derive(Serialize)]
pub struct Live {
    max_id: i64,
    config_mtime: Option<String>,
}

#[tauri::command(async)]
pub fn live(state: State<AppState>) -> CmdResult<Live> {
    Ok(Live {
        max_id: state.journal()?.max_id()?,
        config_mtime: mtime_token(
            fs::metadata(&state.config_path)
                .and_then(|m| m.modified())
                .ok(),
        ),
    })
}
