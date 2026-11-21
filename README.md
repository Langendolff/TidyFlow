<div align="center">

<img src="docs/images/icon.png" width="112" alt="Ícone do TidyFlow">

# TidyFlow

**Sua pasta de Downloads se organiza sozinha.**

Organizador automático de arquivos para Linux, Windows e macOS. Regras em TOML, tudo desfazível, nada sobrescrito.

[![Release](https://img.shields.io/github/v/release/Langendolff/TidyFlow?style=flat-square&color=0E9F6E)](https://github.com/Langendolff/TidyFlow/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/Langendolff/TidyFlow/ci.yml?style=flat-square&label=CI)](https://github.com/Langendolff/TidyFlow/actions/workflows/ci.yml)
![Linux](https://img.shields.io/badge/Linux-systemd-0E9F6E?style=flat-square&logo=linux&logoColor=white)
![Windows](https://img.shields.io/badge/Windows-experimental-0078D4?style=flat-square&logo=windows&logoColor=white)
![macOS](https://img.shields.io/badge/macOS-experimental-111827?style=flat-square&logo=apple&logoColor=white)
![Rust](https://img.shields.io/badge/Rust-2024-B7410E?style=flat-square&logo=rust&logoColor=white)
![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB?style=flat-square&logo=tauri&logoColor=white)
![Offline](https://img.shields.io/badge/rede-nenhuma-111827?style=flat-square)
[![Licença: PolyForm Noncommercial](https://img.shields.io/badge/licen%C3%A7a-PolyForm%20Noncommercial-blue?style=flat-square)](LICENSE)

[🇺🇸 English version](README.en.md) · [Baixar](#download) · [Regras](#regras) · [Menu de contexto](#menu-de-contexto)

<br>

<img src="docs/images/overview.png" width="820" alt="Visão geral do TidyFlow">

</div>

## Por que o TidyFlow

Organizadores comuns olham só para a extensão. O TidyFlow também sabe **de qual site o
arquivo veio**, qual é o tipo real pelo conteúdo e há quanto tempo ele está parado. Uma regra
de poucas linhas resolve o caso que mais incomoda:

```toml
[[rule]]
name   = "Faturas Nubank"
watch  = ["~/Downloads"]
when   = { origin = "*.nubank.com.br", mime = "application/pdf" }
action = { move = "~/Documentos/Finanças/{year}/" }
```

```
~/Downloads/fatura-2026-11.pdf
        ↓
~/Documentos/Finanças/2026/fatura-2026-11.pdf     [Desfazer]
```

Cada ação vai para um histórico e pode ser desfeita pela notificação, pelo terminal ou por
um atalho de teclado.

## Funcionalidades

| | |
|---|---|
| 🌐 **Origem do download** | Regras pelo site de onde o arquivo veio (`origin = "*.nubank.com.br"`), com a pasta do site como variável. |
| 🔍 **MIME pelo conteúdo** | O tipo é detectado lendo o arquivo, não confiando na extensão. |
| 🧹 **Limpeza por idade** | `older_than = "30d"` + `trash` mantém a pasta limpa sem apagar nada de vez. |
| ↩️ **Tudo desfazível** | Histórico em SQLite, desfazer pela notificação, por `tidyflow undo` ou por atalho. |
| 🛡️ **Nada é sobrescrito** | Se o destino já existe, a ação falha e fica registrada como erro. |
| ⏳ **Espera o download terminar** | Arquivos parciais (`.part`, `.crdownload`) e arquivos ainda mudando são ignorados. |
| 🧩 **Extensão de navegador** | Opcional, para Firefox e Chromium: informa a página de origem de cada download e avisa quando ele termina. |
| 🖥️ **Interface opcional** | App Tauri + React para ver a atividade, editar regras e testar um arquivo. |
| 📂 **Menu de contexto** | "Organizar com TidyFlow" e "Testar no TidyFlow" no Nautilus e no Nemo. |
| 📴 **100% local** | Sem contas, sem nuvem, sem telemetria. |

## Telas

<p align="center">
<img src="docs/images/rules.png" width="820" alt="Editor de regras">
</p>
<p align="center">
<img src="docs/images/library.png" width="820" alt="Biblioteca de downloads">
</p>
<p align="center">
<img src="docs/images/history.png" width="405" alt="Histórico com desfazer">&nbsp;
<img src="docs/images/test.png" width="405" alt="Testar arquivo">
</p>

Cinco telas: **Visão geral** (números, atividade recente, pastas monitoradas, varrer ou
simular), **Biblioteca** (tudo o que chegou nas pastas monitoradas: de que site veio, onde
está agora e o que o TidyFlow fez, com busca e filtros), **Regras** (editar, pausar, arrastar para reordenar), **Histórico** (busca,
filtros, desfazer) e **Testar arquivo** (o que o TidyFlow lê de um arquivo e qual regra
casaria).

## Download

Na página de [**Releases**](https://github.com/Langendolff/TidyFlow/releases/latest), cada
versão traz um pacote por sistema e um `SHA256SUMS.txt`. Nenhum instalador liga o daemon
sozinho: as regras de exemplo mandam para a lixeira o que está há mais de 30 dias em
`~/Downloads`, então revise-as antes.

> **Windows e macOS são experimentais.** O CI compila e testa o daemon, a CLI e o host da
> extensão nos dois sistemas, mas a interface, as notificações e o menu de contexto ainda
> não foram testados por ninguém neles. Problemas são bem-vindos nas [Issues](https://github.com/Langendolff/TidyFlow/issues).

### Linux

`TidyFlow-<versão>-linux-x64.tar.gz` (x64 com systemd; a interface precisa de `webkit2gtk-4.1`).

```sh
tar xzf TidyFlow-*-linux-x64.tar.gz
./TidyFlow/install.sh                # binários em ~/.local/bin, config, atalho, ícone e menu de contexto
nano ~/.config/tidyflow/config.toml  # revise as regras antes de ligar o daemon
tidyflow check && tidyflow run --dry-run
tidyflow service enable              # liga o daemon (systemd do usuário)
```

### Windows

`TidyFlow-<versão>-windows-x64.zip` (Windows 10 ou 11; o WebView2 já vem no sistema).

1. Extraia o zip e dê dois cliques em **`Instalar.cmd`**. Ele instala em
   `%LOCALAPPDATA%\Programs\TidyFlow`, coloca o `tidyflow` no PATH, cria o atalho no Menu
   Iniciar, registra o host da extensão e o item **Organizar com o TidyFlow** no menu de
   contexto do Explorer (no Windows 11, em "Mostrar mais opções").
2. Os binários não têm assinatura: se o SmartScreen avisar, clique em **Mais informações →
   Executar assim mesmo**.
3. Revise `%APPDATA%\TidyFlow\config.toml` e, num terminal novo:

```powershell
tidyflow check; tidyflow run --dry-run
tidyflow service enable   # liga o daemon no login (sem janela)
```

Para remover: `%LOCALAPPDATA%\Programs\TidyFlow\uninstall.ps1` (o config e o histórico ficam).

### macOS

`TidyFlow-<versão>-macos-universal.zip` (Apple Silicon e Intel, macOS 11 ou mais novo).

```sh
cd ~/Downloads/TidyFlow   # a pasta extraída do zip
sh install.sh             # TidyFlow.app em ~/Applications, comando em ~/.local/bin, Ação Rápida do Finder
nano ~/Library/Application\ Support/TidyFlow/config.toml
tidyflow check && tidyflow run --dry-run
tidyflow service enable   # liga o daemon no login (LaunchAgent)
```

O app não tem assinatura da Apple. O `install.sh` tira a quarentena do download; se
mesmo assim o macOS disser que o app não pode ser aberto, vá em **Ajustes do Sistema →
Privacidade e Segurança → Abrir Mesmo Assim**. Na primeira vez que o daemon olhar a pasta
Downloads, o macOS pede permissão: permita, ou o daemon não vê os arquivos. Para remover:
`sh uninstall.sh` na pasta do pacote.

## Compilar a partir do código

Requisitos: Rust (edição 2024). No Linux, systemd para o daemon em segundo plano.

```sh
git clone https://github.com/Langendolff/TidyFlow.git
cd TidyFlow

cargo install --path crates/cli
mkdir -p ~/.config/tidyflow
cp config.example.toml ~/.config/tidyflow/config.toml
tidyflow check                       # valida o config
tidyflow run --dry-run               # mostra o que seria feito

tidyflow service enable              # grava a unit do systemd e liga o daemon
journalctl --user -u tidyflow -f     # logs
```

O config fica em `~/.config/tidyflow/config.toml` no Linux, em
`~/Library/Application Support/TidyFlow/config.toml` no macOS e em
`%APPDATA%\TidyFlow\config.toml` no Windows. No macOS e no Windows, o log do daemon fica
em `daemon.log`, na mesma pasta.

## Comandos

| Comando | O que faz |
|---|---|
| `tidyflow daemon` | monitora e organiza em tempo real |
| `tidyflow run [--dry-run]` | varre uma vez |
| `tidyflow organize ARQUIVO...` | organiza os arquivos dados agora (usado pelo menu de contexto) |
| `tidyflow undo [ID]` | desfaz a última ação (ou a ação ID) |
| `tidyflow history [-n N]` | últimas ações |
| `tidyflow check [ARQUIVO]` | valida o config / mostra qual regra casaria |
| `tidyflow check --clear-keep ARQUIVO` | tira a marca de desfeito (o TidyFlow volta a organizar o arquivo) |
| `tidyflow service enable` | liga o daemon no login e inicia agora (`disable`, `start`, `status`) |
| `tidyflow browser install` | registra o host da extensão de navegador (`uninstall` remove) |

Hyprland: `bind = SUPER, U, exec, tidyflow undo` para desfazer pelo teclado.

## Regras

Veja [`config.example.toml`](config.example.toml) para um config completo.

| | |
|---|---|
| **Condições** (todas precisam casar) | `ext`, `name` (glob), `name_regex`, `mime` (glob), `origin` (glob do host), `site` (glob do site da página de origem), `size_gt`, `size_lt` (`"100MB"`, `"1MiB"`), `older_than` (`"30d"`, `"12h"`) |
| **Ações** (uma por regra) | `move`, `copy`, `rename`, `trash` |
| **Variáveis** | `{year}` `{month}` `{day}` (data de modificação), `{ext}`, `{name}`, `{origin_host}`, `{site}` |

A primeira regra que casar vence. `enabled = false` pausa uma regra sem apagá-la.
Cada regra precisa de um nome único. Uma regra `rename` cujo resultado continua casando
com a própria regra renomeia o arquivo de novo a cada varredura: inclua uma
condição que exclua os nomes já renomeados.

Uma pasta `watch` que ainda não existe não é erro: `check` avisa, e o daemon passa
a monitorá-la quando ela aparecer (tenta de novo a cada 60s).

### Origem do download

No Linux, lida do atributo estendido `user.xdg.origin.url`, gravado pelo Chromium/Chrome,
por `wget --xattr`, por `curl --xattr` e, no Firefox, pela [extensão de navegador](#extensão-de-navegador).
No macOS, também de `kMDItemWhereFroms` (Safari, Chrome e Firefox gravam). No Windows,
também do `Zone.Identifier` que Edge, Chrome e Firefox gravam em cada download.

`origin` é o host de onde o arquivo foi baixado, que muitas vezes é uma CDN
(`objects.githubusercontent.com`). `site` é o host da página onde você clicou
(`github.com`), lido de `user.xdg.referrer.url`; sem ela, `site` usa a origem.

### Desfazer

Desfazer é definitivo para aquele arquivo: o TidyFlow grava o xattr
`user.tidyflow.keep` no arquivo restaurado e não mexe nele de novo. Onde o xattr
não puder ser gravado, o histórico é usado no lugar. Para o TidyFlow organizar o
arquivo de novo: `tidyflow check --clear-keep <arquivo>`. `tidyflow check <arquivo>`
mostra se o arquivo está marcado. No Windows, as marcas ficam em fluxos alternativos do
NTFS (`arquivo:tidyflow.keep`) em vez de xattr.

A lixeira é a do sistema. No macOS, desfazer um envio à lixeira só funciona para
arquivos da pasta pessoal; de outro disco, restaure pelo Finder. Os botões **Desfazer** e
**Abrir pasta** na notificação só existem no Linux; nos outros sistemas, desfaça pelo app
ou com `tidyflow undo`.

## Interface

A interface não conversa com o daemon: ela lê o mesmo histórico e edita o mesmo
`config.toml`, que o daemon recarrega sozinho. O editor mantém os comentários e a
formatação do resto do arquivo, e não grava se o arquivo mudou no disco desde que foi
aberto. Se o `config.toml` for um symlink (dotfiles), ele continua sendo.

Requisitos: `webkit2gtk-4.1` e Node 22 ou mais novo.

```sh
npm --prefix ui ci
npm --prefix ui run build
cargo build --release -p tidyflow-app --features custom-protocol
install -Dm755 target/release/tidyflow-app ~/.cargo/bin/tidyflow-app
install -Dm644 contrib/tidyflow.desktop ~/.local/share/applications/tidyflow.desktop
install -Dm644 crates/app/icons/128x128.png ~/.local/share/icons/hicolor/128x128/apps/tidyflow.png
```

Desenvolvimento: `npm --prefix ui run dev` num terminal e `cargo run -p tidyflow-app`
em outro (a janela carrega o Vite com recarga automática).

## Menu de contexto

No Windows, o `Instalar.cmd` cria **Organizar com o TidyFlow** no Explorer. No macOS, o
`install.sh` instala a Ação Rápida **Organizar com o TidyFlow** (botão direito → Ações
Rápidas). No Linux, dois itens: **Organizar com TidyFlow** (`tidyflow organize`) e **Testar no TidyFlow**
(abre a tela Testar arquivo; com o app já aberto, só troca o arquivo).

```sh
# Nautilus: aparece em Scripts → TidyFlow
mkdir -p ~/.local/share/nautilus/scripts
cp -r contrib/nautilus/TidyFlow ~/.local/share/nautilus/scripts/

# Nemo
mkdir -p ~/.local/share/nemo/actions
cp contrib/nemo/*.nemo_action ~/.local/share/nemo/actions/
```

`organize` é um pedido explícito, então ele é diferente do daemon: vale para
**qualquer pasta** (o `watch` das regras é ignorado), não espera o arquivo ficar
parado e ignora a marca de desfeito (que é removida). Cuidado com regras amplas como
`older_than` + `trash`: elas também valem para um arquivo antigo de outra pasta. A
ação vai para o histórico e pode ser desfeita pela notificação, como as outras.

## Extensão de navegador

Opcional. Informa ao TidyFlow a URL, a página de origem e o título da aba de cada
download, e avisa quando ele termina. Com ela, as regras por `origin` e `site`
funcionam no Firefox, e o daemon sabe exatamente quando pode mexer no arquivo, em
vez de adivinhar pelos arquivos `.part`.

```sh
tidyflow browser install   # o install.sh já faz isso
```

Depois, instale a extensão: no Firefox, o `.xpi` do release; no Chromium, Chrome ou
Brave, o zip do release descompactado em `chrome://extensions` (modo do
desenvolvedor → carregar sem compactação). Detalhes em [`browser/README.md`](browser/README.md).

A extensão só observa: não baixa, não move e não renomeia nada. Downloads de janelas
privadas nunca são informados. A comunicação é com o binário `tidyflow` no seu
computador, por native messaging, sem rede.

## Arquitetura

```
TidyFlow/
├── crates/
│   ├── core/   # regras, leitura de metadados, ações, histórico (SQLite), desfazer
│   ├── cli/    # binários `tidyflow` e `tidyflowd` (sem console no Windows)
│   └── app/    # binário `tidyflow-app`: Tauri 2
├── ui/         # React + Vite + Tailwind, a interface do app
├── browser/    # extensão de navegador (WebExtension, TypeScript)
└── contrib/    # instaladores (Linux, windows/, macos/), unit do systemd, Nautilus, Nemo
```

O daemon, a CLI e o app usam o mesmo `core`, então uma regra se comporta igual em todos.

**Stack:** Rust · Tauri 2 · React 19 · Vite · Tailwind · SQLite (rusqlite) · TOML

## Licença

O TidyFlow é um projeto educacional distribuído sob a [PolyForm Noncommercial 1.0.0](LICENSE).

- **Permitido sem pedir autorização:** estudar, ensinar, pesquisar, modificar e usar para fins pessoais ou em instituições de ensino, ONGs e órgãos públicos.
- **Uso comercial** (vender, incluir em produto pago, usar em empresa com fins lucrativos) só com autorização por escrito. Para isso, [abra uma issue](https://github.com/Langendolff/TidyFlow/issues) ou fale com o autor pelo [GitHub](https://github.com/Langendolff).

## Status

Versão `0.1.0`, em estágio inicial. Feito para uso pessoal; ainda tem arestas.
Sugestões e problemas são bem-vindos nas [Issues](https://github.com/Langendolff/TidyFlow/issues).

---

<p align="center">Feito por <a href="https://github.com/Langendolff">Lorenzo Langendolff</a></p>
