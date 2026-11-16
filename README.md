# TidyFlow

Organizador automático de arquivos para Linux. Monitora pastas como `~/Downloads`
e move, copia, renomeia ou manda para a lixeira de acordo com regras em TOML.
Todas as ações ficam num histórico e podem ser desfeitas.

**Diferenciais:** regras pela **origem do download** (o site de onde o arquivo
veio), limpeza por idade, MIME detectado pelo conteúdo, nenhuma sobrescrita e
desfazer pela notificação. Tem uma interface gráfica opcional (Tauri + React) e
itens no menu de contexto do Nautilus e do Nemo.

## Instalação

```sh
cargo install --path crates/cli
mkdir -p ~/.config/tidyflow
cp config.example.toml ~/.config/tidyflow/config.toml
tidyflow check                       # valida o config
tidyflow run --dry-run               # mostra o que seria feito
mkdir -p ~/.config/systemd/user
cp contrib/tidyflow.service ~/.config/systemd/user/
systemctl --user enable --now tidyflow
journalctl --user -u tidyflow -f     # logs
```

## Comandos

| Comando | O que faz |
|---|---|
| `tidyflow daemon` | monitora e organiza em tempo real |
| `tidyflow run [--dry-run]` | varre uma vez |
| `tidyflow organize ARQUIVO...` | organiza os arquivos dados agora (usado pelo menu de contexto) |
| `tidyflow undo [ID]` | desfaz a última ação (ou a ação ID) |
| `tidyflow history [-n N]` | últimas ações |
| `tidyflow check [ARQUIVO]` | valida o config / mostra qual regra casaria |

Hyprland: `bind = SUPER, U, exec, tidyflow undo` para desfazer pelo teclado.

## Desfazer

Desfazer é definitivo para aquele arquivo: o TidyFlow grava o xattr
`user.tidyflow.keep` no arquivo restaurado e não mexe nele de novo. Onde o xattr
não puder ser gravado, o histórico é usado no lugar. Para o TidyFlow organizar o
arquivo de novo: `setfattr -x user.tidyflow.keep <arquivo>`. `tidyflow check <arquivo>`
mostra se o arquivo está marcado.

## Regras

Condições (todas precisam casar): `ext`, `name` (glob), `name_regex`, `mime` (glob),
`origin` (glob do host), `size_gt`, `size_lt` (`"100MB"`, `"1MiB"`), `older_than`
(`"30d"`, `"12h"`).

Ações (uma por regra): `move`, `copy`, `rename`, `trash`.

Variáveis: `{year}` `{month}` `{day}` (data de modificação), `{ext}`, `{name}`, `{origin_host}`.

A primeira regra que casar vence. `enabled = false` pausa uma regra sem apagá-la.
Cada regra precisa de um nome único. Uma regra `rename` cujo resultado continua casando
com a própria regra renomeia o arquivo de novo a cada varredura: inclua uma
condição que exclua os nomes já renomeados.

Uma pasta `watch` que ainda não existe não é erro: `check` avisa, e o daemon passa
a monitorá-la quando ela aparecer (tenta de novo a cada 60s).

## Origem do download

Lida do atributo estendido `user.xdg.origin.url`. Gravado por: Chromium/Chrome,
`wget --xattr`, `curl --xattr`. Firefox: a confirmar.

## Interface

Quatro telas: **Visão geral** (números, atividade recente, pastas monitoradas,
varrer ou simular), **Regras** (editar, pausar, arrastar para reordenar),
**Histórico** (busca, filtros, desfazer) e **Testar arquivo** (o que o TidyFlow lê
de um arquivo e qual regra casaria). A interface não conversa com o daemon: ela lê
o mesmo histórico e edita o mesmo `config.toml`, que o daemon recarrega sozinho. O
editor mantém os comentários e a formatação do resto do arquivo, e não grava se o
arquivo mudou no disco desde que foi aberto. Se o `config.toml` for um symlink
(dotfiles), ele continua sendo.

Pré-requisitos: `webkit2gtk-4.1` e Node 22 ou mais novo.

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

Dois itens: **Organizar com TidyFlow** (`tidyflow organize`) e **Testar no TidyFlow**
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

