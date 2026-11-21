# TidyFlow Fase 3 (navegador, biblioteca, sugestões): plano de implementação

> **Para agentes:** SUB-SKILL OBRIGATÓRIO: use superpowers:subagent-driven-development (recomendado) ou superpowers:executing-plans para implementar este plano tarefa por tarefa. Os passos usam checkbox (`- [ ]`) para acompanhamento.

**Objetivo:** extensão de navegador que informa os downloads ao TidyFlow (3a), tela Biblioteca com tudo o que chegou nas pastas monitoradas (3b) e sugestões de regras por agrupamento, opcionalmente com o Ollama (3c).

**Arquitetura:** sem IPC. O navegador abre `tidyflow native-host`, que grava no mesmo SQLite do journal e escreve xattrs no arquivo. O daemon lê o registro para saber se um download terminou e reage ao evento de xattr. As sugestões rodam no core (agrupamento, validação, simulação). O Ollama é falado por HTTP, atrás da feature `ai`. O front continua sem lógica de regras.

**Tech Stack:** Rust (edition 2024), rusqlite, serde_json, ureq (feature `ai`); WebExtension MV3 em TypeScript, esbuild, web-ext, Vitest; React 19 + Tailwind 4 no app.

**Spec:** `docs/superpowers/specs/2026-11-21-tidyflow-fase3-design.md`

## Restrições globais

- Todas as restrições das fases 1 e 2 continuam (Linux, lixeira, nunca sobrescrever, mensagens em português, executar → verificar → gravar no journal, o front nunca monta caminhos nem avalia regras).
- Cada commit passa em `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, nos checks do `ui/` e, a partir da Tarefa 4, em `npm --prefix browser run lint`, `npm --prefix browser test -- --run` e `npm --prefix browser run build`.
- Dependências entram com `cargo add` / `npm install`. Se a API mudou em relação ao plano, adapte a chamada e mantenha o comportamento.
- Três PRs: Tarefas 1–7 (3a), 8–10 (3b), 11–17 (3c). Cada PR passa no CI e atualiza o README antes de ser mesclado em `main`.
- O stdout do `native-host` é só do protocolo: logs vão para stderr.
- Downloads de janela privada nunca chegam ao host.

## Estrutura de arquivos

```
crates/core/src/journal.rs          migrações v3 e v4; `conn` passa a pub(crate); record/mark_undone atualizam downloads
crates/core/src/downloads.rs        Download, DownloadState, impl Journal (registro e consultas da biblioteca)
crates/core/src/facts.rs            + referrer_url, download_id, site(), category()
crates/core/src/rules.rs            + condição `site`
crates/core/src/template.rs         + {site}
crates/core/src/config/mod.rs       + Conditions.site, Config.ai (AiSettings)
crates/core/src/config/edit.rs      + chave `site`, set_ai, insert_rule
crates/core/src/engine.rs           + facts() com fallback no registro, busy(), registro source='folder'
crates/core/src/xdg.rs              user_dir("DOCUMENTS") a partir de user-dirs.dirs
crates/core/src/suggest/mod.rs      analyze(), Suggestion, Analysis
crates/core/src/suggest/group.rs    agrupamento e assinaturas
crates/core/src/suggest/propose.rs  propostas sem IA
crates/core/src/suggest/validate.rs conversão, posição, simulação, conflitos
crates/core/src/suggest/ollama.rs   cliente HTTP (feature `ai`)
crates/core/src/suggest/store.rs    tabela suggestions
crates/cli/src/native_host.rs       protocolo e mensagens
crates/cli/src/browser.rs           browser install|uninstall
crates/cli/src/main.rs              + native-host, browser, suggest
crates/cli/src/daemon.rs            + eventos de metadados, busy()
crates/app/src/commands.rs          + biblioteca, sugestões, IA
crates/app/src/dto.rs               + Download, Suggestion, AiStatus
ui/src/screens/Library.tsx
ui/src/screens/Suggestions.tsx
ui/src/components/Sidebar.tsx       + Biblioteca, Sugestões (com contador)
ui/src/format.ts                    + site, categorias, estados
browser/package.json, tsconfig.json, build.mjs, eslint.config.js
browser/src/background.ts           listeners → sendNativeMessage
browser/src/messages.ts             conversão pura de eventos em mensagens
browser/src/messages.test.ts
browser/src/setup.html, setup.ts    página "instale o host"
browser/manifest.base.json          + overrides firefox/chromium no build.mjs
browser/README.md                   instalação e roteiro de teste manual
contrib/install.sh                  + tidyflow browser install
.github/workflows/ci.yml            + job browser
.github/workflows/release.yml       + zip da extensão (e .xpi assinado se houver segredos)
README.md, README.en.md
```

---

## PR 3a: extensão de navegador

### Tarefa 1: tabela `downloads` (migração v3)

**Arquivos:** Criar `crates/core/src/downloads.rs`. Modificar `crates/core/src/journal.rs`, `crates/core/src/lib.rs`.

**Interfaces:**
- Produz:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadState { InProgress, Complete, Interrupted, Trashed, Gone }
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Source { Browser, Folder }
#[derive(Debug, Clone, serde::Serialize)]
pub struct Download {
    pub id: i64, pub source: Source, pub browser: Option<String>, pub browser_id: Option<i64>,
    pub path: PathBuf, pub current_path: PathBuf,
    pub url: Option<String>, pub referrer: Option<String>, pub page_title: Option<String>,
    pub mime: Option<String>, pub size: Option<u64>, pub state: DownloadState,
    pub started_at: i64, pub finished_at: Option<i64>,
}
pub struct Started<'a> { pub browser: &'a str, pub browser_id: i64, pub path: &'a Path, pub url: &'a str,
    pub referrer: Option<&'a str>, pub page_title: Option<&'a str>, pub mime: Option<&'a str>, pub started_at: i64 }
impl Journal {
    pub fn download_started(&self, s: &Started) -> Result<i64>;
    /// a linha mais recente `in_progress` daquele navegador/id
    pub fn download_renamed(&self, browser: &str, browser_id: i64, path: &Path) -> Result<Option<i64>>;
    pub fn download_finished(&self, browser: &str, browser_id: i64, size: u64) -> Result<Option<Download>>;
    pub fn download_interrupted(&self, browser: &str, browser_id: i64) -> Result<()>;
    pub fn download_set_state(&self, id: i64, state: DownloadState) -> Result<()>;
    /// linha `in_progress` para `current_path = path`, iniciada depois de `since`
    pub fn download_active(&self, path: &Path, since: SystemTime) -> Result<bool>;
    /// linha `complete` para `current_path = path` com `finished_at >= since`
    pub fn download_finished_since(&self, path: &Path, since: SystemTime) -> Result<bool>;
    pub fn download_at(&self, path: &Path) -> Result<Option<Download>>;
}
```

- [ ] **Passo 1: testes que falham** (em `downloads.rs`)
- `migrates_v2_database`: banco com `user_version = 2` e uma ação → abre, `user_version == 3`, ação intacta, tabela `downloads` existe. Abrir de novo não falha.
- `started_then_finished`: `download_started` → `download_active(path, ontem) == true`; `download_finished` → estado `Complete`, `size`, `finished_at` preenchidos; `download_active == false`; `download_finished_since(path, há 1 min) == true`.
- `renamed_updates_path_and_current_path`.
- `finished_matches_latest_in_progress`: dois `started` com o mesmo `browser_id` (sessões diferentes do navegador); o primeiro já `complete`; `finished` atualiza o segundo.
- `active_ignores_stale_rows`: `started_at` de 25h atrás e `since = now - 24h` → `false`.
- `unknown_finished_returns_none`.

- [ ] **Passo 2: implementar**

Migração v3 (nova entrada em `steps`):
```sql
CREATE TABLE downloads (
    id INTEGER PRIMARY KEY, source TEXT NOT NULL, browser TEXT, browser_id INTEGER,
    path BLOB NOT NULL, current_path BLOB NOT NULL,
    url TEXT, referrer TEXT, page_title TEXT, mime TEXT, size INTEGER,
    state TEXT NOT NULL, started_at INTEGER NOT NULL, finished_at INTEGER
);
CREATE INDEX downloads_current ON downloads(current_path);
CREATE INDEX downloads_browser ON downloads(browser, browser_id);
CREATE INDEX downloads_started ON downloads(started_at);
```
Caminhos como BLOB com os helpers `bytes`/`path` do journal (passam a `pub(crate)`), igual a `actions`. `Journal.conn` vira `pub(crate)`. `lib.rs` ganha `pub mod downloads;`.

- [ ] **Passo 3:** testes, fmt, clippy. **Commit:** `feat(core): registro de downloads (journal v3)`

---

### Tarefa 2: `Facts` com referrer, condição `site` e `{site}`

**Arquivos:** Modificar `crates/core/src/facts.rs`, `crates/core/src/config/mod.rs`, `crates/core/src/config/edit.rs`, `crates/core/src/rules.rs`, `crates/core/src/template.rs`, `ui/src/format.ts`, `ui/src/screens/TestFile.tsx`, `crates/app/src/dto.rs`

**Interfaces:**
- Produz: `facts::REFERRER_XATTR = "user.xdg.referrer.url"`, `facts::DOWNLOAD_XATTR = "user.tidyflow.download"`; `Facts.referrer_url: Option<String>`, `Facts.download_id: Option<i64>`; `Facts::referrer_host()`, `Facts::site() -> Option<String>` (host do referrer, senão host da origem); `Conditions.site: Option<GlobMatcher>`.

- [ ] **Passo 1: testes que falham**
- `facts.rs`: `site_prefers_referrer` (origem `objects.githubusercontent.com`, referrer `https://github.com/x/y/releases` → `github.com`); sem referrer → host da origem; sem nenhum → `None`. Leitura dos dois xattrs novos (pular se o tmpfs não aceitar `user.*`, como nos testes existentes).
- `config`: `when = { site = "*.github.com" }` compila; `site` sozinho conta como condição (não é "when vazio").
- `rules.rs`: linha nova na tabela de `each_condition` para `site`; `explain` coloca `site` logo depois de `origin`.
- `template.rs`: `{site}` → `github.com`; sem site → `unknown`.
- `edit.rs`: um draft com `site` grava e lê de volta.

- [ ] **Passo 2: implementar**

Generalize a extração de host de `origin_host` num `fn host_of(url: &str) -> Option<String>` e reuse. Inclua `site` em `is_empty`, em `CONDITION_KEYS` do `edit.rs` (depois de `origin`) e em `explain` (esperado = o glob). `Facts::assumed` e `Facts::sample` com os campos novos em `None`.

No front: `CONDITION_KEYS`, `CONDITION_LABEL.site = 'Site'`, placeholder `'*.github.com'`, botão de variável `{site}` no editor. A tela Testar arquivo mostra "Referrer" e "Site" em "O que foi lido" (o DTO ganha os dois campos).

- [ ] **Passo 3:** testes Rust e do front, fmt, clippy, lint. **Commit:** `feat: condição site e variável {site}`

---

### Tarefa 3: `tidyflow native-host` e `tidyflow browser install`

**Arquivos:** Criar `crates/cli/src/native_host.rs`, `crates/cli/src/browser.rs`. Modificar `crates/cli/src/main.rs`, `crates/cli/Cargo.toml` (`cargo add serde serde_json xattr -p tidyflow`), `crates/cli/tests/cli.rs`.

**Interfaces:**
- Consome: `Journal::download_*` (Tarefa 1), constantes de xattr (Tarefa 2)
- Produz:
```rust
// native_host.rs
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    Started { id: i64, path: PathBuf, url: String, final_url: Option<String>, referrer: Option<String>,
              page_title: Option<String>, mime: Option<String>, browser: String, started_at: i64 },
    Finished { id: i64, browser: String, path: PathBuf, size: u64 },
    Interrupted { id: i64, browser: String, path: PathBuf, error: Option<String> },
    Renamed { id: i64, browser: String, path: PathBuf },
}
pub fn read_message(r: &mut impl Read) -> io::Result<Option<Vec<u8>>>; // None = EOF
pub fn write_message(w: &mut impl Write, v: &serde_json::Value) -> io::Result<()>;
pub fn handle(journal: &Journal, msg: Message) -> tidyflow_core::Result<()>;
pub fn run(journal_path: &Path) -> tidyflow_core::Result<()>; // loop até EOF
// browser.rs
pub const HOST_NAME: &str = "io.github.langendolff.tidyflow";
pub const FIREFOX_ID: &str = "tidyflow@langendolff.github.io";
pub const CHROMIUM_ID: &str = /* da Tarefa 4 */;
pub fn install(home: &Path, exe: &Path) -> io::Result<Vec<PathBuf>>; // arquivos gravados
pub fn uninstall(home: &Path) -> io::Result<Vec<PathBuf>>;
```

- [ ] **Passo 1: testes que falham**
- Unidade (`native_host.rs`): `read_message`/`write_message` com tamanho em little-endian; mensagem maior que 1 MB → erro; EOF limpo → `None`; EOF no meio do tamanho → erro.
- `handle`: `Finished` atualiza o estado **antes** de gravar o xattr (teste: depois de `handle`, a linha está `complete` e o arquivo tem `user.tidyflow.download = <id>`); não sobrescreve `user.xdg.origin.url` já existente; grava origem e referrer quando ausentes; arquivo inexistente → estado `gone`, sem erro.
- `browser.rs`: em uma home temporária com `.mozilla/` e `.config/chromium/` criados, `install` grava `~/.local/share/tidyflow/native-host` (executável, com `exec '<exe>' native-host "$@"`), o manifest do Firefox com `allowed_extensions: [FIREFOX_ID]` e o do Chromium com `allowed_origins: ["chrome-extension://<CHROMIUM_ID>/"]`; não cria nada para Chrome, Brave e LibreWolf (pastas ausentes). `uninstall` remove só o que `install` grava.
- Integração (`tests/cli.rs`): roda o binário `native-host`, escreve `started` e `finished` no stdin no protocolo real, fecha o stdin; o processo sai com 0, o stdout tem duas respostas `{"ok":true}` e o banco tem a linha `complete`. Uma mensagem com JSON inválido recebe `{"ok":false,...}` e a seguinte ainda é processada.

- [ ] **Passo 2: implementar**

`main.rs`: subcomandos `NativeHost { #[arg(trailing_var_arg = true, allow_hyphen_values = true, hide = true)] _args: Vec<String> }` (o Firefox passa o caminho do manifest e o ID; o Chromium passa a origem) e `Browser { #[command(subcommand)] cmd: BrowserCmd }` com `Install` e `Uninstall`, que imprimem os arquivos gravados ou removidos. `native-host` não inicializa o `tracing` no stdout.

Pastas dos manifests (só quando a pasta-raiz do navegador existe):

| Navegador | Raiz conferida | Pasta do manifest |
|---|---|---|
| Firefox | `~/.mozilla` | `~/.mozilla/native-messaging-hosts/` |
| LibreWolf | `~/.librewolf` | `~/.librewolf/native-messaging-hosts/` |
| Chromium | `~/.config/chromium` | `NativeMessagingHosts/` dentro dela |
| Chrome | `~/.config/google-chrome` | idem |
| Brave | `~/.config/BraveSoftware/Brave-Browser` | idem |

`handle(Finished)`: `download_finished` → se a linha não existe, só registra no log; se o arquivo não existe, `download_set_state(Gone)`; senão grava os xattrs (falha de xattr vira aviso no stderr, não erro). `Renamed` antes de `Started` é ignorado.

- [ ] **Passo 3:** testes, fmt, clippy. **Commit:** `feat(cli): host de native messaging e browser install`

---

### Tarefa 4: extensão (`browser/`)

**Arquivos:** Criar `browser/package.json`, `browser/tsconfig.json`, `browser/eslint.config.js`, `browser/build.mjs`, `browser/manifest.base.json`, `browser/src/background.ts`, `browser/src/messages.ts`, `browser/src/messages.test.ts`, `browser/src/setup.html`, `browser/src/setup.ts`, `browser/icons/` (copiados de `crates/app/icons/`), `browser/README.md`, `browser/.gitignore` (`dist/`, `node_modules/`, `web-ext-artifacts/`). Modificar `crates/cli/src/browser.rs` (`CHROMIUM_ID`), `.github/workflows/ci.yml`.

**Interfaces:**
- Produz (`messages.ts`, sem dependência de API do navegador):
```ts
export type BrowserName = 'firefox' | 'chromium'
export interface DownloadLike { id: number; filename: string; url: string; finalUrl?: string; referrer?: string;
  mime?: string; incognito: boolean; startTime: string; fileSize?: number; totalBytes?: number }
export interface DeltaLike { id: number; state?: { current?: string }; filename?: { current?: string }; error?: { current?: string } }
export type HostMessage = { type: 'started' | 'finished' | 'interrupted' | 'renamed'; browser: BrowserName; id: number; path: string; [k: string]: unknown }
export function onCreated(d: DownloadLike, browser: BrowserName, pageTitle: string | null): HostMessage | null
export function onChanged(delta: DeltaLike, item: DownloadLike | undefined, browser: BrowserName): HostMessage[]
export function titleFor(referrer: string | undefined, tabs: { url?: string; title?: string }[]): string | null
```

- [ ] **Passo 1: projeto**

`npm init` em `browser/`, `npm install -D typescript esbuild web-ext vitest eslint typescript-eslint @types/firefox-webext-browser @types/chrome`. Scripts: `build` (`node build.mjs`, gera `dist/firefox/` e `dist/chromium/`), `lint` (`eslint . && tsc --noEmit && web-ext lint -s dist/firefox`), `test` (`vitest`), `package` (`web-ext build -s dist/firefox -a dist/` e zip de `dist/chromium`).

`manifest.base.json`: `manifest_version: 3`, nome "TidyFlow", `permissions: ["downloads", "nativeMessaging", "tabs"]`, ícones, `action` com `default_title`. `build.mjs` faz o bundle de `background.ts` e `setup.ts` com esbuild e grava dois manifests:
- Firefox: `background.scripts: ["background.js"]`, `browser_specific_settings.gecko = { id: "tidyflow@langendolff.github.io", strict_min_version: "128.0" }`.
- Chromium: `background.service_worker: "background.js"`, `key: <chave pública>`.

ID fixo do Chromium: gere uma chave RSA 2048 uma vez (`openssl genrsa 2048 | openssl rsa -pubout -outform DER | base64 -w0`), grave só a chave pública em `manifest.base.json` (a privada é descartada: extensão descompactada não precisa dela). O ID são os 32 primeiros hex do SHA-256 do DER, com `0-9a-f` mapeados para `a-p`. Grave esse ID em `CHROMIUM_ID` no `browser.rs` e um teste em `build.mjs` (ou no Vitest) que recalcula o ID a partir da chave e compara com o valor do Rust lido por regex, para os dois nunca divergirem.

- [ ] **Passo 2: testes que falham** (`messages.test.ts`)
- `onCreated` com `incognito: true` → `null`; normal → `started` com `path = filename`, `started_at` em segundos unix, `final_url` e `page_title`.
- `onCreated` sem `filename` (Chromium, antes de decidir o nome) → `null`.
- `onChanged`: `state.current = 'complete'` → `finished` com `size` de `fileSize ?? totalBytes`; `'interrupted'` → `interrupted` com o erro; `filename.current` → `renamed`; os dois na mesma delta → `[renamed, finished]` nessa ordem; item incógnito → `[]`.
- Uma delta de `filename` para um item cujo `started` foi pulado por não ter nome → `started` (o Chromium só tem o nome depois).
- `titleFor` acha a aba pela URL exata do referrer, e ignora o fragmento `#...`.

- [ ] **Passo 3: implementar**

`background.ts`: `const api = globalThis.browser ?? chrome`; registra `downloads.onCreated` e `downloads.onChanged` no topo do script (exigência do service worker). Para `onChanged`, busca o item com `downloads.search({ id })`. Guarda em `storage.session` os ids que já receberam `started`, para o caso do service worker reiniciar. Envia cada mensagem com `runtime.sendNativeMessage(HOST_NAME, msg)`. Se falhar com "host não encontrado", troca o ícone/badge para "!" e o clique na ação abre `setup.html`, que mostra o comando `tidyflow browser install`. Nenhuma fila de reenvio.

- [ ] **Passo 4: CI.** Job `browser` em `ci.yml`, igual ao `ui`, em `working-directory: browser`: `npm ci`, `npm run build`, `npm run lint`, `npm test -- --run`.

- [ ] **Passo 5:** `browser/README.md` com a instalação (Firefox: `.xpi` do release ou `about:debugging` → carregar temporário; Chromium: `chrome://extensions` → modo desenvolvedor → carregar `dist/chromium`) e o roteiro de teste manual da Tarefa 7. **Commit:** `feat(browser): extensão que informa downloads ao TidyFlow`

---

### Tarefa 5: daemon e engine usando o registro

**Arquivos:** Modificar `crates/core/src/engine.rs`, `crates/cli/src/daemon.rs`, `crates/app/src/commands.rs` (Testar arquivo usa `engine::facts`)

**Interfaces:**
- Consome: `download_active`, `download_finished_since`, `download_at`
- Produz:
```rust
/// Download em andamento: registro da extensão (até 24h) ou heurística antiga.
pub fn busy(journal: &Journal, path: &Path, now: SystemTime) -> bool;
/// Facts::read + origem/referrer do registro quando os xattrs faltam (fs sem xattr).
pub fn facts(journal: &Journal, path: &Path) -> Result<Facts>;
```

- [ ] **Passo 1: testes que falham**
- `engine`: `scan_skips_registered_in_progress` (arquivo com 1 KB, sem `.part` ao lado, linha `in_progress` → não processado; depois de `download_finished` → processado). `busy_ignores_stale` (linha de 25h → `false`). `facts_falls_back_to_registry` (sem xattr, linha com URL `https://x.com/a.pdf` → `origin_host == "x.com"`).
- `daemon` (teste de integração em `tests/cli.rs`, no estilo do `daemon_does_not_redo_undone_file`): daemon rodando, cria `a.pdf` e registra `started` pelo `native-host` → depois de 4s o arquivo continua lá; envia `finished` → o arquivo vai para o destino em até 5s. Use tempos generosos: o teste equivalente da fase 2 é instável no runner do GitHub.

- [ ] **Passo 2: implementar**

`engine::process`, `scan` e `organize` passam a usar `engine::facts`. `scan` troca `stability::download_in_progress` por `busy`. No daemon:
- `handle` troca a checagem por `engine::busy(&self.journal, path, SystemTime::now())`.
- `on_event` aceita também `EventKind::Modify(ModifyKind::Metadata(_))`, mas só chama `handle` se `journal.download_finished_since(path, received - 60s)`. A mesma guarda de laço (`is_dst_since`) já evita reagir aos xattrs que o próprio TidyFlow copia ou remove.

- [ ] **Passo 3:** testes, fmt, clippy. **Commit:** `feat: daemon espera o fim do download pelo registro da extensão`

---

### Tarefa 6: install.sh, release e README (3a)

**Arquivos:** Modificar `contrib/install.sh`, `.github/workflows/release.yml`, `README.md`, `README.en.md`

- [ ] **Passo 1:** `install.sh` chama `tidyflow browser install` depois de instalar o binário e imprime os links de instalação da extensão.
- [ ] **Passo 2:** `release.yml`: `npm ci && npm run build && npm run package` em `browser/`, e anexa `tidyflow-chromium-<versão>.zip` ao release. Se os segredos `AMO_JWT_ISSUER` e `AMO_JWT_SECRET` existirem, roda `web-ext sign --channel=unlisted` e anexa o `.xpi`; senão, pula o passo com um aviso no log. As chaves do AMO precisam ser criadas pelo dono do repositório (não fazer isso pelo agente).
- [ ] **Passo 3:** README (PT e EN): seção "Extensão de navegador" (o que ela envia, que não envia nada de janelas privadas, instalação, `tidyflow browser install`); a subseção "Origem do download" troca "Firefox: a confirmar" por "Firefox: com a extensão"; condição `site` e variável `{site}` na seção Regras; linha na tabela de funcionalidades. **Commit:** `docs: extensão de navegador no README e no release`

---

### Tarefa 7: verificação do PR 3a

- [ ] Todos os checks (Rust, `ui/`, `browser/`).
- [ ] Teste manual, com o roteiro do `browser/README.md`, no Firefox e no Chromium: `tidyflow browser install`; carregar a extensão; baixar um PDF de um site com CDN (por exemplo, um release do GitHub); ver no `tidyflow check <arquivo>` a origem, o referrer e o site; regra com `site = "github.com"` move o arquivo sem esperar a varredura; um download grande, cancelado no meio, não é movido; um download numa janela privada não aparece em `downloads`; com o host desinstalado, a extensão mostra o aviso e o download funciona normalmente.
- [ ] Abrir o PR 3a. Mesclar só depois do CI verde e da aprovação do usuário.

---

## PR 3b: biblioteca

### Tarefa 8: registro `source = 'folder'` e `current_path`

**Arquivos:** Modificar `crates/core/src/downloads.rs`, `crates/core/src/journal.rs`, `crates/core/src/engine.rs`, `crates/core/src/facts.rs`

**Interfaces:**
- Produz:
```rust
impl Journal {
    /// cria a linha source='folder' se não houver nenhuma para o caminho; devolve o id
    pub fn ensure_download(&self, f: &Facts) -> Result<i64>;
}
// facts.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Category { Document, Image, Video, Audio, Archive, Installer, Other }
pub fn category(mime: &str, ext: &str) -> Category;
```

- [ ] **Passo 1: testes que falham**
- `process_registers_folder_file`: arquivo sem linha → depois de `process` (sem dry-run), há uma linha `Folder` com origem do xattr, `started_at = mtime`; dry-run não grava nada; uma segunda chamada não duplica.
- `current_path_follows_actions`: move → `current_path` = destino, estado `Complete`; rename idem; trash → estado `Trashed`, `current_path` igual ao anterior; copy → nada muda; undo de cada um volta ao estado anterior.
- `sweep_gone_marks_downloads`: `current_path` apagado → `Gone`; linhas `Trashed` não são tocadas.
- `category`: tabela com `application/pdf`, `.docx`, `.odt`, `text/plain` → Document; `image/png` → Image; `.deb`, `.AppImage`, `.iso`, `.rpm`, `.exe`, `.msi` → Installer; `.zip`, `.tar.gz`, `.7z`, `.rar`, `.zst` → Archive; `application/octet-stream` sem extensão conhecida → Other.

- [ ] **Passo 2: implementar**

`process` chama `ensure_download` antes de casar regras, exceto em dry-run. `journal.record` e `mark_undone` rodam numa transação que inclui o `UPDATE downloads ...` correspondente (move/rename: `current_path = dst WHERE current_path = src`; trash: `state = 'trashed' WHERE current_path = src`; undo: o inverso). `sweep_gone` ganha o passo dos downloads `complete` e `in_progress` (só os de mais de 24h) cujo `current_path` não existe.

- [ ] **Passo 3:** testes, fmt, clippy. **Commit:** `feat(core): biblioteca acompanha onde cada arquivo está`

---

### Tarefa 9: consultas da biblioteca e comandos do app

**Arquivos:** Modificar `crates/core/src/downloads.rs`, `crates/app/src/commands.rs`, `crates/app/src/dto.rs`, `crates/app/src/main.rs`, `ui/src/api.ts`

**Interfaces:**
- Produz:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Place { All, InWatch, Organized, Trashed, Missing, Downloading }
pub struct LibraryQuery<'a> { pub place: Place, pub category: Option<Category>, pub site: Option<&'a str>,
    pub since: Option<i64>, pub search: &'a str, pub before_id: Option<i64>, pub limit: usize, pub watch: &'a [PathBuf] }
pub struct LibraryItem { pub download: Download, pub category: Category, pub site: Option<String>, pub last_action: Option<Entry> }
impl Journal {
    pub fn library(&self, q: &LibraryQuery) -> Result<Vec<LibraryItem>>;
    pub fn top_sites(&self, limit: usize) -> Result<Vec<(String, u64)>>;
}
```
Comandos Tauri: `library(query) -> Vec<LibraryItemDto>`, `library_sites() -> Vec<SiteDto>`, `open_url(url)` (via opener, só `http`/`https`).

- [ ] **Passo 1: testes que falham** (`downloads.rs`): cada `Place` (`InWatch` = estado `complete` e `current_path.parent()` numa pasta `watch`; `Organized` = `complete` fora delas; `Downloading` = `in_progress` de menos de 24h); filtro por categoria e site; busca em nome, URL e título, com `%`/`_` literais; paginação por id; `last_action` é a ação mais nova com `src` ou `dst` igual a `current_path`; `top_sites` ordena por quantidade.

- [ ] **Passo 2: implementar.** Categoria e site são calculados em Rust depois do SQL (filtro de categoria em Rust, com paginação feita pedindo blocos até completar `limit`). Os comandos de ação reaproveitam os que já existem (`organize`, `undo`, abrir pasta, `--test`).

- [ ] **Passo 3:** testes, fmt, clippy. **Commit:** `feat: consultas da biblioteca`

---

### Tarefa 10: tela Biblioteca e PR 3b

**Arquivos:** Criar `ui/src/screens/Library.tsx`, `ui/src/screens/Library.test.tsx`. Modificar `ui/src/components/Sidebar.tsx`, `ui/src/App.tsx`, `ui/src/format.ts`, `ui/src/test/tauri.ts`, `README.md`, `README.en.md`

- [ ] **Passo 1: testes que falham** (Vitest, API simulada): chips de estado e de site filtram a chamada `library`; a busca espera 300 ms antes de consultar; "Carregar mais" passa o `before_id`; item `missing` mostra "não encontrado" e esconde "abrir"; item `downloading` aparece no topo com "baixando"; "abrir a página de origem" chama `open_url` com o referrer.
- [ ] **Passo 2: implementar** no estilo das telas existentes (cartões, chips, tabela do Histórico). Item "Biblioteca" na barra lateral, entre Visão geral e Regras. Atualização ao vivo pelo `useLive` (o polling de `max_id` passa a considerar também `max(downloads.id)` e `max(downloads.finished_at)`: crie `Journal::change_token()` que devolve os três e use-o no lugar de `max_id`).
- [ ] **Passo 3:** README com a tela (captura da tela no mesmo formato das outras em `docs/images/`). Checks, teste manual com downloads reais, PR 3b. **Commit:** `feat(ui): tela Biblioteca`

---

## PR 3c: sugestões

### Tarefa 11: `[ai]` no config e pastas XDG

**Arquivos:** Criar `crates/core/src/xdg.rs`. Modificar `crates/core/src/config/mod.rs`, `crates/core/src/config/edit.rs`, `crates/core/src/lib.rs`, `config.example.toml`

**Interfaces:**
- Produz:
```rust
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct AiSettings { pub enabled: bool, pub url: String, pub model: String }
// padrão: false, "http://localhost:11434", "qwen2.5:7b"
impl AiSettings { pub fn is_loopback(&self) -> bool } // localhost, 127.0.0.0/8, ::1
pub struct Config { /* ... */ pub ai: AiSettings }
impl Editor {
    pub fn set_ai(&mut self, ai: &AiSettings) -> Result<(), EditError>;
    /// save_rule(None) + move_rule para `at`
    pub fn insert_rule(&mut self, at: usize, draft: &RuleDraft) -> Result<usize, EditError>;
}
// xdg.rs
pub enum UserDir { Documents, Pictures, Videos, Music, Download }
/// lê $XDG_CONFIG_HOME/user-dirs.dirs; devolve no formato "~/Documentos" para o TOML
pub fn user_dir(d: UserDir) -> String;
```

- [ ] **Passo 1: testes que falham**: config sem `[ai]` → padrão; URL inválida (sem `http://` ou `https://`) → erro com a linha; `is_loopback` para `localhost`, `127.0.0.1:11434`, `[::1]` e `192.168.0.2`; `set_ai` cria a tabela quando não existe e preserva comentários; `insert_rule(0, ...)` coloca a regra no topo; `user_dir` lê `XDG_DOCUMENTS_DIR="$HOME/Documentos"` e cai para `~/Documents` sem o arquivo.
- [ ] **Passo 2: implementar.** `config.example.toml` ganha o bloco `[ai]` comentado.
- [ ] **Passo 3:** testes, fmt, clippy. **Commit:** `feat(core): configuração da IA local e pastas XDG`

---

### Tarefa 12: agrupamento e propostas sem IA

**Arquivos:** Criar `crates/core/src/suggest/mod.rs`, `group.rs`, `propose.rs`. Modificar `crates/core/src/lib.rs`

**Interfaces:**
- Produz:
```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Criterion { SiteMime { site: String, family: String }, NamePattern { pattern: String, ext: String }, Ext { ext: String } }
pub struct Group { pub criterion: Criterion, pub files: Vec<Facts> }
impl Group { pub fn signature(&self) -> String } // estável: "site-mime:github.com:application/pdf"
pub fn candidates(cfg: &Config, journal: &Journal, now: SystemTime) -> Result<Vec<Facts>>;
pub fn group(files: Vec<Facts>) -> Vec<Group>; // mínimo 3 por grupo; cada arquivo em um grupo só
pub fn normalize_stem(stem: &str) -> Option<String>; // "Fatura-2026-11" → "fatura-####-##"
pub fn pattern_glob(pattern: &str, ext: &str) -> String; // "fatura-*-*.pdf"
pub struct UndoHeavy { pub rule: String, pub undone: u64, pub total: u64, pub examples: Vec<PathBuf> }
impl Journal { pub fn undo_heavy(&self, since: i64) -> Result<Vec<UndoHeavy>> } // ≥3 desfeitos e ≥30%
pub fn propose(g: &Group, watch: &Path) -> RuleDraft; // sem IA
```

- [ ] **Passo 1: testes que falham**
- `normalize_stem`: tabela com `Fatura-2026-11` → `fatura-####-##`; `Screenshot_20261101_101010` → `screenshot_########_######`; `IMG_1234` → `img_####`; `1234` → `None` (sem token de letras com 3 ou mais caracteres); `relatório final` → `relatório final` (sem dígitos é aceito).
- `pattern_glob` troca cada sequência de `#` por `*` e escapa `[ ] { } ? *` do resto.
- `group`: 3 PDFs do mesmo site viram um grupo `SiteMime`; 2 não viram; 4 arquivos `fatura-*` sem site viram `NamePattern`; um arquivo que já entrou num grupo `SiteMime` não entra em `NamePattern`; `.deb` soltos viram `Ext`; a ordem dos grupos e dos arquivos é estável (por critério e depois por nome).
- `candidates`: só arquivos das pastas `watch` que nenhuma regra ativa pega, sem a marca keep, ignorando `busy`; o site vem do registro quando o xattr falta.
- `undo_heavy`: 3 de 10 desfeitos → entra; 2 de 3 → não entra (menos de 3); 3 de 20 → não entra (15%).
- `propose`: `SiteMime` de PDFs → `when = [site = "github.com", mime = "application/pdf"]`, `move` para `user_dir(Documents)/{site}/`; imagens → Pictures; `Ext` de `deb` → `move` para `<watch>/Instaladores/`; `zip` → `<watch>/Compactados/`; nomes descritivos ("PDFs de github.com"); o `watch` da regra é a pasta de onde os arquivos vieram.

- [ ] **Passo 2: implementar.** Famílias de MIME: `application/pdf` e os MIMEs de documento de escritório ficam como estão; `image/*`, `video/*`, `audio/*` e `text/*` viram o glob da família.
- [ ] **Passo 3:** testes, fmt, clippy. **Commit:** `feat(core): agrupa arquivos sem regra e propõe regras`

---

### Tarefa 13: validação, posição e simulação

**Arquivos:** Criar `crates/core/src/suggest/validate.rs`

**Interfaces:**
- Produz:
```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct Conflict { pub rule: String, pub files: u64 }
#[derive(Debug, Clone, serde::Serialize)]
pub struct Checked { pub draft: RuleDraft, pub position: usize, pub matches: u64,
    pub examples: Vec<(PathBuf, PathBuf)>, pub conflicts: Vec<Conflict> }
/// None = descartada (não compila ou não pega nenhum arquivo do grupo)
pub fn check(config_src: &str, draft: RuleDraft, group: &Group, watched_files: &[Facts], now: SystemTime) -> Option<Checked>;
```

- [ ] **Passo 1: testes que falham**
- Proposta com glob inválido → `None`.
- Nome repetido → sufixo `" (2)"`, `" (3)"`.
- Proposta que não casa com nenhum arquivo do grupo → `None`.
- Posição: config com "Limpeza geral" (`older_than = "30d"`) e um arquivo antigo do grupo → a regra nova fica antes dela, e `conflicts` traz `Conflict { rule: "Limpeza geral", files: 1 }`; sem conflito → posição = fim.
- `examples` tem até 3 pares origem → destino calculados com `actions::destination`.
- O config de entrada não é alterado (a validação usa `Editor::in_memory`).

- [ ] **Passo 2: implementar** com `Editor::in_memory(config_src)` + `insert_rule` + `config::parse` do resultado e `rules::first_match` para cada arquivo, antes e depois.
- [ ] **Passo 3:** testes, fmt, clippy. **Commit:** `feat(core): valida e simula sugestões`

---

### Tarefa 14: cliente do Ollama

**Arquivos:** Criar `crates/core/src/suggest/ollama.rs`. Modificar `crates/core/Cargo.toml` (`cargo add ureq --optional -p tidyflow-core --no-default-features --features json`, `cargo add serde_json -p tidyflow-core`, feature `ai = ["dep:ureq"]`), `crates/cli/Cargo.toml` e `crates/app/Cargo.toml` (`tidyflow-core` com `features = ["ai"]`)

**Interfaces:**
- Produz (só com a feature `ai`):
```rust
pub enum AiError { Unreachable(String), ModelMissing(String), Timeout, BadResponse(String) }
pub fn models(ai: &AiSettings) -> Result<Vec<String>, AiError>;          // GET /api/tags, 3s
pub fn schema() -> serde_json::Value;                                   // JSON Schema da resposta
pub fn prompt(groups: &[Group], rules: &[RuleSummary], dirs: &[String], vars: &[&str]) -> String;
pub fn ask(ai: &AiSettings, prompt: &str) -> Result<Vec<AiProposal>, AiError>; // POST /api/chat, 120s
#[derive(serde::Deserialize)]
pub struct AiProposal { pub group: String, pub name: String, pub when: Vec<Condition>, pub action: DraftAction, pub reason: String }
```

- [ ] **Passo 1: testes que falham**, com um servidor falso em `std::net::TcpListener` numa thread, que responde uma vez com o corpo dado:
- `models` lê `{"models":[{"name":"qwen2.5:7b"}]}`; conexão recusada → `Unreachable`.
- `ask` envia `stream: false`, `format` = `schema()`, `options.temperature <= 0.2` e o modelo configurado (o teste confere o corpo recebido); resposta com `message.content` em JSON válido → propostas; conteúdo que não é JSON → `BadResponse`; HTTP 404 com `model ... not found` → `ModelMissing`; servidor que não responde → `Timeout` (use um timeout de teste de 200 ms, injetado por um `fn ask_with_timeout` interno).
- `prompt` contém os padrões, as contagens, no máximo 10 exemplos por grupo e as variáveis; não contém caminhos absolutos fora da home trocada por `~`.

- [ ] **Passo 2: implementar.** O prompt pede uma regra por grupo, com preferência por pastas existentes, usando as chaves de condição e as ações do TidyFlow, e o motivo em uma frase em português.
- [ ] **Passo 3:** `cargo test` com e sem `--features ai` em `tidyflow-core`, fmt, clippy. **Commit:** `feat(core): cliente do Ollama para sugestões`

---

### Tarefa 15: `suggest::analyze`, tabela `suggestions` e `tidyflow suggest`

**Arquivos:** Criar `crates/core/src/suggest/store.rs`. Modificar `crates/core/src/suggest/mod.rs`, `crates/core/src/journal.rs` (migração v4), `crates/cli/src/main.rs`, `crates/cli/tests/cli.rs`

**Interfaces:**
- Produz:
```rust
pub enum AiStatus { Off, Unavailable(String), Used { model: String } }
pub enum Stage { Collecting, Grouping, AskingAi, Checking }
pub struct Suggestion { pub id: i64, pub signature: String, pub kind: SuggestionKind, pub reason: String }
pub enum SuggestionKind { NewRule(Checked), Review(UndoHeavy) }
pub struct Analysis { pub suggestions: Vec<Suggestion>, pub ai: AiStatus }
pub fn analyze(cfg: &Config, config_src: &str, journal: &Journal, use_ai: bool, now: SystemTime,
               progress: &dyn Fn(Stage), cancel: &AtomicBool) -> Result<Analysis>;
impl Journal {
    pub fn suggestions_new(&self) -> Result<Vec<Suggestion>>;
    pub fn suggestion_set_status(&self, id: i64, status: SuggestionStatus) -> Result<()>;
    pub fn suggestion_count_new(&self) -> Result<u64>;
}
```

- [ ] **Passo 1: testes que falham**
- Migração v4 a partir de v3.
- `analyze` sem IA num diretório com 3 PDFs de um site e 2 sem nada → uma sugestão `NewRule`; grava em `suggestions` com `status = 'new'`; uma segunda análise substitui as `new` e não duplica.
- Assinatura dispensada há 10 dias não volta; há 91 dias volta.
- Com IA e servidor falso: a proposta da IA substitui a sem IA do mesmo grupo; uma proposta da IA inválida cai para a sem IA; servidor fora do ar → `AiStatus::Unavailable` e sugestões sem IA.
- `cancel` marcado antes de `AskingAi` → devolve `Err("cancelado")` sem gravar nada.
- CLI: `tidyflow suggest --no-ai` em pasta temporária imprime o bloco TOML (`[[rule]]` com `name`, `watch`, `when`, `action`) e não altera o `config.toml`; `--json` imprime JSON válido.

- [ ] **Passo 2: implementar.** `analyze` só chama o Ollama com `use_ai && cfg.ai.enabled`. O TOML de cada sugestão é gerado com `Editor::in_memory("")` + `save_rule`, para sair igual ao que o app gravaria.
- [ ] **Passo 3:** testes, fmt, clippy. **Commit:** `feat: análise de sugestões e tidyflow suggest`

---

### Tarefa 16: tela Sugestões

**Arquivos:** Criar `ui/src/screens/Suggestions.tsx`, `ui/src/screens/Suggestions.test.tsx`. Modificar `crates/app/src/commands.rs`, `crates/app/src/dto.rs`, `crates/app/src/main.rs`, `ui/src/components/Sidebar.tsx`, `ui/src/App.tsx`, `ui/src/api.ts`, `ui/src/screens/Rules.tsx` (abrir o editor com um draft pronto)

**Interfaces:**
- Comandos Tauri: `ai_status() -> AiStatusDto { enabled, url, model, loopback, reachable, models }`, `set_ai(ai)`, `analyze(use_ai) -> AnalysisDto` (async, `spawn_blocking`, emite o evento `suggest-progress` com o `Stage`), `cancel_analysis()`, `suggestions() -> Vec<SuggestionDto>`, `accept_suggestion(id) -> usize` (`insert_rule` na posição, grava, status `accepted`), `dismiss_suggestion(id)`, `apply_suggestion(id) -> Vec<ReportDto>` (`engine::process` em cada arquivo do grupo), `suggestion_count() -> u64`.

- [ ] **Passo 1: testes que falham** (Vitest, API simulada): cartão de IA nos três estados (desligada, não encontrada, pronta com o modelo); aviso quando `loopback = false`; "Analisar agora" mostra o progresso e o botão Cancelar; Aceitar chama `accept_suggestion` e troca o cartão por "Aplicar agora aos N arquivos"; Editar antes navega para Regras com o draft; Dispensar remove o cartão; sugestão `Review` mostra os exemplos e o link para a regra; o contador da barra lateral reflete `suggestion_count`.
- [ ] **Passo 2: implementar.** Se o config mudou em disco durante o Aceitar, mesmo tratamento do editor de regras (`EditError::Conflict` → recarrega e avisa). O app guarda um `Arc<AtomicBool>` de cancelamento no `AppState`. Como a chamada HTTP não é interrompível, o cancelamento descarta o resultado quando ela volta.
- [ ] **Passo 3:** checks Rust e front. **Commit:** `feat(ui): tela Sugestões`

---

### Tarefa 17: README e verificação do PR 3c

- [ ] README (PT e EN): seção "Sugestões" (o que é analisado, que só metadados vão para o modelo, que nada é aplicado sem aceitar), instalação opcional do Ollama (`ollama pull qwen2.5:7b`), bloco `[ai]`, comando `suggest`; a badge "rede: nenhuma" continua verdadeira com a configuração padrão, e o README diz isso explicitamente.
- [ ] Todos os checks.
- [ ] Teste manual: com a pasta de Downloads real (ou uma cópia em `XDG_*` de demonstração), analisar sem Ollama, aceitar uma sugestão, conferir o `config.toml` (comentários preservados, regra na posição certa) e o daemon recarregando; ligar o Ollama, analisar de novo, comparar nomes e pastas; desligar o serviço do Ollama e conferir o aviso "IA indisponível"; dispensar uma sugestão e conferir que ela não volta.
- [ ] Abrir o PR 3c. Atualizar a memória do projeto com o estado da fase.
