# TidyFlow Fase 2 (interface): plano de implementação

> **Para agentes:** SUB-SKILL OBRIGATÓRIO: use superpowers:subagent-driven-development (recomendado) ou superpowers:executing-plans para implementar este plano tarefa por tarefa. Os passos usam checkbox (`- [ ]`) para acompanhamento.

**Objetivo:** app Tauri 2 + React com as telas Visão geral, Regras, Histórico e Testar arquivo, sobre o `tidyflow-core`; comando `tidyflow organize`; menu de contexto no Nautilus e no Nemo.

**Arquitetura:** sem IPC. O app (`crates/app`, binário `tidyflow-app`) usa o core como biblioteca, lê o journal SQLite e edita o `config.toml` com `toml_edit`; o daemon recarrega o TOML sozinho. O front (`ui/`) só fala com os comandos Tauri e não tem lógica de regras. Toda regra de negócio nova fica no core, com testes.

**Tech Stack:** Rust (edition 2024), toml_edit, tauri 2, tauri-plugin-single-instance, tauri-plugin-dialog, tauri-plugin-opener; React 19 + TypeScript + Vite + Tailwind 4, @dnd-kit/sortable, Vitest + Testing Library + jsdom, ESLint.

**Spec:** `docs/superpowers/specs/2026-11-16-tidyflow-fase2-design.md`
**Design:** `docs/design/tidyflow-ui.png` (fonte `docs/design/tidyflow-ui.figcraft`)

## Restrições globais

- Todas as restrições da Fase 1 continuam (Linux apenas, lixeira, nunca sobrescrever, mensagens em português, executar → verificar → gravar no journal).
- Pré-requisitos do sistema: `rustup` (toolchain stable), `webkit2gtk-4.1`, Node ≥ 22. Sem eles, nada do crate `app` compila.
- Cada commit precisa passar em `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` e, a partir da Tarefa 7, em `npm --prefix ui run lint`, `npm --prefix ui test -- --run` e `npm --prefix ui run build`.
- Dependências entram com `cargo add` / `npm install` (versão resolvida na hora). Se a API de um crate ou pacote mudou em relação a este plano, adapte a chamada e mantenha o comportamento.
- O front nunca monta caminhos nem avalia regras: tudo vem pronto dos comandos.
- Textos da interface em português, iguais aos do design.

## Estrutura de arquivos

```
crates/core/src/units.rs           + format_size, format_duration
crates/core/src/config.rs          + Rule.enabled, nomes únicos
crates/core/src/config/edit.rs     Editor (toml_edit), RuleDraft
crates/core/src/rules.rs           + Check, explain; first_match ignora pausadas; first_match_anywhere
crates/core/src/journal.rs         + migração v1 (size), Filter, page, counts, Periods, stats, last_for_rule, max_id
crates/core/src/actions.rs         + clear_keep
crates/core/src/engine.rs          + organize
crates/cli/src/main.rs             + comando organize; check lista pausadas
crates/cli/src/desktop.rs          notificação com título, "Abrir pasta"
crates/cli/src/daemon.rs           usa a notificação nova
crates/app/Cargo.toml              pacote tidyflow-app
crates/app/build.rs
crates/app/tauri.conf.json
crates/app/capabilities/default.json
crates/app/icons/                  gerados com `tauri icon`
crates/app/src/main.rs             builder, plugins, single instance, --test
crates/app/src/dto.rs              tipos serializáveis para o front
crates/app/src/commands.rs         comandos Tauri (finos sobre o core)
crates/app/src/system.rs           systemctl, contagem de arquivos das pastas
ui/package.json, vite.config.ts, tsconfig.json, eslint.config.js, index.html
ui/src/main.tsx, App.tsx, index.css
ui/src/api.ts                      invoke tipado + tipos espelhando dto.rs
ui/src/hooks/useLive.ts            polling de max_id / mtime do config
ui/src/format.ts                   "há 4 min", "3,2 GB", resumo de regra
ui/src/components/                 Sidebar, Card, Button, Chip, KindBadge, UndoButton, Toast, Dialog
ui/src/screens/Overview.tsx
ui/src/screens/Rules.tsx, RuleList.tsx, RuleEditor.tsx, ConditionRow.tsx
ui/src/screens/History.tsx
ui/src/screens/TestFile.tsx
ui/src/**/*.test.ts(x)
contrib/tidyflow.desktop
contrib/nautilus/TidyFlow/Organizar com TidyFlow
contrib/nautilus/TidyFlow/Testar no TidyFlow
contrib/nemo/tidyflow-organize.nemo_action
contrib/nemo/tidyflow-test.nemo_action
.github/workflows/ci.yml           + webkit, job do front
README.md
```

---

### Tarefa 1: `enabled`, nomes únicos e formatação de unidades

**Arquivos:** Modificar `crates/core/src/config.rs`, `crates/core/src/units.rs`, `crates/core/src/rules.rs`, `crates/cli/src/main.rs`

**Interfaces:**
- Produz: `Rule.enabled: bool`; `units::format_size(u64) -> String`; `units::format_duration(Duration) -> String`

- [ ] **Passo 1: testes que falham**

Em `config.rs`:
```rust
#[test]
fn enabled_defaults_to_true_and_can_be_false() {
    let on = parse(&rule(r#"{ ext = ["pdf"] }"#, "{ trash = true }")).unwrap();
    assert!(on.rules[0].enabled);
    let src = rule(r#"{ ext = ["pdf"] }"#, "{ trash = true }").replace("name = \"r\"\n", "name = \"r\"\nenabled = false\n");
    assert!(!parse(&src).unwrap().rules[0].enabled);
}

#[test]
fn duplicate_rule_names_are_rejected() {
    let r = rule(r#"{ ext = ["pdf"] }"#, "{ trash = true }");
    let err = parse(&format!("{r}\n{r}")).unwrap_err();
    assert!(err.message.contains("nome repetido"), "{err}");
    assert_eq!(err.line, Some(7)); // linha do segundo [[rule]]
}
```

Em `units.rs`:
```rust
#[test]
fn formats() {
    assert_eq!(format_size(999), "999 B");
    assert_eq!(format_size(100_000_000), "100 MB");
    assert_eq!(format_size(3_200_000_000), "3,2 GB");
    assert_eq!(format_size(1_500), "1,5 KB");
    assert_eq!(format_duration(Duration::from_secs(14 * 86_400)), "14 dias");
    assert_eq!(format_duration(Duration::from_secs(86_400)), "1 dia");
    assert_eq!(format_duration(Duration::from_secs(12 * 3600)), "12 h");
    assert_eq!(format_duration(Duration::from_secs(90)), "90 s");
}
```

Em `rules.rs`, um caso em `first_match_wins_and_respects_watch`: uma regra pausada antes da que casa é pulada.

- [ ] **Passo 2: implementar**

- `RawRule` ganha `#[serde(default = "yes")] enabled: bool`; `Rule` ganha `pub enabled: bool`.
- Em `parse`, depois de compilar, um `HashSet` de nomes; no repetido: `ConfigError { line: Some(linha do spanned), message: format!("regra {name:?}: nome repetido (cada regra precisa de um nome único)") }`.
- `first_match` filtra `r.enabled`.
- `format_size`: base 1000, unidades `B KB MB GB TB`, uma casa decimal com vírgula, sem `,0`. `format_duration`: dias se divisível por 86400 (`"1 dia"`/`"N dias"`), senão horas se divisível por 3600 (`"N h"`), senão minutos (`"N min"`), senão `"N s"`.
- `tidyflow check`: depois de "config ok", uma linha `pausada: <nome>` por regra com `enabled = false`.

- [ ] **Passo 3:** `cargo test`, fmt, clippy. **Commit:** `feat(core): regras pausadas e nomes únicos`

---

### Tarefa 2: `rules::explain`

**Arquivos:** Modificar `crates/core/src/rules.rs`

**Interfaces:**
- Produz:
```rust
#[derive(Debug, Clone, PartialEq)]
pub struct Check { pub key: &'static str, pub expected: String, pub ok: bool }
pub fn explain(c: &Conditions, f: &Facts, now: SystemTime) -> Vec<Check>;
pub fn first_match_anywhere<'a>(rules: &'a [Rule], f: &Facts, now: SystemTime) -> Option<&'a Rule>; // ignora watch, respeita enabled
```

- [ ] **Passo 1: testes que falham**

- `explain_agrees_with_matches`: para cada linha da tabela de `each_condition`, `explain(...).iter().all(|c| c.ok) == expected`.
- `explain_lists_present_conditions_in_order`: `{ size_gt = "100MB", ext = ["iso", "deb"] }` → `[Check { key: "ext", expected: "iso, deb", .. }, Check { key: "size_gt", expected: "> 100 MB", .. }]` (ordem da tabela do spec, não do TOML).
- `expected` por chave: `ext` → `"iso, deb"`; `name`/`mime`/`origin` → o glob (`GlobMatcher::glob().glob()`); `name_regex` → `Regex::as_str()`; `size_gt` → `"> 100 MB"`; `size_lt` → `"< 1 MB"`; `older_than` → `"> 14 dias"`.
- `first_match_anywhere` casa um arquivo fora das pastas `watch` e pula regras pausadas.

- [ ] **Passo 2: implementar**

`explain` monta o vetor com um `if let Some` por condição, na ordem `ext, name, name_regex, mime, origin, size_gt, size_lt, older_than`. `matches` vira `explain(c, f, now).iter().all(|c| c.ok)`. `first_match` e `first_match_anywhere` compartilham um helper `fn pick(rules, f, now, in_watch: bool)`.

- [ ] **Passo 3:** testes, fmt, clippy. **Commit:** `feat(core): explain mostra o resultado de cada condição`

---

### Tarefa 3: journal v1 (tamanho, filtros, estatísticas)

**Arquivos:** Modificar `crates/core/src/journal.rs`, `crates/core/src/engine.rs`

**Interfaces:**
- Consome: `Facts.size`
- Produz:
```rust
pub struct Entry { /* campos atuais */ pub size: Option<u64> }
impl Journal {
    pub fn record(&self, rule: &str, done: &Done, size: u64) -> Result<i64>; // assinatura nova
    pub fn page(&self, filter: Filter, search: &str, before_id: Option<i64>, limit: usize) -> Result<Vec<Entry>>;
    pub fn counts(&self, search: &str) -> Result<Counts>;
    pub fn stats(&self, p: &Periods, watch: &[PathBuf]) -> Result<Stats>;
    pub fn last_for_rule(&self, rule: &str) -> Result<Option<Entry>>;
    pub fn max_id(&self) -> Result<i64>; // 0 se vazio
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter { All, Moved, Trashed, Undone, Errors }
pub struct Counts { pub all: u64, pub moved: u64, pub trashed: u64, pub undone: u64, pub errors: u64 }
pub struct Periods { pub today: i64, pub yesterday: i64, pub week: i64 } // timestamps unix do início de cada período
impl Periods { pub fn local(now: SystemTime) -> Periods } // meia-noite local de hoje, de ontem, e hoje-6 dias
pub struct Stats { pub organized_today: u64, pub organized_yesterday: u64, pub trashed_week: u64, pub freed_week: u64, pub errors_week: u64 }
```

- [ ] **Passo 1: testes que falham**

- `migrates_phase1_database`: cria um banco com o `SCHEMA` antigo (sem `size`, `user_version = 0`) e uma linha; `Journal::open` → a linha continua lá com `size == None`, e `PRAGMA user_version == 1`. Abrir de novo não falha (migração idempotente).
- `record_stores_size`.
- `filters_and_counts`: grave move, copy, rename, trash, um erro e um desfeito; confira `page` de cada filtro e `counts` (`Moved` = move/copy/rename sem erro e não desfeitos; `Trashed` = trash sem erro e não desfeito; `Undone` = `undone > 0`; `Errors` = `error IS NOT NULL`).
- `search_matches_rule_or_path`: busca `"nubank"` acha pela regra; `"fatura"` acha pelo `src`; `%` e `_` na busca são literais.
- `page_paginates_by_id`: 5 entradas, `limit 2` → ids 5,4; `before_id = 4` → 3,2.
- `stats_by_period`: com `Periods { today: 1000, yesterday: 900, week: 400 }`, entradas gravadas com `ts` ajustado por `UPDATE` direto no teste. `freed_week` soma só move/trash cujo `src.parent()` está em `watch`.
- `last_for_rule_skips_errors_and_undone`.
- `Periods::local`: `today <= now`, `today - yesterday` entre 23h e 25h, `today - week` = 6 dias ± 1h.

- [ ] **Passo 2: implementar**

```rust
fn migrate(conn: &Connection) -> Result<()> {
    let v: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if v < 1 {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch("ALTER TABLE actions ADD COLUMN size INTEGER; PRAGMA user_version = 1;")?;
        tx.commit()?;
    }
    Ok(())
}
```
`SCHEMA` continua criando a tabela da Fase 1 (sem `size`) e `migrate` roda logo depois; assim banco novo e banco antigo passam pelo mesmo caminho. `COLS` ganha `size`. Índices novos: `actions_ts ON actions(ts)`.

Filtro em SQL (`WHERE` montado por `match`); busca com `LIKE ?1 ESCAPE '\'` sobre `rule`, `CAST(src AS TEXT)` e `CAST(dst AS TEXT)`, escapando `\`, `%` e `_`. `Periods::local` usa `chrono::Local` (`date_naive().and_hms(0,0,0)` → `and_local_timezone(Local).earliest()`). `freed_week` busca `(src, size)` da semana e filtra em Rust.

`engine::process` passa `facts.size` para `record`.

- [ ] **Passo 3:** testes, fmt, clippy. **Commit:** `feat(core): journal com tamanho, filtros, busca e estatísticas`

---

### Tarefa 4: `engine::organize`

**Arquivos:** Modificar `crates/core/src/engine.rs`, `crates/core/src/actions.rs`

**Interfaces:**
- Consome: `rules::first_match_anywhere`
- Produz: `engine::organize(cfg: &Config, journal: &Journal, path: &Path, now: SystemTime) -> Result<Report>`; `actions::clear_keep(path: &Path)` (ignora erro de xattr ausente)

- [ ] **Passo 1: testes que falham**

- `organize_ignores_watch`: arquivo `a.pdf` numa pasta fora de `watch` → `Report::Done`, arquivo em `out/`.
- `organize_overrides_keep`: mova, desfaça (o arquivo ganha `user.tidyflow.keep`), `organize` → move de novo e o arquivo em `out/` não tem a marca. Pule o teste se o tmpfs não aceitar xattr `user.*` (mesmo padrão dos testes de facts da Fase 1).
- `organize_does_not_wait_min_age`: arquivo recém-criado é organizado.
- `organize_skips_paused`: com a única regra pausada → `NoMatch`.
- Copy: depois de `organize` com uma regra copy, nem a origem nem a cópia têm a marca.

- [ ] **Passo 2: implementar**

Extraia de `process` um `fn apply(rule, facts, journal) -> Result<Report>` (execute → record/record_error) e use-o nas duas funções. `organize`: `Facts::read`, `first_match_anywhere`, `apply`; se `facts.keep`, depois de `Done` chama `clear_keep` no `dst` (se houver) e, para copy, também na origem. Não consulta `was_undone_src` nem `was_copied_since` (pedido explícito).

- [ ] **Passo 3:** testes, fmt, clippy. **Commit:** `feat(core): organize para arquivos escolhidos pelo usuário`

---

### Tarefa 5: `config::edit`

**Arquivos:** Criar `crates/core/src/config/edit.rs` (mova `config.rs` para `config/mod.rs` ou use `#[path]`; escolha `config/mod.rs`). Modificar `crates/core/Cargo.toml` (`cargo add toml_edit -p tidyflow-core`).

**Interfaces:**
- Consome: `config::parse`
- Produz:
```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuleDraft {
    pub name: String,
    pub enabled: bool,
    pub watch: Vec<String>,           // como escrito (com ~)
    pub when: Vec<Condition>,         // ordem do formulário
    pub action: DraftAction,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Condition { pub key: String, pub value: String } // ext: "pdf, xml"
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "lowercase")]
pub enum DraftAction { Move(String), Copy(String), Rename(String), Trash }

pub enum EditError { Invalid(ConfigError), Conflict, Io(std::io::Error) }

pub struct Editor { /* path, doc: DocumentMut, mtime: Option<SystemTime> */ }
impl Editor {
    pub fn open(path: &Path) -> Result<Editor, EditError>; // arquivo ausente = documento vazio
    pub fn drafts(&self) -> Result<Vec<RuleDraft>, EditError>;
    pub fn save_rule(&mut self, index: Option<usize>, draft: &RuleDraft) -> Result<usize, EditError>;
    pub fn delete_rule(&mut self, index: usize) -> Result<(), EditError>;
    pub fn move_rule(&mut self, from: usize, to: usize) -> Result<(), EditError>;
    pub fn set_enabled(&mut self, index: usize, enabled: bool) -> Result<(), EditError>;
    pub fn write(&mut self) -> Result<(), EditError>;
    pub fn to_string(&self) -> String;
}
```
`EditError` implementa `Display` (mensagens em português; `Conflict`: "o config.toml mudou no disco desde que foi aberto") e `std::error::Error`.

- [ ] **Passo 1: testes que falham** (todos com o `config.example.toml` do repositório como entrada, lido com `include_str!("../../../../config.example.toml")`)

- `drafts_roundtrip`: `drafts()` devolve 5 regras; a 4ª tem `when == [ext "iso, AppImage, deb", size_gt "100MB", older_than "14d"]` e `action == Trash`.
- `save_preserves_comments_and_others`: substitui a regra 1; o comentário `# Primeira regra que casar vence` e as outras regras continuam byte a byte iguais (compare os trechos de texto).
- `save_new_appends`: `index: None` → fica por último; `parse(to_string())` aceita.
- `invalid_draft_is_rejected_and_doc_unchanged`: regex inválida → `Err(Invalid)` com a mensagem do `parse`, e `to_string()` igual ao de antes.
- `move_rule_reorders`: `move_rule(4, 0)` → `parse` devolve "Limpeza geral" primeiro; as outras na mesma ordem relativa; o bloco `[settings]` continua no topo.
- `delete_rule`, `set_enabled(false)` grava `enabled = false`, e `set_enabled(true)` remove a chave.
- `reads_subtable_style`: uma regra escrita com `[rule.when]` também vira draft.
- `write_is_atomic_and_detects_conflict`: `open`, alterar o arquivo por fora (mude o mtime com `filetime` ou espere 1,1s), `write` → `Err(Conflict)` e o conteúdo de fora fica intacto. Sem conflito, `write` grava e um segundo `write` também funciona (mtime atualizado).

- [ ] **Passo 2: implementar**

- Regras em `doc["rule"]` como `ArrayOfTables`. Para ler, aceite `when`/`action` como tabela ou tabela inline (`as_table_like()`).
- Novo `[[rule]]` gerado com `name`, `enabled` (só se `false`), `watch` (array), `when` e `action` como **tabelas inline**, no formato do exemplo.
- Substituir: guarde `decor()` e `position()` da tabela antiga e aplique na nova, para manter comentários e ordem.
- Reordenar: copie as tabelas para um `Vec`, reordene, guarde as posições originais ordenadas e reatribua-as na nova ordem com `set_position`, depois `clear` + `push`.
- Cada operação: guarde `doc.clone()`, aplique, valide com `config::parse(&doc.to_string())`; se falhar, restaure o clone e devolva `Invalid`.
- `write`: compare o mtime atual com o lido (`Conflict` se diferente), grave em `config.toml.tmp` no mesmo diretório, `fsync`, `rename`, atualize o mtime guardado. Crie o diretório se não existir.

- [ ] **Passo 3:** testes, fmt, clippy. **Commit:** `feat(core): editor de regras que preserva o TOML`

---

### Tarefa 6: CLI `organize` e notificação nova

**Arquivos:** Modificar `crates/cli/src/main.rs`, `crates/cli/src/desktop.rs`, `crates/cli/src/daemon.rs`, `crates/cli/tests/cli.rs`

**Interfaces:**
- Consome: `engine::organize`
- Produz: `tidyflow organize <ARQUIVO>...`; `desktop::organized(journal_path, id, done: &Done, rule: &str)`; `desktop::wait()` (bloqueia até as notificações abertas fecharem, para o processo da CLI não sair antes do clique)

- [ ] **Passo 1: teste que falha** em `tests/cli.rs`: config com uma regra move cujo `watch` é outra pasta; `tidyflow organize <arquivo>` com `TIDYFLOW_NO_NOTIFY=1` sai 0, imprime `#1 [regra] move ...` e o arquivo está no destino. Um arquivo sem regra imprime `nenhuma regra casou: <arquivo>` e sai 0. Arquivo inexistente sai 1.

- [ ] **Passo 2: implementar**

- `desktop::organized` monta: título `"<nome> organizado"`; corpo por tipo (`Movido para <pasta> pela regra "<regra>"`, `Copiado para …`, `Renomeado para <novo nome> …`, `Enviado à lixeira pela regra …`), abreviando `$HOME` como `~`. Botões `Desfazer` e, quando há destino, `Abrir pasta` (`xdg-open <pasta do dst>`). As threads de notificação ficam num `static` `Mutex<Vec<JoinHandle>>` para `wait()`.
- Daemon e `organize` chamam a função nova; `describe` some.
- `TIDYFLOW_NO_NOTIFY=1` desliga as notificações (para os testes e para quem não quer).
- `organize`: abre config e journal uma vez, organiza cada arquivo, imprime como o `run`, chama `wait()` no fim (até 5 min; depois sai).

- [ ] **Passo 3:** testes, fmt, clippy. **Commit:** `feat(cli): comando organize e notificação com Abrir pasta`

---

### Tarefa 7: esqueleto do app (Tauri + Vite) e CI

**Arquivos:** Criar `crates/app/*`, `ui/*`; Modificar `Cargo.toml` (membro `crates/app`), `.github/workflows/ci.yml`, `.gitignore` (`/ui/node_modules`, `/ui/dist`)

- [ ] **Passo 1: front**

```sh
npm create vite@latest ui -- --template react-ts
cd ui && npm install
npm install @tauri-apps/api @tauri-apps/plugin-dialog @tauri-apps/plugin-opener @dnd-kit/core @dnd-kit/sortable @dnd-kit/utilities
npm install -D tailwindcss @tailwindcss/vite vitest jsdom @testing-library/react @testing-library/user-event @testing-library/jest-dom @tauri-apps/cli
```
Scripts: `dev`, `build` (`tsc -b && vite build`), `lint`, `test` (`vitest`). `vite.config.ts`: plugin Tailwind, `server.port = 5173`, `strictPort: true`, `test.environment = "jsdom"`, `test.setupFiles = "src/test-setup.ts"`. Remova o conteúdo de exemplo do Vite. `index.css`: `@import "tailwindcss";` e os tokens do design em `@theme` (verde `#16a34a`-ish da marca, barra lateral `#0f172a`, fundo `#f8fafc`; confira as cores exatas no `.figcraft` com o figcraft `inspect_document`).

- [ ] **Passo 2: crate**

`crates/app/Cargo.toml`: pacote `tidyflow-app`, `[[bin]] name = "tidyflow-app"`, dependências `tauri`, `tauri-plugin-single-instance`, `tauri-plugin-dialog`, `tauri-plugin-opener`, `serde`, `serde_json`, `tidyflow-core`, `chrono`; `[build-dependencies] tauri-build`. Feature `custom-protocol = ["tauri/custom-protocol"]` (build de produção carrega `ui/dist` em vez do servidor do Vite).

`tauri.conf.json`: `productName "TidyFlow"`, `identifier "io.github.tidyflow"`, `build.frontendDist "../../ui/dist"`, `build.devUrl "http://localhost:5173"`, janela 1280×800 (mínimo 960×640), título "TidyFlow", `app.security.csp` restrito (`default-src 'self'; style-src 'self' 'unsafe-inline'`). `capabilities/default.json`: `core:default`, `dialog:allow-open`, `opener:allow-open-path`, `core:window:default`.

Ícones: exporte o logo "T" do design (figcraft `export` do nó do logo, 1024×1024 PNG) para `crates/app/icons/source.png` e rode `npx --prefix ui tauri icon crates/app/icons/source.png -o crates/app/icons`.

`main.rs`: `tauri::Builder::default()` com os três plugins e um comando `ping` só para validar. O single instance entra na Tarefa 8.

Rodar em desenvolvimento: `npm --prefix ui run dev` num terminal e `cargo run -p tidyflow-app` em outro. Produção: `npm --prefix ui run build && cargo build --release -p tidyflow-app --features custom-protocol`.

- [ ] **Passo 3: CI**

Job `test` ganha, antes do cargo: `sudo apt-get update && sudo apt-get install -y libwebkit2gtk-4.1-dev librsvg2-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev` e um `mkdir -p ui/dist && touch ui/dist/index.html` (o `generate_context!` exige o `frontendDist`). Job novo `ui`: `actions/setup-node@v4` (node 22, cache npm em `ui/package-lock.json`), `npm ci`, `lint`, `test -- --run`, `build`.

- [ ] **Passo 4:** abra a janela localmente e confira que o `ping` responde. **Commit:** `feat(app): esqueleto Tauri + React`

---

### Tarefa 8: comandos do app

**Arquivos:** Criar `crates/app/src/dto.rs`, `commands.rs`, `system.rs`; Modificar `main.rs`

**Interfaces** (todos os comandos devolvem `Result<T, String>`; o `String` é a mensagem para o usuário):

| Comando | Entrada | Saída |
|---|---|---|
| `overview` | — | `Overview { stats: Stats, recent: Vec<EntryDto> (6), folders: Vec<FolderDto> }` |
| `history` | `filter, search, before_id?` | `HistoryPage { entries: Vec<EntryDto> (50), counts: Counts, has_more }` |
| `undo` | `id?` | `EntryDto` |
| `scan` | `dry_run` | `Vec<ScanItem { path, rule?, kind?, dst?, error? }>` (só itens com regra ou erro) |
| `rules` | — | `RulesState { drafts: Vec<RuleDraft>, error: Option<ConfigErrorDto>, config_path }` |
| `save_rule` | `index?, draft` | `RulesState` |
| `delete_rule`, `move_rule`, `set_enabled` | índices / bool | `RulesState` |
| `preview` | `draft` | `Option<PreviewDto { src, dst }>` (última entrada da regra com o template do draft) |
| `inspect` | `path` | `Inspect { facts: FactsDto, rules: Vec<RuleCheck { name, enabled, checks: Vec<CheckDto>, verdict: "wins" \| "fails" \| "untested" \| "paused" }>, result: Option<{ rule, kind, dst? }> }` |
| `organize` | `path` | `EntryDto` ou erro "nenhuma regra casou" |
| `daemon_status` | — | `DaemonStatus { active: bool, since: Option<i64>, folders: usize }` |
| `start_daemon` | — | `DaemonStatus` |
| `live` | — | `Live { max_id: i64, config_mtime: Option<i64> }` |
| `open_config` | — | abre o `config.toml` (cria a partir de um arquivo vazio se não existir) |

`EntryDto`: `id, ts, rule, kind, src, dst?, size?, undone, error?`, com caminhos já abreviados (`~`) em campos `src_display`/`dst_display` além dos absolutos. `FolderDto`: `path_display, exists, rules: usize, files: usize, bytes: u64`. `FactsDto`: `path, name, ext, mime, origin_url?, origin_host?, size, modified, keep` (keep = xattr ou fallback do journal).

- [ ] **Passo 1: implementar**

- Estado gerenciado: `AppState { config_path, journal_path }`. Cada comando abre `Journal` e lê o config na hora (são baratos; evita estado velho quando o daemon ou o usuário mudam algo).
- `preview` monta um `Rule` a partir do draft serializando-o (`Editor` em memória sobre um documento vazio + `parse`) e chama `actions::destination` com `Facts::read(entry.src)` se o arquivo ainda existir, senão com `Facts` sintéticos a partir do caminho e da data da entrada. Se a montagem falhar (draft inválido), devolve `None`.
- `inspect`: `explain` para cada regra na ordem; `verdict` = `paused` se pausada; `wins` para a primeira ativa que casa (ignorando `watch`, igual ao `organize`); `fails` para as ativas antes dela; `untested` para as depois.
- `system.rs`: `systemctl --user show tidyflow -p ActiveState -p ActiveEnterTimestampMonotonic` → converta para unix; sem `systemctl` ou unidade ausente → `active: false`. `folders` conta só arquivos regulares do nível de cima (como o `scan`), ignorando nomes temporários.
- Single instance: `tauri_plugin_single_instance::init(|app, argv, _| { foca a janela; se argv tiver --test <p>, app.emit("test-file", p) })`. Na primeira execução, `--test` é lido de `std::env::args` e exposto pelo comando `initial_test_file`.

- [ ] **Passo 2: testes** — os comandos que só convertem não têm teste próprio. `system.rs` tem um teste do parser da saída do `systemctl` (string fixa → `DaemonStatus`). `dto.rs` tem um teste de `abbreviate_home`.

- [ ] **Passo 3:** fmt, clippy, test. **Commit:** `feat(app): comandos sobre o core`

---

### Tarefa 9: casca da interface

**Arquivos:** Criar `ui/src/api.ts`, `hooks/useLive.ts`, `format.ts`, `components/*`, `App.tsx`

- [ ] **Passo 1: testes que falham** (`format.test.ts`)

- `relativeTime`: agora → "agora"; 4 min → "há 4 min"; 1 h → "há 1 h"; ontem → "ontem"; mais antigo → "12 nov 22:15".
- `size`: espelha `format_size` (3,2 GB etc.).
- `ruleSummary(draft)`: `"origem *.nubank.com.br → mover"`, `"pdf, xml · nome com \"nota\" → mover"` (regex mostrada como `nome ~ /.../`), `"> 30 dias → lixeira"`, e `"· pausada"` no fim quando pausada.

- [ ] **Passo 2: implementar**

- `api.ts`: um wrapper `call<T>(cmd, args)` sobre `invoke` e funções tipadas, uma por comando da Tarefa 8; tipos espelhando `dto.rs`.
- `useLive`: `setInterval` de 2s chamando `live`; expõe um contador `version` que muda quando `max_id` ou `config_mtime` mudam; as telas usam `version` como dependência do carregamento.
- Layout: barra lateral escura fixa (logo, quatro itens com ícone, rodapé com o estado do daemon e o botão "Iniciar" quando parado), área principal clara. Navegação por estado (`useState<Screen>`), sem router.
- Componentes: `Button` (primário verde, secundário com borda), `Card`, `Chip`, `KindBadge` (mover verde, copiar azul, lixeira vermelho, renomear roxo, erro vermelho cheio, desfeito cinza), `UndoButton` (chama `undo`, mostra toast de erro), `Toast`, `Dialog`.
- Atalho: o evento `test-file` e `initial_test_file` abrem a tela Testar com o arquivo.

- [ ] **Passo 3:** lint, test, build. **Commit:** `feat(ui): layout, navegação e API`

---

### Tarefa 10: Visão geral

**Arquivos:** Criar `ui/src/screens/Overview.tsx`, `Overview.test.tsx`

- [ ] **Passo 1: teste** com `invoke` simulado: renderiza os 4 cartões com os números do mock, "↑ 8 a mais que ontem" (ou "↓ N a menos", ou "igual a ontem"), o link "Ver no histórico →" no cartão de erros vai para o Histórico com o filtro Erros, a pasta inexistente mostra "aguardando pasta".
- [ ] **Passo 2: implementar** conforme o design: cabeçalho com subtítulo ("Tudo em ordem." ou "N erro(s) nos últimos 7 dias."), botões "Simular (dry-run)" (abre `Dialog` com a lista do `scan(true)`, ou "Nada a fazer") e "Varrer agora" (`scan(false)`, toast "N arquivo(s) organizado(s)"), atividade recente com "Ver tudo", pastas monitoradas, caixa da dica de atalho.
- [ ] **Passo 3:** lint, test, build. **Commit:** `feat(ui): visão geral`

---

### Tarefa 11: Histórico

**Arquivos:** Criar `ui/src/screens/History.tsx`, `History.test.tsx`

- [ ] **Passo 1: teste**: digitar na busca chama `history` com `search` (debounce 250ms); clicar no chip "Lixeira" chama com `filter: "trashed"`; "Carregar mais" passa `before_id` = menor id exibido e concatena; linha desfeita aparece riscada e sem botão; linha com erro tem "Detalhes".
- [ ] **Passo 2: implementar** a tabela do design (ID, quando, ação, arquivo com origem → destino abaixo, regra, botão), chips com contagem (Erros em vermelho), "Desfazer a última", "Mostrando N de M ações". Detalhes abre `Dialog` com a mensagem, `src` e a regra.
- [ ] **Passo 3:** lint, test, build. **Commit:** `feat(ui): histórico`

---

### Tarefa 12: Regras

**Arquivos:** Criar `ui/src/screens/Rules.tsx`, `RuleList.tsx`, `RuleEditor.tsx`, `ConditionRow.tsx` e testes

- [ ] **Passo 1: testes**

- `RuleEditor`: "Adicionar condição" adiciona uma linha com o primeiro tipo ainda não usado; o seletor de tipo de cada linha não oferece tipos já usados; com todos os 8 usados o botão some. Trocar a aba de ação para Lixeira esconde o campo de destino. Clicar em `{year}` insere no cursor do campo de destino. "Salvar regra" chama `save_rule` com o draft montado; um erro devolvido aparece no topo do formulário. "Descartar" volta ao draft original.
- `RuleList`: o interruptor chama `set_enabled`; regra pausada aparece esmaecida com "pausada".
- `Rules`: com `error` no estado, mostra a mensagem com a linha e "Abrir config.toml", sem a lista.

- [ ] **Passo 2: implementar**

- Lista com `@dnd-kit/sortable`; ao soltar, `move_rule(from, to)`.
- Editor à direita: nome, pastas (chips com ×, "+ Adicionar pasta" com o seletor de pasta do plugin dialog, abreviando `$HOME` como `~`), condições (`ConditionRow`: tipo em português — Extensão, Nome (glob), Nome (regex), Tipo (MIME), Origem do download, Maior que, Menor que, Mais antigo que — o texto "casa com"/"é" e o valor), ação em abas (Mover, Copiar, Renomear, Lixeira), destino com os botões de variável.
- Prévia: `preview(draft)` com debounce de 400ms; mostra "Prévia com o último arquivo que casou" com origem → destino.
- "+ Nova regra" abre o editor vazio (watch `~/Downloads`, uma condição Extensão, ação Mover). "Excluir regra" pede confirmação.
- Conflito (`Conflict`) mostra o toast "O config.toml mudou no disco. Recarreguei; salve de novo." e recarrega o estado mantendo o draft no formulário.

- [ ] **Passo 3:** lint, test, build. **Commit:** `feat(ui): editor de regras`

---

### Tarefa 13: Testar arquivo

**Arquivos:** Criar `ui/src/screens/TestFile.tsx`, `TestFile.test.tsx`

- [ ] **Passo 1: teste** com um `Inspect` simulado: a regra `wins` aparece destacada com o selo "vence"; as `untested` mostram "não testada" e as marcas ✓/× de cada `check`; a faixa de resultado mostra `mover → <dst>`; sem `result`, mostra "Nenhuma regra casaria". "Aplicar agora" chama `organize` e mostra o toast com o id.
- [ ] **Passo 2: implementar** conforme o design: área de soltar (via `getCurrentWebview().onDragDropEvent`) e "Escolher arquivo…" (plugin dialog); "O que foi lido do arquivo"; lista de regras; nota de rodapé ("Depois que uma regra casa, as seguintes não são testadas…"); faixa escura de resultado.
- [ ] **Passo 3:** lint, test, build. **Commit:** `feat(ui): testar arquivo`

---

### Tarefa 14: menu de contexto, `.desktop` e README

**Arquivos:** Criar `contrib/tidyflow.desktop`, `contrib/nautilus/TidyFlow/Organizar com TidyFlow`, `contrib/nautilus/TidyFlow/Testar no TidyFlow`, `contrib/nemo/tidyflow-organize.nemo_action`, `contrib/nemo/tidyflow-test.nemo_action`; Modificar `README.md`

- [ ] **Passo 1: arquivos**

Scripts do Nautilus (executáveis, `#!/bin/sh`), lendo `NAUTILUS_SCRIPT_SELECTED_FILE_PATHS` linha a linha:
```sh
#!/bin/sh
# Organizar com TidyFlow
IFS='
'
exec tidyflow organize $NAUTILUS_SCRIPT_SELECTED_FILE_PATHS
```
```sh
#!/bin/sh
# Testar no TidyFlow (só o primeiro arquivo)
first=$(printf '%s\n' "$NAUTILUS_SCRIPT_SELECTED_FILE_PATHS" | head -n1)
exec tidyflow-app --test "$first"
```
Nemo:
```ini
[Nemo Action]
Name=Organizar com TidyFlow
Comment=Aplica a primeira regra que casar
Exec=tidyflow organize %F
Selection=notnone
Extensions=nodirs;
Icon-Name=edit-clear-all
```
(o de teste com `Exec=tidyflow-app --test %F`, `Selection=s`). `tidyflow.desktop`: `Exec=tidyflow-app`, `Icon=tidyflow`, `Categories=Utility;FileTools;`.

- [ ] **Passo 2: README**: seção "Interface" (build de produção, copiar o binário para `~/.cargo/bin`, o `.desktop` para `~/.local/share/applications/` e o ícone 128×128 para `~/.local/share/icons/hicolor/128x128/apps/tidyflow.png`), seção "Menu de contexto" (Nautilus: copiar a pasta `TidyFlow` para `~/.local/share/nautilus/scripts/` e `chmod +x`; Nemo: copiar os `.nemo_action` para `~/.local/share/nemo/actions/`), comando `organize` na tabela, `enabled = false` na seção Regras, aviso de que `organize` ignora `watch` e a marca de desfeito.

- [ ] **Passo 3: teste manual** no Nautilus: os dois itens aparecem em Scripts → TidyFlow e funcionam com um e com vários arquivos (nomes com espaço). **Commit:** `feat: menu de contexto no Nautilus e no Nemo`

---

### Tarefa 15: verificação final

- [ ] `cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test`
- [ ] `npm --prefix ui run lint && npm --prefix ui test -- --run && npm --prefix ui run build`
- [ ] Build de produção do app e uso real por 10 min: daemon ativo, baixar um arquivo, ver aparecer na Visão geral sem recarregar, desfazer pelo app, editar uma regra e conferir no `config.toml` que comentários continuam lá e que o daemon recarregou (`journalctl --user -u tidyflow`), pausar uma regra, arrastar a ordem, testar um arquivo pelo menu do Nautilus.
- [ ] Comparar cada tela com `docs/design/tidyflow-ui.png` e anotar diferenças intencionais no PR.
