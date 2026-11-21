# TidyFlow — Windows e macOS — Design

Data: 2026-11-21


## Contexto

Até a 0.1 o release só tinha o pacote Linux. Pedido: releases para Windows e macOS antes da 3c, com **paridade completa** e **sem assinatura**. Não dá para só adicionar plataformas ao workflow, porque o código depende do Linux em vários pontos:

| Ponto | Onde | Linux hoje |
|---|---|---|
| Serviço em segundo plano | `crates/app/src/system.rs` | `systemctl --user` |
| Origem, página, id do download, marca de desfeito | `core/facts.rs`, `core/actions.rs`, `cli/native_host.rs` | xattr `user.*` (o crate `xattr` não existe no Windows) |
| Eventos do daemon | `cli/daemon.rs` (`on_event`) | `Access(Close(Write))`, que só o inotify emite: no macOS e no Windows o daemon **não veria arquivos novos** |
| Desfazer lixeira | `core/actions.rs` (`trash::os_limited`) | não existe no macOS |
| Caminhos | `core/lib.rs` `home()`/`xdg_dir`, `journal.rs` bytes de caminho (`os::unix`) | XDG, `$HOME` |
| Native messaging | `cli/browser.rs` | script `sh` + pastas `~/.mozilla` etc. (o Windows usa o Registro e não roda `sh`) |
| Notificação com botões Desfazer/Abrir pasta | `cli/desktop.rs` | `wait_for_action` do notify-rust só existe no Linux; `xdg-open` |
| Menu de contexto, instalação | `contrib/` | Nautilus/Nemo, `install.sh`, unit systemd |

**Branch:** `multiplataforma` criado a partir de `fase3`, não do `main`. A extensão e a Biblioteca só existem na `fase3`, e partir do `main` obrigaria a portar a extensão duas vezes. A 3c continua depois em cima deste branch.

**Sem Windows nem Mac para testar:** a verificação fica com o CI (testes + um smoke test de ponta a ponta em cada sistema). O README vai marcar Windows/macOS como **experimental** até alguém testar a parte visual.

## Desenho

### 1. Caminhos (`crates/core/src/lib.rs`, `config/mod.rs`, `journal.rs`)
- `home()`: `std::env::home_dir()` (HOME no Unix, USERPROFILE no Windows).
- Pastas do TidyFlow: Linux continua no XDG; no macOS fica em `~/Library/Application Support/TidyFlow/`; no Windows, em `%APPDATA%\TidyFlow\` (`config.toml`, `journal.db`, `daemon.lock`). Uma função `core::dirs::{config_dir, data_dir}` substitui os `xdg_dir`.
- `expand_tilde` aceita também `~\`.
- `journal.rs`: `as_encoded_bytes`/`from_encoded_bytes_unchecked` no lugar de `os::unix` (no Unix são os mesmos bytes, então o banco existente continua compatível).

### 2. Metadados do arquivo: novo `crates/core/src/meta.rs`
API única: `meta::get(path, Tag)`, `set`, `set_if_absent`, `remove`, `copy_all(src, dst)`, com `Tag::{Origin, Referrer, Download, Keep}`.
- **Linux/macOS:** xattr com os nomes atuais (`user.xdg.origin.url`…), então o que já está gravado continua valendo. `xattr` passa a ser dependência só `cfg(unix)`.
- **macOS:** se não houver xattr nosso, a origem e a página vêm de `com.apple.metadata:kMDItemWhereFroms` (bplist `[url, referrer]`, gravado por Safari, Chrome e Firefox). Nova dependência: `plist`.
- **Windows:** fluxos alternativos do NTFS (`arquivo:tidyflow.origin` etc., lidos e gravados com `std::fs` no caminho `"{path}:{stream}"`). Sem fluxo nosso, a origem e a página vêm do `Zone.Identifier` (`HostUrl=`, `ReferrerUrl=`, gravado por Chrome, Edge e Firefox). `copy_all` copia os nossos fluxos e o `Zone.Identifier`.
- Quem passa a usar: `Facts::read` (`facts.rs`), `keep`/`clear_keep`/`copy_xattrs` (`actions.rs`), `set_if_absent` e `xattr::set` (`native_host.rs`), e a dica do `check` (`cli/main.rs`: `setfattr` só no Linux; nos outros sistemas, um novo `tidyflow check --clear-keep <arquivo>`).

### 3. Daemon (`crates/cli/src/daemon.rs`)
- `on_event` fica como está no Linux. No macOS e no Windows também disparam `Create(File|Any)`, `Modify(Data|Any)` e `Modify(Name(To|Any))` quando o caminho existe. O macOS emite `Modify(Metadata)` quando o host grava o xattr, e esse caso continua passando por `download_finished_since`. No Windows, a escrita de fluxo chega como `Modify(Any)`.
- Como esses sistemas mandam vários eventos por arquivo, o daemon ignora o mesmo caminho por `QUIET` depois de tratá-lo. `wait_stable`, `busy` e a proteção contra loop já cobrem o resto.
- No início, o daemon pega `File::try_lock` em `data_dir/daemon.lock` e regrava o arquivo, então o mtime marca o início. Isso impede dois daemons ao mesmo tempo e permite ao app saber se o daemon está rodando no Windows e no macOS.

### 4. Lixeira no macOS (`actions.rs`)
- `trash::os_limited` só existe no Linux e no Windows. No macOS, a ação lixeira move o arquivo para `~/.Trash/<nome livre>` com `rename` e grava esse caminho como `dst`; o desfazer move de volta. Se o arquivo estiver em outro volume, usa `trash::delete` e o desfazer avisa que não é possível. Preciso conferir no código do undo que `Kind::Trash` com `dst` preenchido funciona.

### 5. Serviço: novo `crates/core/src/service.rs` + `tidyflow service enable|disable|start|status`
- **Linux:** systemd, como hoje (`parse_show` sai de `app/system.rs` para cá). `enable` grava a unit (o que o `install.sh` faz hoje) e roda `systemctl --user enable --now`.
- **macOS:** LaunchAgent `~/Library/LaunchAgents/io.github.tidyflow.daemon.plist` (`RunAtLoad`, `KeepAlive` em caso de falha). Ativação com `launchctl bootstrap gui/$(id -u)`, início com `launchctl kickstart`. O estado vem do `daemon.lock`.
- **Windows:** valor `TidyFlow` em `HKCU\…\CurrentVersion\Run` apontando para `tidyflowd.exe daemon`. `start` abre o processo destacado e o estado vem do `daemon.lock`. Nova dependência: `winreg` (`cfg(windows)`).
- `app/system.rs` `daemon_status`/`start_daemon` passam a chamar `core::service`.

### 6. Binário sem console no Windows (`crates/cli`)
- O `cli` vira lib (`src/lib.rs` com os módulos e a função `main`) + `src/main.rs` (`tidyflow`) + `src/bin/tidyflowd.rs` com `#![cfg_attr(windows, windows_subsystem = "windows")]`. Sem isso, o daemon no login, o menu de contexto e o host da extensão abririam uma janela de console. Nos outros sistemas, o `tidyflowd` é só um apelido e não entra nos pacotes.

### 7. Extensão de navegador (`crates/cli/src/browser.rs`, `cli/main.rs`)
- O manifest passa a apontar direto para o executável (`tidyflowd.exe` no Windows) em vez do script `sh`. Antes do clap, `main` reconhece a chamada do navegador (1º argumento `chrome-extension://…` ou um `.json`, como o Firefox passa) e entra em `native_host::run`. O `uninstall` continua removendo o script antigo.
- **macOS:** pastas em `~/Library/Application Support/{Mozilla, Google/Chrome, Chromium, BraveSoftware/Brave-Browser, Microsoft Edge}/NativeMessagingHosts`.
- **Windows:** manifests em `%APPDATA%\TidyFlow\native-host\{firefox,chromium}.json` e chaves `HKCU\Software\{Mozilla, Google\Chrome, Chromium, Microsoft\Edge, BraveSoftware\Brave-Browser}\NativeMessagingHosts\io.github.langendolff.tidyflow`.
- Os testes atuais continuam valendo no Linux. As listas de outros sistemas ganham testes com a home falsa.

### 8. Notificações (`crates/cli/src/desktop.rs`)
- Os botões Desfazer e Abrir pasta (`wait_for_action`) ficam só no Linux via `cfg`. No macOS e no Windows a notificação sai sem botões, e o desfazer fica no app e no `tidyflow undo`. `wait()` retorna na hora quando não há notificação aberta. `tilde()` usa `core::home()`.

### 9. Menu de contexto
- **Windows:** `HKCU\Software\Classes\*\shell\TidyFlow` ("Organizar com o TidyFlow", comando `"tidyflowd.exe" organize "%1"`), gravado por `install.ps1`.
- **macOS:** Ação Rápida do Finder em `contrib/macos/Organizar com o TidyFlow.workflow` (Automator, "Executar script de shell" → `tidyflow organize "$@"`), copiada para `~/Library/Services` pelo instalador. Não dá para testar sem Mac, então fica como experimental no README.

### 10. Pacotes e release (`.github/workflows/release.yml`)
Os jobs viram uma matriz. A interface e a extensão são compiladas uma vez, no job Linux, e as outras plataformas baixam o `ui/dist` como artefato.
- **Linux:** igual a hoje. `install.sh` passa a chamar `tidyflow service enable` no fim, se o usuário quiser.
- **Windows (`windows-latest`):** `TidyFlow-<v>-windows-x64.zip` com `bin\{tidyflow,tidyflowd,tidyflow-app}.exe`, `install.ps1`, `uninstall.ps1`, `config.example.toml` e README. O `install.ps1` instala em `%LOCALAPPDATA%\Programs\TidyFlow`, cria o atalho no Menu Iniciar, coloca o executável no PATH do usuário, copia o config de exemplo, roda `tidyflow browser install` e grava o menu de contexto. O serviço não é ligado automaticamente, como no Linux. O WebView2 já vem no Windows 10/11.
- **macOS (`macos-latest`):** universal (arm64 + x86_64). Os binários `tidyflow-app` e `tidyflow` são compilados para os dois alvos e juntados com `lipo`. O `TidyFlow.app` sai do bundler do Tauri (`npx @tauri-apps/cli build --target universal-apple-darwin --bundles app`, só no CI, com `bundle.active` ligado por `--config`). O `tidyflow` vai em `TidyFlow.app/Contents/MacOS/`, e tudo é assinado ad hoc (`codesign -s -`), porque arm64 sem assinatura nem abre. Pacote `TidyFlow-<v>-macos-universal.zip` com o `.app`, `install.sh` do macOS (copia para `~/Applications`, cria link do `tidyflow` em `~/.local/bin`, tira a quarentena com `xattr -dr com.apple.quarantine`, copia o config e roda `browser install`) e a Ação Rápida.
- `SHA256SUMS.txt` junta os três pacotes. O upload para o release fica num job final que depende da matriz.

### 11. CI (`.github/workflows/ci.yml`)
- O job `test` vira matriz `ubuntu / windows / macos` (`fmt`, `clippy -D warnings` e `test`). Testes que usam `PermissionsExt`, symlink ou nomes de arquivo inválidos em UTF-8 ganham `#[cfg(unix)]` ou `#[cfg(target_os = "linux")]`.
- **Smoke test**: os testes de integração de `crates/cli/tests/cli.rs` já sobem o daemon de verdade (arquivo novo, arquivo renomeado para a pasta, undo sem refazer, host da extensão por stdin, espera pelo fim do download). Com `TIDYFLOW_DATA_DIR` e caminhos em strings literais do TOML, eles rodam nos 3 sistemas no lugar de scripts `smoke.sh`/`smoke.ps1` separados.

### 12. Documentação
- `README.md` e `README.en.md`: seções de instalação para Windows e macOS, incluindo como passar pelo aviso do SmartScreen e do Gatekeeper ("Abrir mesmo assim" em Ajustes → Privacidade e Segurança), o comando `tidyflow service enable` e o selo **experimental**.
- Spec e plano em `docs/superpowers/{specs,plans}/2026-11-21-tidyflow-multiplataforma*.md`, no formato das fases anteriores.

## Ordem dos commits (cada um com o CI verde nos 3 sistemas)
1. CI em matriz + `cfg` nos testes (deixa o Windows e o macOS compilando: caminhos, `journal` bytes, `xattr` só no Unix com stub no Windows).
2. `core::dirs` + `home_dir` + `~\`.
3. `core::meta` (xattr / ADS / kMDItemWhereFroms / Zone.Identifier) e troca dos usos.
4. Daemon: eventos de macOS/Windows, dedupe, `daemon.lock`.
5. Lixeira com desfazer no macOS.
6. `cli` lib + `tidyflowd` + `core::service` + `tidyflow service …` + `app/system.rs`.
7. Native messaging em macOS/Windows + detecção de chamada pelo navegador.
8. Notificações por plataforma.
9. Smoke tests no CI.
10. Pacotes: `install.ps1`/`uninstall.ps1`, `install.sh` do macOS, Ação Rápida, `release.yml` em matriz.
11. READMEs, spec e plano.

## Verificação
- Local (Linux): `cargo fmt --check`, `cargo clippy --all-targets -D warnings`, `cargo test`, `npm` da ui/browser e `contrib/smoke.sh`. Reinstalar o app (`--features custom-protocol`) e conferir que nada mudou no Linux.
- CI em cada push do branch: os 3 sistemas verdes, incluindo o smoke test.
- Release: `workflow_dispatch` com uma tag de teste (ex.: `v0.2.0-rc.1`, marcada como pre-release) para gerar os três pacotes. Confiro o conteúdo de cada um (lista de arquivos, `lipo -info`, `codesign -dv`) antes de qualquer release de verdade. A tag e o release só com o seu ok.
- Limite conhecido: a parte visual (janela do app, notificação, menu de contexto, Ação Rápida, Gatekeeper/SmartScreen) não é testada por ninguém até um usuário rodar nessas plataformas. Por isso o selo experimental.
