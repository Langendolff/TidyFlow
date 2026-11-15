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

A primeira regra que casar vence. Uma regra `rename` cujo resultado continua casando
com a própria regra renomeia o arquivo de novo a cada varredura: inclua uma
condição que exclua os nomes já renomeados.

Uma pasta `watch` que ainda não existe não é erro: `check` avisa, e o daemon passa
a monitorá-la quando ela aparecer (tenta de novo a cada 60s).

## Origem do download

Lida do atributo estendido `user.xdg.origin.url`. Gravado por: Chromium/Chrome,
`wget --xattr`, `curl --xattr`. Firefox: a confirmar.
