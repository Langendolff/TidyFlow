# TidyFlow Fase 1 (núcleo): plano de implementação

> **Para agentes:** SUB-SKILL OBRIGATÓRIO: use superpowers:subagent-driven-development (recomendado) ou superpowers:executing-plans para implementar este plano tarefa por tarefa. Os passos usam checkbox (`- [ ]`) para acompanhamento.

**Objetivo:** daemon + CLI em Rust que organiza arquivos por regras TOML (incluindo a origem do download e a idade do arquivo), com histórico em SQLite, desfazer e notificações.

**Arquitetura:** workspace com dois crates. `tidyflow-core` é uma biblioteca com toda a lógica: config, facts, rules, template, actions, journal, stability e engine. `tidyflow` é o binário com a CLI e o daemon. O SQLite (WAL) é o estado compartilhado entre os processos, então não há IPC. O módulo `rules` é puro, e só `actions` escreve no disco.

**Tech Stack:** Rust (edition 2024, toolchain 1.96), notify, rusqlite (bundled), serde + toml, globset, regex, xattr, infer, mime_guess, trash, chrono, clap, notify-rust, tracing.

**Spec:** `docs/superpowers/specs/2026-11-15-tidyflow-fase1-design.md`

## Restrições globais

- Linux apenas (xattr, inotify, lixeira freedesktop). Nenhum código para outros sistemas.
- Exclusão sempre via `trash`, nunca `fs::remove_file` em arquivos do usuário (a única exceção é remover uma cópia parcial nossa ou a origem depois de uma cópia já verificada).
- Nunca sobrescrever: em caso de colisão, usar o sufixo `nome (1).ext`.
- Mensagens para o usuário em português.
- Ordem das operações: executar → verificar → gravar no journal.
- Config: `$XDG_CONFIG_HOME/tidyflow/config.toml` (padrão `~/.config/...`). Journal: `$XDG_DATA_HOME/tidyflow/journal.db` (padrão `~/.local/share/...`).
- Cada commit precisa passar em `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings` e `cargo test`.
- Dependências entram com `cargo add` (a versão é resolvida na hora). Se a API de um crate mudou em relação ao código deste plano, adapte a chamada e mantenha o comportamento.

## Estrutura de arquivos

```
Cargo.toml                         workspace
.gitignore
.github/workflows/ci.yml
crates/core/Cargo.toml
crates/core/src/lib.rs             módulos + tipos Result/Error + helpers XDG
crates/core/src/units.rs           "100MB" → bytes, "30d" → Duration
crates/core/src/config.rs          TOML → Config validado (com o número da linha nos erros)
crates/core/src/facts.rs           Facts::read(path): nome, ext, mime, tamanho, mtime, origem
crates/core/src/rules.rs           matches / first_match (puras)
crates/core/src/template.rs        {year} {month} {day} {ext} {name} {origin_host}
crates/core/src/actions.rs         Kind, Done, destination, execute, undo
crates/core/src/journal.rs         SQLite: record, get, recent, undo marks
crates/core/src/stability.rs       nomes temporários e espera de tamanho estável
crates/core/src/engine.rs          process / scan / undo (une tudo)
crates/cli/Cargo.toml              pacote `tidyflow`
crates/cli/src/main.rs             clap: daemon, run, undo, history, check
crates/cli/src/daemon.rs           watcher + timer + reload
crates/cli/src/desktop.rs          notificações D-Bus com Desfazer
crates/cli/tests/cli.rs            testes de integração do binário
contrib/tidyflow.service           unidade systemd de usuário
config.example.toml
README.md
```

---

### Tarefa 1: workspace, CI e `units`

**Arquivos:**
- Criar: `Cargo.toml`, `.gitignore`, `.github/workflows/ci.yml`, `crates/core/Cargo.toml`, `crates/core/src/lib.rs`, `crates/core/src/units.rs`

**Interfaces:**
- Produz: `tidyflow_core::Result<T>`, `tidyflow_core::Error` (= `Box<dyn std::error::Error + Send + Sync>`), `tidyflow_core::units::parse_size(&str) -> Result<u64, String>` e `tidyflow_core::units::parse_duration(&str) -> Result<Duration, String>`

- [ ] **Passo 1: criar o workspace**

`Cargo.toml`:
```toml
[workspace]
resolver = "3"
members = ["crates/core"]
```

`.gitignore`:
```
/target
```

`crates/core/Cargo.toml`:
```toml
[package]
name = "tidyflow-core"
version = "0.1.0"
edition = "2024"

[dependencies]
```

`crates/core/src/lib.rs`:
```rust
pub mod units;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T> = std::result::Result<T, Error>;
```

`.github/workflows/ci.yml`:
```yaml
name: CI
on: [push, pull_request]
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy, rustfmt
      - run: cargo fmt --all --check
      - run: cargo clippy --all-targets -- -D warnings
      - run: cargo test
```

- [ ] **Passo 2: escrever os testes que falham**

`crates/core/src/units.rs`:
```rust
use std::time::Duration;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(parse_size("100").unwrap(), 100);
        assert_eq!(parse_size("100B").unwrap(), 100);
        assert_eq!(parse_size("2KB").unwrap(), 2_000);
        assert_eq!(parse_size("1.5MB").unwrap(), 1_500_000);
        assert_eq!(parse_size("1GB").unwrap(), 1_000_000_000);
        assert_eq!(parse_size("1MiB").unwrap(), 1_048_576);
        assert_eq!(parse_size(" 3 KiB ").unwrap(), 3_072);
        assert!(parse_size("10XB").is_err());
        assert!(parse_size("MB").is_err());
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("30s").unwrap(), Duration::from_secs(30));
        assert_eq!(parse_duration("5m").unwrap(), Duration::from_secs(300));
        assert_eq!(parse_duration("12h").unwrap(), Duration::from_secs(43_200));
        assert_eq!(parse_duration("30d").unwrap(), Duration::from_secs(2_592_000));
        assert_eq!(parse_duration("2w").unwrap(), Duration::from_secs(1_209_600));
        assert!(parse_duration("30").is_err());
        assert!(parse_duration("d").is_err());
        assert!(parse_duration("3y").is_err());
    }
}
```

- [ ] **Passo 3: rodar e confirmar que falha**

Run: `cargo test -p tidyflow-core units`
Esperado: erro de compilação, `cannot find function parse_size`.

- [ ] **Passo 4: implementar** (acrescentar acima do `#[cfg(test)]` em `units.rs`)

```rust
/// "100MB" → 100_000_000. KB/MB/GB na base 1000; KiB/MiB/GiB na base 1024.
pub fn parse_size(s: &str) -> Result<u64, String> {
    let s = s.trim();
    let split = s
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let n: f64 = num.parse().map_err(|_| format!("tamanho inválido: {s:?}"))?;
    let mult: u64 = match unit.trim() {
        "" | "B" => 1,
        "KB" => 1_000,
        "MB" => 1_000_000,
        "GB" => 1_000_000_000,
        "KiB" => 1 << 10,
        "MiB" => 1 << 20,
        "GiB" => 1 << 30,
        u => return Err(format!("unidade de tamanho desconhecida: {u:?}")),
    };
    Ok((n * mult as f64) as u64)
}

/// "30d" → 30 dias. Unidades: s, m, h, d, w (obrigatória).
pub fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    let split = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let n: u64 = num.parse().map_err(|_| format!("duração inválida: {s:?}"))?;
    let secs = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3_600,
        "d" => 86_400,
        "w" => 604_800,
        u => return Err(format!("unidade de duração desconhecida: {u:?} (use s, m, h, d ou w)")),
    };
    Ok(Duration::from_secs(n * secs))
}
```

- [ ] **Passo 5: rodar e confirmar que passa**

Run: `cargo test -p tidyflow-core units && cargo clippy --all-targets -- -D warnings && cargo fmt --all --check`
Esperado: 2 testes passando, sem avisos.

- [ ] **Passo 6: commit**

```bash
git add Cargo.toml .gitignore .github crates/core
git commit -m "feat(core): workspace, CI e parser de tamanho/duração"
```

---

### Tarefa 2: `config` (parse e validação do TOML)

**Arquivos:**
- Criar: `crates/core/src/config.rs`
- Modificar: `crates/core/src/lib.rs`

**Interfaces:**
- Consome: `units::parse_size` e `units::parse_duration`
- Produz:
  ```rust
  pub struct Config { pub age_scan_interval: Duration, pub notify: bool, pub rules: Vec<Rule> }
  pub struct Rule { pub name: String, pub watch: Vec<PathBuf>, pub when: Conditions, pub action: Action }
  pub struct Conditions { pub ext: Option<Vec<String>>, pub name: Option<GlobMatcher>,
      pub name_regex: Option<Regex>, pub mime: Option<GlobMatcher>, pub origin: Option<GlobMatcher>,
      pub size_gt: Option<u64>, pub size_lt: Option<u64>, pub older_than: Option<Duration> }
  pub enum Action { Move(String), Copy(String), Rename(String), Trash }  // Move/Copy: diretório-template, com ~ já expandido
  pub struct ConfigError { pub line: Option<usize>, pub message: String }  // Display: "linha N: msg"
  pub fn parse(src: &str) -> Result<Config, ConfigError>
  pub fn load(path: &Path) -> Result<Config, ConfigError>
  pub fn default_path() -> PathBuf
  // em lib.rs:
  pub(crate) fn xdg_dir(var: &str, fallback: &str) -> PathBuf
  ```

- [ ] **Passo 1: adicionar as dependências**

Run: `cargo add -p tidyflow-core serde --features derive && cargo add -p tidyflow-core toml globset regex && cargo add -p tidyflow-core --dev tempfile`

- [ ] **Passo 2: registrar o módulo e os helpers XDG**

`crates/core/src/lib.rs` passa a ser:
```rust
pub mod config;
pub mod units;

use std::path::PathBuf;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn home() -> String {
    std::env::var("HOME").unwrap_or_default()
}

/// Diretório XDG (`var`) ou `$HOME/<fallback>` quando a variável está vazia ou ausente.
pub(crate) fn xdg_dir(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(home()).join(fallback))
}
```

- [ ] **Passo 3: escrever os testes que falham**

`crates/core/src/config.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> String {
        std::env::temp_dir().display().to_string()
    }

    fn rule(when: &str, action: &str) -> String {
        format!(
            "[[rule]]\nname = \"r\"\nwatch = [\"{}\"]\nwhen = {when}\naction = {action}\n",
            tmp()
        )
    }

    #[test]
    fn parses_full_rule() {
        let src = format!(
            "[settings]\nage_scan_interval = \"30m\"\nnotify = false\n\n{}",
            rule(
                r#"{ ext = [".PDF", "xml"], name = "*nota*", name_regex = "(?i)nf", mime = "application/*", origin = "*.gov.br", size_gt = "1KB", size_lt = "10MB", older_than = "2d" }"#,
                r#"{ move = "~/Docs/{year}/" }"#,
            )
        );
        let cfg = parse(&src).unwrap();
        assert_eq!(cfg.age_scan_interval, Duration::from_secs(1800));
        assert!(!cfg.notify);
        let r = &cfg.rules[0];
        assert_eq!(r.name, "r");
        assert_eq!(r.watch, vec![PathBuf::from(tmp())]);
        assert_eq!(r.when.ext, Some(vec!["pdf".to_string(), "xml".to_string()]));
        assert_eq!(r.when.size_gt, Some(1_000));
        assert_eq!(r.when.older_than, Some(Duration::from_secs(172_800)));
        assert_eq!(r.action, Action::Move(format!("{}/Docs/{{year}}/", crate::home())));
    }

    #[test]
    fn defaults() {
        let cfg = parse(&rule(r#"{ ext = ["pdf"] }"#, "{ trash = true }")).unwrap();
        assert_eq!(cfg.age_scan_interval, Duration::from_secs(3600));
        assert!(cfg.notify);
        assert_eq!(cfg.rules[0].action, Action::Trash);
    }

    #[test]
    fn syntax_error_has_line() {
        let err = parse("[settings]\nnotify = \n").unwrap_err();
        assert_eq!(err.line, Some(2));
    }

    #[test]
    fn unknown_key_has_line() {
        let src = format!(
            "[[rule]]\nname = \"r\"\nwatch = [\"{}\"]\naction = {{ trash = true }}\n\n[rule.when]\nfoo = 1\n",
            tmp()
        );
        let err = parse(&src).unwrap_err();
        assert!(err.message.contains("foo"), "{err}");
        assert_eq!(err.line, Some(7));
    }

    #[test]
    fn semantic_errors_name_the_rule() {
        let cases = [
            rule("{}", "{ trash = true }"),
            rule(r#"{ ext = ["pdf"] }"#, "{}"),
            rule(r#"{ ext = ["pdf"] }"#, r#"{ trash = true, move = "/x" }"#),
            rule(r#"{ name_regex = "(" }"#, "{ trash = true }"),
            rule(r#"{ size_gt = "10XB" }"#, "{ trash = true }"),
            "[[rule]]\nname = \"r\"\nwatch = [\"/nao/existe/tidyflow\"]\nwhen = { ext = [\"pdf\"] }\naction = { trash = true }\n".to_string(),
        ];
        for src in cases {
            let err = parse(&src).unwrap_err();
            assert!(err.message.contains("regra \"r\""), "{err}");
            assert_eq!(err.line, Some(1), "{err}");
        }
    }

    #[test]
    fn display_includes_line() {
        let e = ConfigError { line: Some(3), message: "x".into() };
        assert_eq!(e.to_string(), "linha 3: x");
    }
}
```

- [ ] **Passo 4: rodar e confirmar que falha**

Run: `cargo test -p tidyflow-core config`
Esperado: erros de compilação (`parse`, `Config` etc. não existem).

- [ ] **Passo 5: implementar** (acima do `#[cfg(test)]`)

```rust
use crate::units::{parse_duration, parse_size};
use globset::{GlobBuilder, GlobMatcher};
use regex::Regex;
use serde::Deserialize;
use std::{
    fmt, fs,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug)]
pub struct Config {
    pub age_scan_interval: Duration,
    pub notify: bool,
    pub rules: Vec<Rule>,
}

#[derive(Debug)]
pub struct Rule {
    pub name: String,
    pub watch: Vec<PathBuf>,
    pub when: Conditions,
    pub action: Action,
}

#[derive(Debug)]
pub struct Conditions {
    pub ext: Option<Vec<String>>,
    pub name: Option<GlobMatcher>,
    pub name_regex: Option<Regex>,
    pub mime: Option<GlobMatcher>,
    pub origin: Option<GlobMatcher>,
    pub size_gt: Option<u64>,
    pub size_lt: Option<u64>,
    pub older_than: Option<Duration>,
}

impl Conditions {
    fn is_empty(&self) -> bool {
        self.ext.is_none()
            && self.name.is_none()
            && self.name_regex.is_none()
            && self.mime.is_none()
            && self.origin.is_none()
            && self.size_gt.is_none()
            && self.size_lt.is_none()
            && self.older_than.is_none()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Move(String),
    Copy(String),
    Rename(String),
    Trash,
}

#[derive(Debug)]
pub struct ConfigError {
    pub line: Option<usize>,
    pub message: String,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "linha {line}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for ConfigError {}

pub fn default_path() -> PathBuf {
    crate::xdg_dir("XDG_CONFIG_HOME", ".config").join("tidyflow/config.toml")
}

pub fn load(path: &Path) -> Result<Config, ConfigError> {
    let src = fs::read_to_string(path).map_err(|e| ConfigError {
        line: None,
        message: format!("não foi possível ler {}: {e}", path.display()),
    })?;
    parse(&src)
}

pub fn parse(src: &str) -> Result<Config, ConfigError> {
    let raw: RawConfig = toml::from_str(src).map_err(|e| ConfigError {
        line: e.span().map(|s| line_of(src, s.start)),
        message: e.message().to_string(),
    })?;
    let age_scan_interval = raw
        .settings
        .age_scan_interval
        .as_deref()
        .map(parse_duration)
        .transpose()
        .map_err(|message| ConfigError { line: None, message })?
        .unwrap_or(Duration::from_secs(3600));
    let rules = raw
        .rules
        .into_iter()
        .map(|spanned| {
            let line = line_of(src, spanned.span().start);
            compile_rule(spanned.into_inner()).map_err(|message| ConfigError {
                line: Some(line),
                message,
            })
        })
        .collect::<Result<_, _>>()?;
    Ok(Config {
        age_scan_interval,
        notify: raw.settings.notify.unwrap_or(true),
        rules,
    })
}

fn line_of(src: &str, offset: usize) -> usize {
    src[..offset.min(src.len())].matches('\n').count() + 1
}

fn expand_tilde(s: &str) -> String {
    match s.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("{}{rest}", crate::home()),
        _ => s.to_string(),
    }
}

fn glob(pattern: &str) -> Result<GlobMatcher, String> {
    GlobBuilder::new(pattern)
        .case_insensitive(true)
        .build()
        .map(|g| g.compile_matcher())
        .map_err(|e| format!("glob inválido {pattern:?}: {e}"))
}

fn compile_rule(r: RawRule) -> Result<Rule, String> {
    let name = r.name;
    let err = |m: String| format!("regra {name:?}: {m}");

    if r.watch.is_empty() {
        return Err(err("`watch` está vazio".into()));
    }
    let watch = r
        .watch
        .iter()
        .map(|w| {
            let p = PathBuf::from(expand_tilde(w));
            if p.is_dir() {
                Ok(p)
            } else {
                Err(err(format!("pasta `watch` não existe: {}", p.display())))
            }
        })
        .collect::<Result<Vec<_>, _>>()?;

    let w = r.when;
    let when = Conditions {
        ext: w.ext.map(|v| {
            v.iter()
                .map(|e| e.trim_start_matches('.').to_lowercase())
                .collect()
        }),
        name: w.name.as_deref().map(glob).transpose().map_err(err)?,
        name_regex: w
            .name_regex
            .as_deref()
            .map(|r| Regex::new(r).map_err(|e| format!("regex inválida: {e}")))
            .transpose()
            .map_err(err)?,
        mime: w.mime.as_deref().map(glob).transpose().map_err(err)?,
        origin: w.origin.as_deref().map(glob).transpose().map_err(err)?,
        size_gt: w.size_gt.as_deref().map(parse_size).transpose().map_err(err)?,
        size_lt: w.size_lt.as_deref().map(parse_size).transpose().map_err(err)?,
        older_than: w
            .older_than
            .as_deref()
            .map(parse_duration)
            .transpose()
            .map_err(err)?,
    };
    if when.is_empty() {
        return Err(err("`when` precisa de pelo menos uma condição".into()));
    }

    let a = r.action;
    let actions: Vec<Action> = [
        a.r#move.map(|d| Action::Move(expand_tilde(&d))),
        a.copy.map(|d| Action::Copy(expand_tilde(&d))),
        a.rename.map(Action::Rename),
        a.trash.filter(|t| *t).map(|_| Action::Trash),
    ]
    .into_iter()
    .flatten()
    .collect();
    let Ok([action]) = <[Action; 1]>::try_from(actions) else {
        return Err(err(
            "`action` precisa de exatamente uma ação: move, copy, rename ou trash".into(),
        ));
    };

    Ok(Rule { name, watch, when, action })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    #[serde(default)]
    settings: RawSettings,
    #[serde(default, rename = "rule")]
    rules: Vec<toml::Spanned<RawRule>>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawSettings {
    age_scan_interval: Option<String>,
    notify: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRule {
    name: String,
    watch: Vec<String>,
    #[serde(default)]
    when: RawWhen,
    #[serde(default)]
    action: RawAction,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawWhen {
    ext: Option<Vec<String>>,
    name: Option<String>,
    name_regex: Option<String>,
    mime: Option<String>,
    origin: Option<String>,
    size_gt: Option<String>,
    size_lt: Option<String>,
    older_than: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawAction {
    r#move: Option<String>,
    copy: Option<String>,
    rename: Option<String>,
    trash: Option<bool>,
}
```

Nota: se a linha de `unknown_key_has_line` ou de `semantic_errors_name_the_rule` sair diferente (o span do `toml` pode apontar para o cabeçalho ou para a chave), ajuste o valor esperado no teste para a linha que o `toml` realmente reporta. O requisito é ter um número de linha útil; não importa se é o da chave ou o do cabeçalho.

- [ ] **Passo 6: rodar e confirmar que passa**

Run: `cargo test -p tidyflow-core config && cargo clippy --all-targets -- -D warnings`
Esperado: 6 testes passando.

- [ ] **Passo 7: commit**

```bash
git add -A && git commit -m "feat(core): config TOML com validação e linha nos erros"
```

---

### Tarefa 3: `facts`

**Arquivos:**
- Criar: `crates/core/src/facts.rs`
- Modificar: `crates/core/src/lib.rs` (adicionar `pub mod facts;`)

**Interfaces:**
- Produz:
  ```rust
  pub const ORIGIN_XATTR: &str = "user.xdg.origin.url";
  pub struct Facts { pub path: PathBuf, pub name: String, pub stem: String, pub ext: String,
      pub mime: String, pub size: u64, pub modified: SystemTime, pub origin_url: Option<String> }
  impl Facts {
      pub fn read(path: &Path) -> io::Result<Facts>;
      pub fn origin_host(&self) -> Option<String>;   // minúsculo, sem porta nem credenciais
      pub fn age(&self, now: SystemTime) -> Duration; // 0 se mtime estiver no futuro
      #[cfg(test)] pub(crate) fn sample(name: &str) -> Facts; // em temp_dir, mtime 2026-06-15 12:00 UTC
  }
  ```
  `ext` fica minúsculo e sem o ponto (`""` se não houver extensão). `stem` é o nome sem a extensão.

- [ ] **Passo 1: dependências**

Run: `cargo add -p tidyflow-core xattr infer mime_guess`

- [ ] **Passo 2: escrever os testes que falham**

`crates/core/src/facts.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, time::UNIX_EPOCH};

    #[test]
    fn reads_basic_facts() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("Relatório.Final.PDF");
        fs::write(&p, b"%PDF-1.4 resto").unwrap();
        let f = Facts::read(&p).unwrap();
        assert_eq!(f.name, "Relatório.Final.PDF");
        assert_eq!(f.stem, "Relatório.Final");
        assert_eq!(f.ext, "pdf");
        assert_eq!(f.mime, "application/pdf");
        assert_eq!(f.size, 14);
        assert_eq!(f.origin_url, None);
    }

    #[test]
    fn mime_from_content_beats_extension() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("foto.txt");
        fs::write(&p, [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0]).unwrap();
        assert_eq!(Facts::read(&p).unwrap().mime, "image/png");
    }

    #[test]
    fn mime_falls_back_to_extension() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("dados.csv");
        fs::write(&p, "a,b\n1,2\n").unwrap();
        assert_eq!(Facts::read(&p).unwrap().mime, "text/csv");
        let p = dir.path().join("sem_extensao");
        fs::write(&p, "x").unwrap();
        let f = Facts::read(&p).unwrap();
        assert_eq!(f.ext, "");
        assert_eq!(f.mime, "application/octet-stream");
    }

    #[test]
    fn reads_origin_xattr() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("fatura.pdf");
        fs::write(&p, "x").unwrap();
        xattr::set(&p, ORIGIN_XATTR, b"https://user:pw@App.Nubank.com.br:443/x?y=1").unwrap();
        let f = Facts::read(&p).unwrap();
        assert_eq!(f.origin_host().as_deref(), Some("app.nubank.com.br"));
    }

    #[test]
    fn host_parsing() {
        let mut f = Facts::sample("a");
        for (url, host) in [
            ("https://github.com/a/b", Some("github.com")),
            ("http://x.org", Some("x.org")),
            ("https://h.com#frag", Some("h.com")),
            ("not a url", None),
            ("file:///home/x", None),
        ] {
            f.origin_url = Some(url.into());
            assert_eq!(f.origin_host().as_deref(), host, "{url}");
        }
    }

    #[test]
    fn age_never_negative() {
        let f = Facts::sample("a");
        assert_eq!(f.age(UNIX_EPOCH), Duration::ZERO);
        assert_eq!(f.age(f.modified + Duration::from_secs(10)), Duration::from_secs(10));
    }

    #[test]
    fn missing_file_is_not_found() {
        let err = Facts::read(Path::new("/nao/existe/x.pdf")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }
}
```

- [ ] **Passo 3: rodar e confirmar que falha**

Run: `cargo test -p tidyflow-core facts`
Esperado: erro de compilação (`Facts` não existe).

- [ ] **Passo 4: implementar**

```rust
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

/// Gravado por Chromium/Chrome, `wget --xattr` e `curl --xattr`.
pub const ORIGIN_XATTR: &str = "user.xdg.origin.url";

#[derive(Debug, Clone)]
pub struct Facts {
    pub path: PathBuf,
    pub name: String,
    pub stem: String,
    pub ext: String,
    pub mime: String,
    pub size: u64,
    pub modified: SystemTime,
    pub origin_url: Option<String>,
}

impl Facts {
    pub fn read(path: &Path) -> io::Result<Facts> {
        let meta = fs::metadata(path)?;
        let lossy = |s: Option<&std::ffi::OsStr>| s.map(|s| s.to_string_lossy().into_owned());
        let mime = match infer::get_from_path(path)? {
            Some(t) => t.mime_type().to_string(),
            None => mime_guess::from_path(path).first_or_octet_stream().to_string(),
        };
        // Falha de xattr (fs sem suporte) = sem origem.
        let origin_url = xattr::get(path, ORIGIN_XATTR)
            .ok()
            .flatten()
            .and_then(|v| String::from_utf8(v).ok());
        Ok(Facts {
            path: path.to_path_buf(),
            name: lossy(path.file_name()).unwrap_or_default(),
            stem: lossy(path.file_stem()).unwrap_or_default(),
            ext: lossy(path.extension()).unwrap_or_default().to_lowercase(),
            mime,
            size: meta.len(),
            modified: meta.modified()?,
            origin_url,
        })
    }

    pub fn origin_host(&self) -> Option<String> {
        let rest = self.origin_url.as_deref()?.split_once("://")?.1;
        let authority = rest.split(['/', '?', '#']).next()?;
        let host = authority.rsplit('@').next()?.split(':').next()?;
        (!host.is_empty()).then(|| host.to_ascii_lowercase())
    }

    pub fn age(&self, now: SystemTime) -> Duration {
        now.duration_since(self.modified).unwrap_or_default()
    }

    #[cfg(test)]
    pub(crate) fn sample(name: &str) -> Facts {
        let path = std::env::temp_dir().join(name);
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        Facts {
            stem: path.file_stem().unwrap().to_string_lossy().into_owned(),
            name: name.to_string(),
            path,
            ext,
            mime: "application/octet-stream".into(),
            size: 0,
            modified: std::time::UNIX_EPOCH + Duration::from_secs(1_781_524_800), // 2026-06-15 12:00 UTC
            origin_url: None,
        }
    }
}
```

- [ ] **Passo 5: rodar e confirmar que passa**

Run: `cargo test -p tidyflow-core facts && cargo clippy --all-targets -- -D warnings`
Esperado: 7 testes passando. Se `reads_origin_xattr` falhar com `Operation not supported`, o `/tmp` não aceita xattr de usuário: rode com `TMPDIR=$HOME/.cache cargo test`.

- [ ] **Passo 6: commit**

```bash
git add -A && git commit -m "feat(core): facts com MIME por conteúdo e origem via xattr"
```

---

### Tarefa 4: `rules` (funções puras)

**Arquivos:**
- Criar: `crates/core/src/rules.rs`
- Modificar: `crates/core/src/lib.rs` (`pub mod rules;`)

**Interfaces:**
- Consome: `config::{Conditions, Rule}`, `facts::Facts`
- Produz:
  ```rust
  pub fn matches(c: &Conditions, f: &Facts, now: SystemTime) -> bool
  pub fn first_match<'a>(rules: &'a [Rule], f: &Facts, now: SystemTime) -> Option<&'a Rule>
  // first_match só considera regras cujo `watch` contém a pasta pai do arquivo
  ```

- [ ] **Passo 1: escrever os testes que falham**

`crates/core/src/rules.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{self, Config};
    use std::time::Duration;

    fn cfg(rules: &[(&str, &str)]) -> Config {
        let tmp = std::env::temp_dir().display().to_string();
        let src: String = rules
            .iter()
            .map(|(name, when)| {
                format!("[[rule]]\nname = \"{name}\"\nwatch = [\"{tmp}\"]\nwhen = {when}\naction = {{ trash = true }}\n\n")
            })
            .collect();
        config::parse(&src).unwrap()
    }

    fn hit(when: &str, f: &Facts) -> bool {
        matches(&cfg(&[("r", when)]).rules[0].when, f, f.modified)
    }

    #[test]
    fn each_condition() {
        let mut f = Facts::sample("Nota_Fiscal.PDF");
        f.mime = "application/pdf".into();
        f.size = 5_000;
        f.origin_url = Some("https://www.receita.gov.br/x".into());

        let table = [
            (r#"{ ext = ["pdf"] }"#, true),
            (r#"{ ext = ["png", "jpg"] }"#, false),
            (r#"{ name = "nota_*" }"#, true), // glob ignora maiúsculas
            (r#"{ name = "*.zip" }"#, false),
            (r#"{ name_regex = "(?i)fiscal" }"#, true),
            (r#"{ name_regex = "^fiscal" }"#, false),
            (r#"{ mime = "application/*" }"#, true),
            (r#"{ mime = "image/*" }"#, false),
            (r#"{ origin = "*.gov.br" }"#, true),
            (r#"{ origin = "*.nubank.com.br" }"#, false),
            (r#"{ size_gt = "1KB" }"#, true),
            (r#"{ size_gt = "5KB" }"#, false), // estritamente maior
            (r#"{ size_lt = "1MB" }"#, true),
            (r#"{ size_lt = "1KB" }"#, false),
            (r#"{ ext = ["pdf"], size_gt = "10KB" }"#, false), // E lógico
        ];
        for (when, expected) in table {
            assert_eq!(hit(when, &f), expected, "{when}");
        }
    }

    #[test]
    fn origin_condition_needs_origin() {
        let f = Facts::sample("a.pdf");
        assert!(!hit(r#"{ origin = "*" }"#, &f));
    }

    #[test]
    fn older_than() {
        let c = cfg(&[("r", r#"{ older_than = "1d" }"#)]);
        let f = Facts::sample("a.pdf");
        let day = Duration::from_secs(86_400);
        assert!(!matches(&c.rules[0].when, &f, f.modified + day));
        assert!(matches(&c.rules[0].when, &f, f.modified + day + Duration::from_secs(1)));
    }

    #[test]
    fn first_match_wins_and_respects_watch() {
        let c = cfg(&[("pdf", r#"{ ext = ["pdf"] }"#), ("tudo", r#"{ name = "*" }"#)]);
        let f = Facts::sample("a.pdf");
        assert_eq!(first_match(&c.rules, &f, f.modified).unwrap().name, "pdf");
        let g = Facts::sample("a.txt");
        assert_eq!(first_match(&c.rules, &g, g.modified).unwrap().name, "tudo");

        let mut outside = Facts::sample("a.pdf");
        outside.path = "/outra/pasta/a.pdf".into();
        assert!(first_match(&c.rules, &outside, outside.modified).is_none());
    }
}
```

- [ ] **Passo 2: rodar e confirmar que falha**

Run: `cargo test -p tidyflow-core rules`
Esperado: erro de compilação (`matches` não existe).

- [ ] **Passo 3: implementar**

```rust
use crate::{
    config::{Conditions, Rule},
    facts::Facts,
};
use std::time::SystemTime;

pub fn matches(c: &Conditions, f: &Facts, now: SystemTime) -> bool {
    c.ext.as_ref().is_none_or(|exts| exts.contains(&f.ext))
        && c.name.as_ref().is_none_or(|g| g.is_match(&f.name))
        && c.name_regex.as_ref().is_none_or(|r| r.is_match(&f.name))
        && c.mime.as_ref().is_none_or(|g| g.is_match(&f.mime))
        && c.origin
            .as_ref()
            .is_none_or(|g| f.origin_host().is_some_and(|h| g.is_match(&h)))
        && c.size_gt.is_none_or(|n| f.size > n)
        && c.size_lt.is_none_or(|n| f.size < n)
        && c.older_than.is_none_or(|d| f.age(now) > d)
}

pub fn first_match<'a>(rules: &'a [Rule], f: &Facts, now: SystemTime) -> Option<&'a Rule> {
    let dir = f.path.parent()?;
    rules
        .iter()
        .find(|r| r.watch.iter().any(|w| w == dir) && matches(&r.when, f, now))
}
```

- [ ] **Passo 4: rodar e confirmar que passa**

Run: `cargo test -p tidyflow-core rules && cargo clippy --all-targets -- -D warnings`
Esperado: 4 testes passando.

- [ ] **Passo 5: commit**

```bash
git add -A && git commit -m "feat(core): avaliação de regras"
```

---

### Tarefa 5: `template`

**Arquivos:**
- Criar: `crates/core/src/template.rs`
- Modificar: `crates/core/src/lib.rs` (`pub mod template;`)

**Interfaces:**
- Consome: `facts::Facts`
- Produz: `pub fn render(template: &str, f: &Facts) -> String`, com as variáveis `{year}`, `{month}`, `{day}` (mtime no fuso local, com zero à esquerda), `{ext}`, `{name}` (= stem) e `{origin_host}` (`unknown` se não houver origem)

- [ ] **Passo 1: dependência**

Run: `cargo add -p tidyflow-core chrono`

- [ ] **Passo 2: escrever o teste que falha**

`crates/core/src/template.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_all_variables() {
        // sample: 2026-06-15 12:00 UTC → mesmo dia em qualquer fuso
        let mut f = Facts::sample("Fatura.PDF");
        assert_eq!(
            render("/d/{year}/{month}-{day}/{name}.{ext}/{origin_host}", &f),
            "/d/2026/06-15/Fatura.pdf/unknown"
        );
        f.origin_url = Some("https://nubank.com.br/x".into());
        assert_eq!(render("{origin_host}", &f), "nubank.com.br");
        assert_eq!(render("sem variáveis", &f), "sem variáveis");
    }
}
```

- [ ] **Passo 3: rodar e confirmar que falha**

Run: `cargo test -p tidyflow-core template`
Esperado: erro de compilação (`render` não existe).

- [ ] **Passo 4: implementar**

```rust
use crate::facts::Facts;
use chrono::{DateTime, Local};

// ponytail: replace encadeado; um valor que contenha "{...}" pode ser substituído de novo.
// Trocar por um parser de um passo se isso virar problema real.
pub fn render(template: &str, f: &Facts) -> String {
    let t: DateTime<Local> = f.modified.into();
    template
        .replace("{year}", &t.format("%Y").to_string())
        .replace("{month}", &t.format("%m").to_string())
        .replace("{day}", &t.format("%d").to_string())
        .replace("{ext}", &f.ext)
        .replace("{name}", &f.stem)
        .replace(
            "{origin_host}",
            &f.origin_host().unwrap_or_else(|| "unknown".into()),
        )
}
```

- [ ] **Passo 5: rodar e confirmar que passa**

Run: `cargo test -p tidyflow-core template && cargo clippy --all-targets -- -D warnings`

- [ ] **Passo 6: commit**

```bash
git add -A && git commit -m "feat(core): templates de destino"
```

---

### Tarefa 6: `actions` (executar e desfazer no disco)

**Arquivos:**
- Criar: `crates/core/src/actions.rs`
- Modificar: `crates/core/src/lib.rs` (`pub mod actions;`)

**Interfaces:**
- Consome: `config::Action`, `facts::Facts`, `template::render`
- Produz:
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Kind { Move, Copy, Rename, Trash }
  impl Kind { pub fn as_str(self) -> &'static str; pub fn parse(s: &str) -> Option<Kind> } // "move" | "copy" | "rename" | "trash"
  pub fn kind_of(a: &Action) -> Kind
  #[derive(Debug, Clone, PartialEq)] pub struct Done { pub kind: Kind, pub src: PathBuf, pub dst: Option<PathBuf> } // dst = None para Trash
  pub fn destination(a: &Action, f: &Facts) -> Option<PathBuf>  // sem resolver colisão; None para Trash
  pub fn execute(a: &Action, f: &Facts) -> io::Result<Option<Done>> // Ok(None) = já está no destino
  pub fn undo(kind: Kind, src: &Path, dst: Option<&Path>) -> io::Result<()>
  ```

- [ ] **Passo 1: dependência**

Run: `cargo add -p tidyflow-core trash`

- [ ] **Passo 2: escrever os testes que falham**

`crates/core/src/actions.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn file(dir: &Path, name: &str, body: &str) -> Facts {
        let p = dir.join(name);
        fs::write(&p, body).unwrap();
        Facts::read(&p).unwrap()
    }

    fn mv(dir: &Path) -> Action {
        Action::Move(format!("{}/out/{{ext}}", dir.display()))
    }

    #[test]
    fn kind_roundtrip() {
        for k in [Kind::Move, Kind::Copy, Kind::Rename, Kind::Trash] {
            assert_eq!(Kind::parse(k.as_str()), Some(k));
        }
        assert_eq!(Kind::parse("x"), None);
    }

    #[test]
    fn move_creates_dirs_and_undo_restores() {
        let d = tempfile::tempdir().unwrap();
        let f = file(d.path(), "a.pdf", "1");
        let done = execute(&mv(d.path()), &f).unwrap().unwrap();
        let dst = d.path().join("out/pdf/a.pdf");
        assert_eq!(done, Done { kind: Kind::Move, src: f.path.clone(), dst: Some(dst.clone()) });
        assert!(dst.exists() && !f.path.exists());

        undo(done.kind, &done.src, done.dst.as_deref()).unwrap();
        assert!(f.path.exists() && !dst.exists());
    }

    #[test]
    fn never_overwrites() {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir_all(d.path().join("out/pdf")).unwrap();
        fs::write(d.path().join("out/pdf/a.pdf"), "velho").unwrap();
        fs::write(d.path().join("out/pdf/a (1).pdf"), "velho").unwrap();
        let f = file(d.path(), "a.pdf", "novo");
        let done = execute(&mv(d.path()), &f).unwrap().unwrap();
        assert_eq!(done.dst.unwrap(), d.path().join("out/pdf/a (2).pdf"));
        assert_eq!(fs::read_to_string(d.path().join("out/pdf/a.pdf")).unwrap(), "velho");
    }

    #[test]
    fn copy_keeps_source_and_undo_trashes_copy() {
        let d = tempfile::tempdir().unwrap();
        let f = file(d.path(), "a.txt", "x");
        let a = Action::Copy(format!("{}/bak", d.path().display()));
        let done = execute(&a, &f).unwrap().unwrap();
        let dst = done.dst.clone().unwrap();
        assert!(f.path.exists() && dst.exists());
        undo(done.kind, &done.src, done.dst.as_deref()).unwrap();
        assert!(f.path.exists() && !dst.exists());
    }

    #[test]
    fn rename_and_noop_when_already_named() {
        let d = tempfile::tempdir().unwrap();
        let f = file(d.path(), "a.txt", "x");
        let a = Action::Rename("doc-{name}.{ext}".into());
        let done = execute(&a, &f).unwrap().unwrap();
        assert_eq!(done.dst.unwrap(), d.path().join("doc-a.txt"));

        let g = file(d.path(), "b.txt", "x");
        assert_eq!(execute(&Action::Rename("{name}.{ext}".into()), &g).unwrap(), None);
    }

    #[test]
    fn cross_device_path_copies_then_removes() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("a.bin");
        let dst = d.path().join("b.bin");
        fs::write(&src, vec![7u8; 10_000]).unwrap();
        move_via_copy(&src, &dst).unwrap();
        assert!(!src.exists());
        assert_eq!(fs::read(&dst).unwrap(), vec![7u8; 10_000]);
    }

    #[test]
    fn failed_copy_leaves_source_and_no_partial() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("a.bin");
        fs::write(&src, "x").unwrap();
        let dst = d.path().join("nao/existe/b.bin"); // pasta pai ausente → fs::copy falha
        assert!(move_via_copy(&src, &dst).is_err());
        assert!(src.exists() && !dst.exists());
    }

    #[test]
    fn undo_with_occupied_source_uses_suffix() {
        let d = tempfile::tempdir().unwrap();
        let f = file(d.path(), "a.pdf", "1");
        let done = execute(&mv(d.path()), &f).unwrap().unwrap();
        fs::write(&f.path, "outro").unwrap();
        undo(done.kind, &done.src, done.dst.as_deref()).unwrap();
        assert_eq!(fs::read_to_string(d.path().join("a (1).pdf")).unwrap(), "1");
        assert_eq!(fs::read_to_string(&f.path).unwrap(), "outro");
    }

    #[test]
    fn undo_with_missing_destination_errors() {
        let err = undo(Kind::Move, Path::new("/tmp/x"), Some(Path::new("/nao/existe"))).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert!(err.to_string().contains("/nao/existe"));
    }

    #[test]
    fn trash_and_restore() {
        let d = tempfile::tempdir().unwrap();
        let f = file(d.path(), "lixo.txt", "x");
        let done = execute(&Action::Trash, &f).unwrap().unwrap();
        assert!(!f.path.exists() && done.dst.is_none());
        undo(done.kind, &done.src, None).unwrap();
        assert!(f.path.exists());
    }
}
```

- [ ] **Passo 3: rodar e confirmar que falha**

Run: `cargo test -p tidyflow-core actions`
Esperado: erro de compilação.

- [ ] **Passo 4: implementar**

```rust
use crate::{config::Action, facts::Facts, template::render};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Move,
    Copy,
    Rename,
    Trash,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Move => "move",
            Kind::Copy => "copy",
            Kind::Rename => "rename",
            Kind::Trash => "trash",
        }
    }

    pub fn parse(s: &str) -> Option<Kind> {
        [Kind::Move, Kind::Copy, Kind::Rename, Kind::Trash]
            .into_iter()
            .find(|k| k.as_str() == s)
    }
}

pub fn kind_of(a: &Action) -> Kind {
    match a {
        Action::Move(_) => Kind::Move,
        Action::Copy(_) => Kind::Copy,
        Action::Rename(_) => Kind::Rename,
        Action::Trash => Kind::Trash,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Done {
    pub kind: Kind,
    pub src: PathBuf,
    pub dst: Option<PathBuf>,
}

pub fn destination(a: &Action, f: &Facts) -> Option<PathBuf> {
    match a {
        Action::Move(dir) | Action::Copy(dir) => Some(Path::new(&render(dir, f)).join(&f.name)),
        Action::Rename(name) => Some(f.path.with_file_name(render(name, f))),
        Action::Trash => None,
    }
}

pub fn execute(a: &Action, f: &Facts) -> io::Result<Option<Done>> {
    let kind = kind_of(a);
    let Some(target) = destination(a, f) else {
        trash::delete(&f.path).map_err(|e| io::Error::other(e.to_string()))?;
        return Ok(Some(Done { kind, src: f.path.clone(), dst: None }));
    };
    if target == f.path {
        return Ok(None);
    }
    let dst = free_path(&target);
    if let Some(dir) = dst.parent() {
        fs::create_dir_all(dir)?;
    }
    match kind {
        Kind::Copy => copy_verified(&f.path, &dst)?,
        _ => move_file(&f.path, &dst)?,
    }
    Ok(Some(Done { kind, src: f.path.clone(), dst: Some(dst) }))
}

pub fn undo(kind: Kind, src: &Path, dst: Option<&Path>) -> io::Result<()> {
    if kind == Kind::Trash {
        return restore_from_trash(src);
    }
    let dst = dst.ok_or_else(|| io::Error::other("ação sem destino registrado"))?;
    if !dst.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("arquivo não encontrado em {}", dst.display()),
        ));
    }
    if kind == Kind::Copy {
        // A cópia é redundante, mas vai para a lixeira: nunca apagar de vez.
        return trash::delete(dst).map_err(|e| io::Error::other(e.to_string()));
    }
    let target = free_path(src);
    if let Some(dir) = target.parent() {
        fs::create_dir_all(dir)?;
    }
    move_file(dst, &target)
}

fn restore_from_trash(src: &Path) -> io::Result<()> {
    let item = trash::os_limited::list()
        .map_err(|e| io::Error::other(e.to_string()))?
        .into_iter()
        .filter(|i| i.original_path() == src)
        .max_by_key(|i| i.time_deleted)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("{} não está na lixeira", src.display()),
            )
        })?;
    trash::os_limited::restore_all([item]).map_err(|e| io::Error::other(e.to_string()))
}

// ponytail: verificar e depois usar (TOCTOU). Duas instâncias escrevendo no mesmo
// destino ao mesmo tempo podem colidir. Usar O_EXCL / renameat2(NOREPLACE) se isso acontecer.
fn free_path(p: &Path) -> PathBuf {
    if !p.exists() {
        return p.to_path_buf();
    }
    let stem = p.file_stem().unwrap_or_default().to_string_lossy();
    let ext = p.extension().map(|e| e.to_string_lossy());
    (1..)
        .map(|n| match &ext {
            Some(e) => p.with_file_name(format!("{stem} ({n}).{e}")),
            None => p.with_file_name(format!("{stem} ({n})")),
        })
        .find(|c| !c.exists())
        .expect("sufixo livre")
}

fn move_file(src: &Path, dst: &Path) -> io::Result<()> {
    match fs::rename(src, dst) {
        Err(e) if e.kind() == io::ErrorKind::CrossesDevices => move_via_copy(src, dst),
        r => r,
    }
}

pub(crate) fn move_via_copy(src: &Path, dst: &Path) -> io::Result<()> {
    copy_verified(src, dst)?;
    fs::remove_file(src)
}

fn copy_verified(src: &Path, dst: &Path) -> io::Result<()> {
    let result = (|| {
        let n = fs::copy(src, dst)?;
        fs::File::open(dst)?.sync_all()?;
        if n != fs::metadata(src)?.len() {
            return Err(io::Error::other("tamanho da cópia não confere"));
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(dst); // só a nossa cópia parcial
    }
    result
}
```

- [ ] **Passo 5: rodar e confirmar que passa**

Run: `cargo test -p tidyflow-core actions && cargo clippy --all-targets -- -D warnings`
Esperado: 10 testes passando. O `trash_and_restore` usa a lixeira real do sistema. Se falhar porque o `/tmp` é tmpfs sem `.Trash-$UID` gravável, rode com `TMPDIR=$HOME/.cache cargo test`. Se a API do crate `trash` estiver diferente (`time_deleted`, `original_path`), adapte para a versão instalada.

- [ ] **Passo 6: commit**

```bash
git add -A && git commit -m "feat(core): ações seguras com colisão, cópia entre discos e desfazer"
```

---

### Tarefa 7: `journal` (SQLite)

**Arquivos:**
- Criar: `crates/core/src/journal.rs`
- Modificar: `crates/core/src/lib.rs` (`pub mod journal;`)

**Interfaces:**
- Consome: `actions::{Done, Kind}`, `crate::Result`, `crate::xdg_dir`
- Produz:
  ```rust
  #[derive(Debug, Clone)] pub struct Entry { pub id: i64, pub ts: i64, pub rule: String, pub kind: Kind,
      pub src: PathBuf, pub dst: Option<PathBuf>, pub undone: bool, pub error: Option<String> }
  pub struct Journal { /* conn */ }
  impl Journal {
      pub fn open(path: &Path) -> Result<Journal>;           // cria as pastas, WAL, busy_timeout 5s, schema
      pub fn record(&self, rule: &str, done: &Done) -> Result<i64>;
      pub fn record_error(&self, rule: &str, kind: Kind, src: &Path, error: &str) -> Result<i64>;
      pub fn get(&self, id: i64) -> Result<Option<Entry>>;
      pub fn last_undoable(&self) -> Result<Option<Entry>>;  // undone = 0, error NULL, maior id
      pub fn recent(&self, limit: usize) -> Result<Vec<Entry>>; // id decrescente
      pub fn mark_undone(&self, id: i64) -> Result<()>;
      pub fn is_recent_dst(&self, path: &Path, within_secs: i64) -> Result<bool>;
  }
  pub fn default_path() -> PathBuf
  ```
  `ts` é em segundos Unix. Os caminhos ficam em BLOB (bytes crus), porque nomes que não são UTF-8 precisam sobreviver ao undo.

- [ ] **Passo 1: dependência**

Run: `cargo add -p tidyflow-core rusqlite --features bundled`

- [ ] **Passo 2: escrever os testes que falham**

`crates/core/src/journal.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};

    fn open() -> (tempfile::TempDir, Journal) {
        let d = tempfile::tempdir().unwrap();
        let j = Journal::open(&d.path().join("sub/journal.db")).unwrap();
        (d, j)
    }

    fn done(src: &str, dst: &str) -> Done {
        Done { kind: Kind::Move, src: src.into(), dst: Some(dst.into()) }
    }

    #[test]
    fn record_and_read_back() {
        let (_d, j) = open();
        let id = j.record("pdfs", &done("/a/x.pdf", "/b/x.pdf")).unwrap();
        let e = j.get(id).unwrap().unwrap();
        assert_eq!(e.rule, "pdfs");
        assert_eq!(e.kind, Kind::Move);
        assert_eq!(e.src, PathBuf::from("/a/x.pdf"));
        assert_eq!(e.dst, Some(PathBuf::from("/b/x.pdf")));
        assert!(!e.undone && e.error.is_none());
        assert!(j.get(999).unwrap().is_none());
    }

    #[test]
    fn non_utf8_paths_survive() {
        let (_d, j) = open();
        let weird = PathBuf::from(OsStr::from_bytes(b"/a/\xff.bin"));
        let id = j
            .record("r", &Done { kind: Kind::Trash, src: weird.clone(), dst: None })
            .unwrap();
        let e = j.get(id).unwrap().unwrap();
        assert_eq!(e.src, weird);
        assert_eq!(e.dst, None);
    }

    #[test]
    fn last_undoable_skips_undone_and_errors() {
        let (_d, j) = open();
        let a = j.record("r", &done("/a", "/b")).unwrap();
        let b = j.record("r", &done("/c", "/d")).unwrap();
        j.record_error("r", Kind::Move, Path::new("/e"), "sem permissão").unwrap();
        assert_eq!(j.last_undoable().unwrap().unwrap().id, b);
        j.mark_undone(b).unwrap();
        assert!(j.get(b).unwrap().unwrap().undone);
        assert_eq!(j.last_undoable().unwrap().unwrap().id, a);
    }

    #[test]
    fn recent_is_newest_first() {
        let (_d, j) = open();
        for i in 0..5 {
            j.record("r", &done(&format!("/s{i}"), "/d")).unwrap();
        }
        let ids: Vec<i64> = j.recent(3).unwrap().iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![5, 4, 3]);
    }

    #[test]
    fn recent_dst() {
        let (_d, j) = open();
        j.record("r", &done("/a", "/b/x")).unwrap();
        assert!(j.is_recent_dst(Path::new("/b/x"), 5).unwrap());
        assert!(!j.is_recent_dst(Path::new("/b/y"), 5).unwrap());
    }

    #[test]
    fn two_connections_share_state() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("j.db");
        let a = Journal::open(&path).unwrap();
        let b = Journal::open(&path).unwrap();
        let id = a.record("r", &done("/a", "/b")).unwrap();
        b.mark_undone(id).unwrap();
        assert!(a.get(id).unwrap().unwrap().undone);
    }
}
```

- [ ] **Passo 3: rodar e confirmar que falha**

Run: `cargo test -p tidyflow-core journal`
Esperado: erro de compilação.

- [ ] **Passo 4: implementar**

```rust
use crate::{
    Result,
    actions::{Done, Kind},
};
use rusqlite::{Connection, OptionalExtension, Row, params, types::Type};
use std::{
    ffi::OsString,
    fs,
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS actions (
    id     INTEGER PRIMARY KEY,
    ts     INTEGER NOT NULL,
    rule   TEXT    NOT NULL,
    kind   TEXT    NOT NULL,
    src    BLOB    NOT NULL,
    dst    BLOB,
    undone INTEGER NOT NULL DEFAULT 0,
    error  TEXT
);
CREATE INDEX IF NOT EXISTS actions_dst ON actions(dst);
";

const COLS: &str = "id, ts, rule, kind, src, dst, undone, error";

#[derive(Debug, Clone)]
pub struct Entry {
    pub id: i64,
    pub ts: i64,
    pub rule: String,
    pub kind: Kind,
    pub src: PathBuf,
    pub dst: Option<PathBuf>,
    pub undone: bool,
    pub error: Option<String>,
}

pub struct Journal {
    conn: Connection,
}

pub fn default_path() -> PathBuf {
    crate::xdg_dir("XDG_DATA_HOME", ".local/share").join("tidyflow/journal.db")
}

impl Journal {
    pub fn open(path: &Path) -> Result<Journal> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update_and_check(None, "journal_mode", "WAL", |_| Ok(()))?;
        conn.execute_batch(SCHEMA)?;
        Ok(Journal { conn })
    }

    pub fn record(&self, rule: &str, done: &Done) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO actions (ts, rule, kind, src, dst) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![now(), rule, done.kind.as_str(), bytes(&done.src), done.dst.as_deref().map(bytes)],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn record_error(&self, rule: &str, kind: Kind, src: &Path, error: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO actions (ts, rule, kind, src, error) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![now(), rule, kind.as_str(), bytes(src), error],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get(&self, id: i64) -> Result<Option<Entry>> {
        let sql = format!("SELECT {COLS} FROM actions WHERE id = ?1");
        Ok(self.conn.query_row(&sql, [id], entry).optional()?)
    }

    pub fn last_undoable(&self) -> Result<Option<Entry>> {
        let sql = format!(
            "SELECT {COLS} FROM actions WHERE undone = 0 AND error IS NULL ORDER BY id DESC LIMIT 1"
        );
        Ok(self.conn.query_row(&sql, [], entry).optional()?)
    }

    pub fn recent(&self, limit: usize) -> Result<Vec<Entry>> {
        let sql = format!("SELECT {COLS} FROM actions ORDER BY id DESC LIMIT ?1");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([limit as i64], entry)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn mark_undone(&self, id: i64) -> Result<()> {
        self.conn.execute("UPDATE actions SET undone = 1 WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn is_recent_dst(&self, path: &Path, within_secs: i64) -> Result<bool> {
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM actions WHERE dst = ?1 AND ts >= ?2)",
            params![bytes(path), now() - within_secs],
            |r| r.get(0),
        )?)
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn bytes(p: &Path) -> &[u8] {
    p.as_os_str().as_bytes()
}

fn path(b: Vec<u8>) -> PathBuf {
    PathBuf::from(OsString::from_vec(b))
}

fn entry(r: &Row) -> rusqlite::Result<Entry> {
    let kind: String = r.get(3)?;
    Ok(Entry {
        id: r.get(0)?,
        ts: r.get(1)?,
        rule: r.get(2)?,
        kind: Kind::parse(&kind).ok_or(rusqlite::Error::InvalidColumnType(3, kind, Type::Text))?,
        src: path(r.get(4)?),
        dst: r.get::<_, Option<Vec<u8>>>(5)?.map(path),
        undone: r.get(6)?,
        error: r.get(7)?,
    })
}
```

- [ ] **Passo 5: rodar e confirmar que passa**

Run: `cargo test -p tidyflow-core journal && cargo clippy --all-targets -- -D warnings`
Esperado: 6 testes passando.

- [ ] **Passo 6: commit**

```bash
git add -A && git commit -m "feat(core): journal SQLite"
```

---

### Tarefa 8: `stability`

**Arquivos:**
- Criar: `crates/core/src/stability.rs`
- Modificar: `crates/core/src/lib.rs` (`pub mod stability;`)

**Interfaces:**
- Produz:
  ```rust
  pub fn is_ignored_name(name: &str) -> bool  // ocultos (.x) e .part/.crdownload/.tmp
  pub fn wait_stable(path: &Path, quiet: Duration, max_wait: Duration) -> bool
  // true = tamanho e mtime iguais durante `quiet`; false = arquivo sumiu ou não estabilizou em `max_wait`
  ```

- [ ] **Passo 1: escrever os testes que falham**

`crates/core/src/stability.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, io::Write, thread};

    #[test]
    fn ignored_names() {
        for n in [".hidden", "a.part", "a.pdf.crdownload", "x.tmp"] {
            assert!(is_ignored_name(n), "{n}");
        }
        for n in ["a.pdf", "partitura.pdf", "tmp.txt"] {
            assert!(!is_ignored_name(n), "{n}");
        }
    }

    #[test]
    fn stable_file() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a");
        fs::write(&p, "x").unwrap();
        assert!(wait_stable(&p, Duration::from_millis(50), Duration::from_secs(1)));
    }

    #[test]
    fn missing_file() {
        assert!(!wait_stable(Path::new("/nao/existe"), Duration::from_millis(10), Duration::from_secs(1)));
    }

    #[test]
    fn growing_file_times_out() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a");
        let mut f = fs::File::create(&p).unwrap();
        let writer = thread::spawn(move || {
            for _ in 0..30 {
                f.write_all(b"x").unwrap();
                f.flush().unwrap();
                thread::sleep(Duration::from_millis(20));
            }
        });
        assert!(!wait_stable(&p, Duration::from_millis(100), Duration::from_millis(300)));
        writer.join().unwrap();
    }
}
```

- [ ] **Passo 2: rodar e confirmar que falha**

Run: `cargo test -p tidyflow-core stability`
Esperado: erro de compilação.

- [ ] **Passo 3: implementar**

```rust
use std::{
    fs,
    path::Path,
    thread,
    time::{Duration, Instant, SystemTime},
};

const TEMP_SUFFIXES: [&str; 3] = [".part", ".crdownload", ".tmp"];

pub fn is_ignored_name(name: &str) -> bool {
    name.starts_with('.') || TEMP_SUFFIXES.iter().any(|s| name.ends_with(s))
}

fn snapshot(path: &Path) -> Option<(u64, SystemTime)> {
    let m = fs::metadata(path).ok()?;
    Some((m.len(), m.modified().ok()?))
}

pub fn wait_stable(path: &Path, quiet: Duration, max_wait: Duration) -> bool {
    let start = Instant::now();
    let Some(mut last) = snapshot(path) else {
        return false;
    };
    loop {
        thread::sleep(quiet);
        let Some(now) = snapshot(path) else {
            return false;
        };
        if now == last {
            return true;
        }
        if start.elapsed() >= max_wait {
            return false;
        }
        last = now;
    }
}
```

- [ ] **Passo 4: rodar e confirmar que passa**

Run: `cargo test -p tidyflow-core stability && cargo clippy --all-targets -- -D warnings`
Esperado: 4 testes passando.

- [ ] **Passo 5: commit**

```bash
git add -A && git commit -m "feat(core): detecção de download em andamento"
```

---

### Tarefa 9: `engine` (pipeline)

**Arquivos:**
- Criar: `crates/core/src/engine.rs`
- Modificar: `crates/core/src/lib.rs` (`pub mod engine;`)

**Interfaces:**
- Consome: tudo das tarefas 2 a 8
- Produz:
  ```rust
  #[derive(Debug)] pub enum Report {
      NoMatch,
      Planned { rule: String, kind: Kind, dst: Option<PathBuf> },  // dry-run
      Done { id: i64, rule: String, done: actions::Done },
      Failed { rule: String, error: String },                     // erro também gravado no journal
  }
  pub fn process(cfg: &Config, journal: &Journal, path: &Path, dry_run: bool, now: SystemTime) -> Result<Report>
  pub fn scan(cfg: &Config, journal: &Journal, dry_run: bool, now: SystemTime) -> Vec<(PathBuf, Result<Report>)>
  pub fn undo(journal: &Journal, id: Option<i64>) -> Result<Entry> // None = última ação desfazível
  ```
  `process` devolve `Err` quando não consegue ler o arquivo (por exemplo, se ele sumiu). Falhas da ação viram `Ok(Report::Failed)`.

- [ ] **Passo 1: escrever os testes que falham**

`crates/core/src/engine.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;
    use std::fs;

    struct Env {
        _d: tempfile::TempDir,
        watch: PathBuf,
        out: PathBuf,
        cfg: Config,
        journal: Journal,
    }

    fn env() -> Env {
        let d = tempfile::tempdir().unwrap();
        let watch = d.path().join("in");
        let out = d.path().join("out");
        fs::create_dir(&watch).unwrap();
        let cfg = config::parse(&format!(
            "[[rule]]\nname = \"pdfs\"\nwatch = [\"{}\"]\nwhen = {{ ext = [\"pdf\"] }}\naction = {{ move = \"{}\" }}\n",
            watch.display(),
            out.display()
        ))
        .unwrap();
        let journal = Journal::open(&d.path().join("j.db")).unwrap();
        Env { _d: d, watch, out, cfg, journal }
    }

    #[test]
    fn process_moves_and_records() {
        let e = env();
        let p = e.watch.join("a.pdf");
        fs::write(&p, "x").unwrap();
        let Report::Done { id, rule, done } = process(&e.cfg, &e.journal, &p, false, SystemTime::now()).unwrap() else {
            panic!("esperava Done");
        };
        assert_eq!(rule, "pdfs");
        assert_eq!(done.dst.as_deref(), Some(e.out.join("a.pdf").as_path()));
        assert_eq!(e.journal.get(id).unwrap().unwrap().src, p);
    }

    #[test]
    fn dry_run_changes_nothing() {
        let e = env();
        let p = e.watch.join("a.pdf");
        fs::write(&p, "x").unwrap();
        let r = process(&e.cfg, &e.journal, &p, true, SystemTime::now()).unwrap();
        assert!(matches!(r, Report::Planned { kind: Kind::Move, .. }));
        assert!(p.exists());
        assert!(e.journal.recent(10).unwrap().is_empty());
    }

    #[test]
    fn no_match() {
        let e = env();
        let p = e.watch.join("a.txt");
        fs::write(&p, "x").unwrap();
        assert!(matches!(process(&e.cfg, &e.journal, &p, false, SystemTime::now()).unwrap(), Report::NoMatch));
    }

    #[test]
    fn failure_is_reported_and_recorded() {
        let e = env();
        fs::write(&e.out, "sou um arquivo, não uma pasta").unwrap();
        let p = e.watch.join("a.pdf");
        fs::write(&p, "x").unwrap();
        let r = process(&e.cfg, &e.journal, &p, false, SystemTime::now()).unwrap();
        assert!(matches!(r, Report::Failed { .. }), "{r:?}");
        assert!(p.exists());
        assert!(e.journal.recent(1).unwrap()[0].error.is_some());
    }

    #[test]
    fn scan_skips_ignored_and_dirs() {
        let e = env();
        fs::write(e.watch.join("a.pdf"), "x").unwrap();
        fs::write(e.watch.join("b.pdf.part"), "x").unwrap();
        fs::write(e.watch.join(".c.pdf"), "x").unwrap();
        fs::create_dir(e.watch.join("d.pdf")).unwrap();
        let results = scan(&e.cfg, &e.journal, false, SystemTime::now());
        assert_eq!(results.len(), 1);
        assert!(e.out.join("a.pdf").exists());
    }

    #[test]
    fn undo_last_and_twice() {
        let e = env();
        let p = e.watch.join("a.pdf");
        fs::write(&p, "x").unwrap();
        process(&e.cfg, &e.journal, &p, false, SystemTime::now()).unwrap();
        let entry = undo(&e.journal, None).unwrap();
        assert!(p.exists());
        assert!(undo(&e.journal, Some(entry.id)).unwrap_err().to_string().contains("já foi desfeita"));
        assert!(undo(&e.journal, None).unwrap_err().to_string().contains("nada para desfazer"));
        assert!(undo(&e.journal, Some(42)).unwrap_err().to_string().contains("não existe"));
    }
}
```

- [ ] **Passo 2: rodar e confirmar que falha**

Run: `cargo test -p tidyflow-core engine`
Esperado: erro de compilação.

- [ ] **Passo 3: implementar**

```rust
use crate::{
    Result,
    actions::{self, Kind},
    config::Config,
    facts::Facts,
    journal::{Entry, Journal},
    rules, stability,
};
use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Debug)]
pub enum Report {
    NoMatch,
    Planned { rule: String, kind: Kind, dst: Option<PathBuf> },
    Done { id: i64, rule: String, done: actions::Done },
    Failed { rule: String, error: String },
}

pub fn process(cfg: &Config, journal: &Journal, path: &Path, dry_run: bool, now: SystemTime) -> Result<Report> {
    let facts = Facts::read(path)?;
    let Some(rule) = rules::first_match(&cfg.rules, &facts, now) else {
        return Ok(Report::NoMatch);
    };
    let kind = actions::kind_of(&rule.action);
    if dry_run {
        return Ok(Report::Planned {
            rule: rule.name.clone(),
            kind,
            dst: actions::destination(&rule.action, &facts),
        });
    }
    match actions::execute(&rule.action, &facts) {
        Ok(Some(done)) => {
            let id = journal.record(&rule.name, &done)?;
            Ok(Report::Done { id, rule: rule.name.clone(), done })
        }
        Ok(None) => Ok(Report::NoMatch),
        Err(e) => {
            let error = e.to_string();
            journal.record_error(&rule.name, kind, path, &error)?;
            Ok(Report::Failed { rule: rule.name.clone(), error })
        }
    }
}

pub fn scan(cfg: &Config, journal: &Journal, dry_run: bool, now: SystemTime) -> Vec<(PathBuf, Result<Report>)> {
    let mut dirs: Vec<&PathBuf> = cfg.rules.iter().flat_map(|r| &r.watch).collect();
    dirs.sort();
    dirs.dedup();
    let mut out = Vec::new();
    for dir in dirs {
        let Ok(entries) = fs::read_dir(dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() || stability::is_ignored_name(&entry.file_name().to_string_lossy()) {
                continue;
            }
            let report = process(cfg, journal, &path, dry_run, now);
            out.push((path, report));
        }
    }
    out
}

pub fn undo(journal: &Journal, id: Option<i64>) -> Result<Entry> {
    let entry = match id {
        Some(id) => journal.get(id)?.ok_or(format!("ação #{id} não existe"))?,
        None => journal.last_undoable()?.ok_or("nada para desfazer")?,
    };
    if entry.undone {
        return Err(format!("ação #{} já foi desfeita", entry.id).into());
    }
    if entry.error.is_some() {
        return Err(format!("ação #{} falhou, não há o que desfazer", entry.id).into());
    }
    actions::undo(entry.kind, &entry.src, entry.dst.as_deref())?;
    journal.mark_undone(entry.id)?;
    Ok(entry)
}
```

- [ ] **Passo 4: rodar e confirmar que passa**

Run: `cargo test -p tidyflow-core && cargo clippy --all-targets -- -D warnings && cargo fmt --all --check`
Esperado: todos os testes do core passando.

- [ ] **Passo 5: commit**

```bash
git add -A && git commit -m "feat(core): engine (process, scan, undo)"
```

---

### Tarefa 10: CLI (`run`, `undo`, `history`, `check`)

**Arquivos:**
- Criar: `crates/cli/Cargo.toml`, `crates/cli/src/main.rs`, `crates/cli/tests/cli.rs`
- Modificar: `Cargo.toml` (members)

**Interfaces:**
- Consome: `tidyflow_core::{config, engine, journal, facts, rules, actions}`
- Produz: o binário `tidyflow` com as opções `--config <PATH>` (global) e `run [--dry-run]`, `undo [ID]`, `history [-n N]`, `check [ARQUIVO]`. O journal segue `XDG_DATA_HOME`, que os testes usam para se isolar. O `daemon` entra na Tarefa 11.

- [ ] **Passo 1: criar o crate**

`Cargo.toml` (raiz): `members = ["crates/core", "crates/cli"]`

`crates/cli/Cargo.toml`:
```toml
[package]
name = "tidyflow"
version = "0.1.0"
edition = "2024"

[dependencies]
tidyflow-core = { path = "../core" }
```

Run: `cargo add -p tidyflow clap --features derive && cargo add -p tidyflow chrono tracing && cargo add -p tidyflow tracing-subscriber --features env-filter && cargo add -p tidyflow --dev tempfile`

- [ ] **Passo 2: escrever os testes que falham**

`crates/cli/tests/cli.rs`:
```rust
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};

struct Env {
    d: tempfile::TempDir,
    watch: PathBuf,
    out: PathBuf,
    cfg: PathBuf,
}

fn env() -> Env {
    let d = tempfile::tempdir().unwrap();
    let watch = d.path().join("in");
    let out = d.path().join("out");
    fs::create_dir(&watch).unwrap();
    let cfg = d.path().join("config.toml");
    fs::write(
        &cfg,
        format!(
            "[settings]\nnotify = false\n\n[[rule]]\nname = \"pdfs\"\nwatch = [\"{}\"]\nwhen = {{ ext = [\"pdf\"] }}\naction = {{ move = \"{}\" }}\n",
            watch.display(),
            out.display()
        ),
    )
    .unwrap();
    Env { d, watch, out, cfg }
}

fn cmd(e: &Env) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_tidyflow"));
    c.env("XDG_DATA_HOME", e.d.path()).arg("--config").arg(&e.cfg);
    c
}

fn run(e: &Env, args: &[&str]) -> Output {
    cmd(e).args(args).output().unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn run_then_undo() {
    let e = env();
    fs::write(e.watch.join("a.pdf"), "x").unwrap();
    let o = run(&e, &["run"]);
    assert!(o.status.success(), "{o:?}");
    assert!(e.out.join("a.pdf").exists());

    let o = run(&e, &["history"]);
    assert!(stdout(&o).contains("a.pdf"), "{}", stdout(&o));

    let o = run(&e, &["undo"]);
    assert!(o.status.success(), "{o:?}");
    assert!(e.watch.join("a.pdf").exists() && !e.out.join("a.pdf").exists());
}

#[test]
fn dry_run_changes_nothing() {
    let e = env();
    fs::write(e.watch.join("a.pdf"), "x").unwrap();
    let o = run(&e, &["run", "--dry-run"]);
    assert!(stdout(&o).contains("a.pdf"));
    assert!(e.watch.join("a.pdf").exists());
}

#[test]
fn check_shows_matching_rule() {
    let e = env();
    let f = e.watch.join("a.pdf");
    fs::write(&f, "x").unwrap();
    let o = run(&e, &["check", f.to_str().unwrap()]);
    assert!(o.status.success());
    assert!(stdout(&o).contains("pdfs"), "{}", stdout(&o));
}

#[test]
fn check_reports_bad_config() {
    let e = env();
    fs::write(&e.cfg, "[settings]\nnotify = \n").unwrap();
    let o = run(&e, &["check"]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("linha 2"));
}
```

- [ ] **Passo 3: rodar e confirmar que falha**

Run: `cargo test -p tidyflow`
Esperado: erro de compilação (o `main.rs` não existe).

- [ ] **Passo 4: implementar**

`crates/cli/src/main.rs`:
```rust
use clap::{Parser, Subcommand};
use std::{
    path::{Path, PathBuf},
    process::ExitCode,
    time::SystemTime,
};
use tidyflow_core::{
    actions, config,
    engine::{self, Report},
    facts::Facts,
    journal::{self, Entry, Journal},
    rules,
};

#[derive(Parser)]
#[command(name = "tidyflow", version, about = "Organiza arquivos automaticamente por regras")]
struct Cli {
    /// Caminho do config (padrão: ~/.config/tidyflow/config.toml)
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Varre as pastas monitoradas uma vez
    Run {
        /// Só mostra o que seria feito
        #[arg(long)]
        dry_run: bool,
    },
    /// Desfaz a última ação, ou a ação <ID>
    Undo { id: Option<i64> },
    /// Mostra as últimas ações
    History {
        #[arg(short = 'n', long, default_value_t = 20)]
        limit: usize,
    },
    /// Valida o config; com <ARQUIVO>, mostra qual regra casaria
    Check { file: Option<PathBuf> },
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let cli = Cli::parse();
    let config_path = cli.config.unwrap_or_else(config::default_path);
    match run(cli.cmd, &config_path) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("erro: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(cmd: Cmd, config_path: &Path) -> tidyflow_core::Result<()> {
    match cmd {
        Cmd::Run { dry_run } => {
            let cfg = config::load(config_path)?;
            let journal = Journal::open(&journal::default_path())?;
            for (path, report) in engine::scan(&cfg, &journal, dry_run, SystemTime::now()) {
                match report {
                    Ok(Report::NoMatch) => {}
                    Ok(Report::Planned { rule, kind, dst }) => {
                        println!("[{rule}] {} {} → {}", kind.as_str(), path.display(), target(dst.as_deref()))
                    }
                    Ok(Report::Done { id, rule, done }) => println!(
                        "#{id} [{rule}] {} {} → {}",
                        done.kind.as_str(),
                        path.display(),
                        target(done.dst.as_deref())
                    ),
                    Ok(Report::Failed { rule, error }) => {
                        eprintln!("[{rule}] falhou em {}: {error}", path.display())
                    }
                    Err(e) => eprintln!("{}: {e}", path.display()),
                }
            }
        }
        Cmd::Undo { id } => {
            let journal = Journal::open(&journal::default_path())?;
            let e = engine::undo(&journal, id)?;
            println!("desfeito #{}: {}", e.id, e.src.display());
        }
        Cmd::History { limit } => {
            let journal = Journal::open(&journal::default_path())?;
            for e in journal.recent(limit)? {
                println!("{}", format_entry(&e));
            }
        }
        Cmd::Check { file } => {
            let cfg = config::load(config_path)?;
            println!("config ok: {} regra(s)", cfg.rules.len());
            if let Some(file) = file {
                let f = Facts::read(&file)?;
                println!("mime: {}\nextensão: {}\ntamanho: {} bytes", f.mime, f.ext, f.size);
                println!("origem: {}", f.origin_url.as_deref().unwrap_or("(nenhuma)"));
                match rules::first_match(&cfg.rules, &f, SystemTime::now()) {
                    Some(r) => println!(
                        "regra: {} → {}",
                        r.name,
                        target(actions::destination(&r.action, &f).as_deref())
                    ),
                    None => println!("nenhuma regra casa com este arquivo"),
                }
            }
        }
    }
    Ok(())
}

fn target(dst: Option<&Path>) -> String {
    dst.map_or("lixeira".into(), |d| d.display().to_string())
}

fn format_entry(e: &Entry) -> String {
    let when = chrono::DateTime::from_timestamp(e.ts, 0)
        .map(|t| t.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default();
    let status = match (&e.error, e.undone) {
        (Some(err), _) => format!("  (erro: {err})"),
        (None, true) => "  (desfeito)".into(),
        (None, false) => String::new(),
    };
    format!(
        "#{} {when} [{}] {} {} → {}{status}",
        e.id,
        e.rule,
        e.kind.as_str(),
        e.src.display(),
        target(e.dst.as_deref())
    )
}
```

- [ ] **Passo 5: rodar e confirmar que passa**

Run: `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --all --check`
Esperado: todos passando, incluindo os 4 testes do `cli.rs`.

- [ ] **Passo 6: teste manual rápido**

Run: `cargo run -q -p tidyflow -- --help`
Esperado: lista com run, undo, history e check em português.

- [ ] **Passo 7: commit**

```bash
git add -A && git commit -m "feat(cli): comandos run, undo, history e check"
```

---

### Tarefa 11: daemon e notificações

**Arquivos:**
- Criar: `crates/cli/src/daemon.rs`, `crates/cli/src/desktop.rs`
- Modificar: `crates/cli/src/main.rs` (subcomando `Daemon`), `crates/cli/tests/cli.rs` (teste de ponta a ponta)

**Interfaces:**
- Consome: `config::load`, `engine::{process, scan, undo, Report}`, `journal::Journal`, `stability::{is_ignored_name, wait_stable}`
- Produz: `tidyflow daemon` e as funções `desktop::organized(journal_path: PathBuf, id: i64, body: String)` e `desktop::error(body: &str)`

- [ ] **Passo 1: dependências**

Run: `cargo add -p tidyflow notify notify-rust`

- [ ] **Passo 2: escrever o teste de ponta a ponta que falha** (acrescentar ao fim de `crates/cli/tests/cli.rs`)

```rust
#[test]
fn daemon_organizes_new_and_existing_files() {
    use std::{
        thread,
        time::{Duration, Instant},
    };
    let e = env();
    fs::write(e.watch.join("antes.pdf"), "x").unwrap(); // varredura inicial
    let mut child = cmd(&e).arg("daemon").spawn().unwrap();
    thread::sleep(Duration::from_millis(500));
    fs::write(e.watch.join("depois.pdf"), "x").unwrap(); // evento inotify

    let wanted = [e.out.join("antes.pdf"), e.out.join("depois.pdf")];
    let deadline = Instant::now() + Duration::from_secs(10);
    while !wanted.iter().all(|p| p.exists()) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(100));
    }
    child.kill().unwrap();
    child.wait().unwrap();
    for p in &wanted {
        assert!(p.exists(), "{} não chegou", p.display());
    }
}
```

Run: `cargo test -p tidyflow daemon_`
Esperado: FAIL (o clap não reconhece o subcomando `daemon`).

- [ ] **Passo 3: notificações**

`crates/cli/src/desktop.rs`:
```rust
use notify_rust::Notification;
use std::path::PathBuf;
use tidyflow_core::{engine, journal::Journal};
use tracing::warn;

// ponytail: uma thread por notificação, bloqueada até ela fechar. Trocar por um
// único loop D-Bus se muitas notificações simultâneas virarem problema.
pub fn organized(journal_path: PathBuf, id: i64, body: String) {
    std::thread::spawn(move || {
        let handle = match Notification::new()
            .appname("TidyFlow")
            .summary("TidyFlow")
            .body(&body)
            .action("undo", "Desfazer")
            .show()
        {
            Ok(h) => h,
            Err(e) => return warn!("notificação falhou: {e}"),
        };
        handle.wait_for_action(|action| {
            if action != "undo" {
                return;
            }
            // Cada thread abre a sua conexão; o WAL resolve a concorrência com o daemon.
            if let Err(e) = Journal::open(&journal_path).and_then(|j| engine::undo(&j, Some(id))) {
                error(&format!("Não foi possível desfazer: {e}"));
            }
        });
    });
}

pub fn error(body: &str) {
    if let Err(e) = Notification::new()
        .appname("TidyFlow")
        .summary("TidyFlow: erro")
        .body(body)
        .show()
    {
        warn!("notificação falhou: {e}");
    }
}
```

- [ ] **Passo 4: daemon**

`crates/cli/src/daemon.rs`:
```rust
use crate::desktop;
use notify::{
    Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
    event::{AccessKind, AccessMode, ModifyKind, RenameMode},
};
use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, RecvTimeoutError},
    time::{Duration, Instant, SystemTime},
};
use tidyflow_core::{
    actions::Done,
    config::{self, Config},
    engine::{self, Report},
    journal::Journal,
    stability,
};
use tracing::{debug, error, info, warn};

const RETRY: Duration = Duration::from_secs(60);
const QUIET: Duration = Duration::from_secs(2);
const MAX_WAIT: Duration = Duration::from_secs(60);
const LOOP_GUARD_SECS: i64 = 5;

struct Daemon {
    config_path: PathBuf,
    journal_path: PathBuf,
    cfg: Config,
    journal: Journal,
    watcher: RecommendedWatcher,
    watched: Vec<PathBuf>,
    missing: Vec<PathBuf>,
}

pub fn run(config_path: &Path, journal_path: &Path) -> tidyflow_core::Result<()> {
    let config_path = config_path.canonicalize()?;
    let cfg = config::load(&config_path)?;
    let journal = Journal::open(journal_path)?;
    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(tx)?;
    // Observa a pasta, não o arquivo: editores salvam via rename e trocam o inode.
    let config_dir = config_path.parent().ok_or("caminho de config inválido")?;
    watcher.watch(config_dir, RecursiveMode::NonRecursive)?;

    let mut d = Daemon {
        config_path,
        journal_path: journal_path.to_path_buf(),
        cfg,
        journal,
        watcher,
        watched: Vec::new(),
        missing: Vec::new(),
    };
    d.rewatch();
    d.scan();
    info!("daemon iniciado com {} regra(s)", d.cfg.rules.len());

    let mut next_scan = Instant::now() + d.cfg.age_scan_interval;
    let mut next_retry = Instant::now() + RETRY;
    loop {
        let deadline = if d.missing.is_empty() { next_scan } else { next_scan.min(next_retry) };
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(Ok(event)) => d.on_event(event),
            Ok(Err(e)) => warn!("erro do watcher: {e}"),
            Err(RecvTimeoutError::Timeout) => {
                let now = Instant::now();
                if !d.missing.is_empty() && now >= next_retry {
                    d.rewatch();
                    next_retry = now + RETRY;
                }
                if now >= next_scan {
                    d.scan();
                    next_scan = now + d.cfg.age_scan_interval;
                }
            }
            Err(RecvTimeoutError::Disconnected) => return Err("watcher encerrado".into()),
        }
    }
}

impl Daemon {
    fn rewatch(&mut self) {
        for dir in self.watched.drain(..) {
            let _ = self.watcher.unwatch(&dir);
        }
        let mut dirs: Vec<PathBuf> = self.cfg.rules.iter().flat_map(|r| r.watch.clone()).collect();
        dirs.sort();
        dirs.dedup();
        let was_missing = std::mem::take(&mut self.missing);
        for dir in dirs {
            match self.watcher.watch(&dir, RecursiveMode::NonRecursive) {
                Ok(()) => self.watched.push(dir),
                Err(e) => {
                    warn!("não foi possível monitorar {}: {e}", dir.display());
                    if !was_missing.contains(&dir) {
                        self.alert(&format!("Não foi possível monitorar {}", dir.display()));
                    }
                    self.missing.push(dir);
                }
            }
        }
    }

    fn on_event(&mut self, event: Event) {
        if matches!(event.kind, EventKind::Remove(_)) {
            for p in &event.paths {
                if let Some(i) = self.watched.iter().position(|w| w == p) {
                    let dir = self.watched.remove(i);
                    warn!("pasta monitorada removida: {}", dir.display());
                    self.alert(&format!("Pasta monitorada removida: {}", dir.display()));
                    self.missing.push(dir);
                }
            }
            return;
        }
        let path = match event.kind {
            EventKind::Access(AccessKind::Close(AccessMode::Write))
            | EventKind::Modify(ModifyKind::Name(RenameMode::To)) => event.paths.first(),
            _ => None,
        };
        let Some(path) = path else { return };
        if *path == self.config_path {
            self.reload();
        } else {
            self.handle(path);
        }
    }

    fn handle(&self, path: &Path) {
        let in_watched = path.parent().is_some_and(|p| self.watched.iter().any(|w| w == p));
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if !in_watched || stability::is_ignored_name(&name) {
            return;
        }
        match self.journal.is_recent_dst(path, LOOP_GUARD_SECS) {
            Ok(true) => return debug!("ignorando evento gerado por nós: {}", path.display()),
            Ok(false) => {}
            Err(e) => warn!("journal: {e}"),
        }
        // ponytail: espera em série; downloads simultâneos somam 2s cada.
        // Mover para uma fila com uma thread por arquivo se isso incomodar.
        if !stability::wait_stable(path, QUIET, MAX_WAIT) {
            return debug!("{} sumiu ou não estabilizou", path.display());
        }
        let result = engine::process(&self.cfg, &self.journal, path, false, SystemTime::now());
        self.report(path, result);
    }

    fn scan(&self) {
        for (path, result) in engine::scan(&self.cfg, &self.journal, false, SystemTime::now()) {
            self.report(&path, result);
        }
    }

    fn report(&self, path: &Path, result: tidyflow_core::Result<Report>) {
        match result {
            Ok(Report::Done { id, rule, done }) => {
                info!("#{id} [{rule}] {}", describe(&done));
                if self.cfg.notify {
                    desktop::organized(self.journal_path.clone(), id, describe(&done));
                }
            }
            Ok(Report::Failed { rule, error }) => {
                warn!("[{rule}] falhou em {}: {error}", path.display());
                self.alert(&format!("{}: {error}", path.display()));
            }
            Ok(_) => {}
            Err(e) if !path.exists() => debug!("{} sumiu antes da ação: {e}", path.display()),
            Err(e) => warn!("{}: {e}", path.display()),
        }
    }

    fn reload(&mut self) {
        match config::load(&self.config_path) {
            Ok(cfg) => {
                self.cfg = cfg;
                self.rewatch();
                info!("config recarregado: {} regra(s)", self.cfg.rules.len());
            }
            Err(e) => {
                error!("config inválido, mantendo o anterior: {e}");
                self.alert(&format!("Config inválido: {e}"));
            }
        }
    }

    fn alert(&self, msg: &str) {
        if self.cfg.notify {
            desktop::error(msg);
        }
    }
}

fn describe(done: &Done) -> String {
    let name = done.src.file_name().unwrap_or_default().to_string_lossy();
    let place = done
        .dst
        .as_ref()
        .and_then(|d| d.parent())
        .map_or("lixeira".into(), |p| p.display().to_string());
    format!("{name} → {place}")
}
```

- [ ] **Passo 5: ligar o subcomando em `main.rs`**

No topo de `crates/cli/src/main.rs`:
```rust
mod daemon;
mod desktop;
```

No `enum Cmd`, como primeira variante:
```rust
    /// Monitora as pastas e organiza em tempo real
    Daemon,
```

No `match cmd` de `fn run`, como primeiro braço:
```rust
        Cmd::Daemon => daemon::run(config_path, &journal::default_path())?,
```

- [ ] **Passo 6: rodar e confirmar que passa**

Run: `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --all --check`
Esperado: tudo passando. O `daemon_organizes_new_and_existing_files` leva cerca de 3s por causa da espera de estabilidade.

- [ ] **Passo 7: teste manual no desktop**

Crie `~/.config/tidyflow/config.toml` com uma regra para `~/Downloads` (sem `notify = false`), rode `cargo run -q -p tidyflow -- daemon` e baixe um PDF pelo Chromium. Esperado: o arquivo é movido, aparece uma notificação com o botão "Desfazer", e clicar nele devolve o arquivo para `~/Downloads`.

- [ ] **Passo 8: commit**

```bash
git add -A && git commit -m "feat(cli): daemon com watcher, reload, varredura por idade e notificações"
```

---

### Tarefa 12: systemd, exemplo de config e README

**Arquivos:**
- Criar: `contrib/tidyflow.service`, `config.example.toml`, `README.md`

- [ ] **Passo 1: unidade systemd**

`contrib/tidyflow.service`:
```ini
[Unit]
Description=TidyFlow: organizador automático de arquivos

[Service]
ExecStart=%h/.cargo/bin/tidyflow daemon
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
```

- [ ] **Passo 2: config de exemplo**

`config.example.toml`:
```toml
[settings]
age_scan_interval = "1h"
notify = true

# Primeira regra que casar vence: deixe as mais específicas em cima.

[[rule]]
name   = "Faturas Nubank"
watch  = ["~/Downloads"]
when   = { origin = "*.nubank.com.br", mime = "application/pdf" }
action = { move = "~/Documentos/Finanças/{year}/" }

[[rule]]
name   = "Notas fiscais"
watch  = ["~/Downloads", "~/Desktop"]
when   = { ext = ["pdf", "xml"], name_regex = "(?i)nota|nf-?e" }
action = { move = "~/Documentos/NotasFiscais/{year}-{month}/" }

[[rule]]
name   = "Imagens por site"
watch  = ["~/Downloads"]
when   = { mime = "image/*" }
action = { move = "~/Imagens/{origin_host}/" }

[[rule]]
name   = "Instaladores grandes antigos"
watch  = ["~/Downloads"]
when   = { ext = ["iso", "AppImage", "deb"], size_gt = "100MB", older_than = "14d" }
action = { trash = true }

[[rule]]
name   = "Limpeza geral"
watch  = ["~/Downloads"]
when   = { older_than = "30d" }
action = { trash = true }
```

- [ ] **Passo 3: validar o exemplo de verdade**

Run: `mkdir -p ~/Desktop ~/Downloads && cargo run -q -p tidyflow -- --config config.example.toml check`
Esperado: `config ok: 5 regra(s)`.

- [ ] **Passo 4: verificar quais navegadores gravam a origem**

Baixe um arquivo pelo Chromium/Chrome e outro pelo Firefox, e rode `getfattr -d ~/Downloads/<arquivo>` em cada um. Anote no README quais gravam `user.xdg.origin.url`. O Chromium grava por padrão. Para o Firefox é preciso confirmar; se ele não gravar, documente isso como limitação.

- [ ] **Passo 5: README**

`README.md`:
````markdown
# TidyFlow

Organizador automático de arquivos para Linux. Monitora pastas como `~/Downloads`
e move, copia, renomeia ou manda para a lixeira de acordo com regras em TOML.
Todas as ações ficam num histórico e podem ser desfeitas.

**Diferenciais:** regras pela **origem do download** (o site de onde o arquivo
veio), limpeza por idade, MIME detectado pelo conteúdo, nenhuma sobrescrita e
desfazer pela notificação.

## Instalação

```sh
cargo install --path crates/cli
mkdir -p ~/.config/tidyflow
cp config.example.toml ~/.config/tidyflow/config.toml
tidyflow check                       # valida o config
tidyflow run --dry-run               # mostra o que seria feito
cp contrib/tidyflow.service ~/.config/systemd/user/
systemctl --user enable --now tidyflow
journalctl --user -u tidyflow -f     # logs
```

## Comandos

| Comando | O que faz |
|---|---|
| `tidyflow daemon` | monitora e organiza em tempo real |
| `tidyflow run [--dry-run]` | varre uma vez |
| `tidyflow undo [ID]` | desfaz a última ação (ou a ação ID) |
| `tidyflow history [-n N]` | últimas ações |
| `tidyflow check [ARQUIVO]` | valida o config / mostra qual regra casaria |

Hyprland: `bind = SUPER, U, exec, tidyflow undo` para desfazer pelo teclado.

## Regras

Condições (todas precisam casar): `ext`, `name` (glob), `name_regex`, `mime` (glob),
`origin` (glob do host), `size_gt`, `size_lt` (`"100MB"`, `"1MiB"`), `older_than`
(`"30d"`, `"12h"`).

Ações (uma por regra): `move`, `copy`, `rename`, `trash`.

Variáveis: `{year}` `{month}` `{day}` (data de modificação), `{ext}`, `{name}`, `{origin_host}`.

A primeira regra que casar vence. Uma regra `rename` cujo resultado continua casando
com a própria regra renomeia o arquivo de novo a cada varredura: inclua uma
condição que exclua os nomes já renomeados.

## Origem do download

Lida do atributo estendido `user.xdg.origin.url`. Gravado por: Chromium/Chrome,
`wget --xattr`, `curl --xattr` (complete com o resultado do Passo 4).
````

- [ ] **Passo 6: commit**

```bash
git add -A && git commit -m "docs: README, config de exemplo e unidade systemd"
```
