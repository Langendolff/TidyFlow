# TidyFlow — Fase 1 (núcleo) — Design

Data: 2026-11-15

## Objetivo

Organizador de arquivos para Linux, em Rust, com dois objetivos: **portfólio**
(arquitetura limpa, testes, CI) e **uso diário** (confiável desde o dia 1).
Funciona em qualquer ambiente gráfico via padrões freedesktop (usuário está no
Cinnamon e vai migrar para Hyprland).

Diferenciais em relação a Mouzi, Hazelnut, organize e autotidy (roadmap completo):

1. Regras por **origem do download** (`user.xdg.origin.url`) — **Fase 1**
2. **Limpeza temporal** (idade, lixeira) — **Fase 1**
3. Interface Tauri + React, notificações com desfazer, menu de contexto (Nemo primeiro) — Fase 2
4. Regras por **conteúdo** (texto de PDF, OCR, EXIF) — Fase 3
5. **Aprende com o usuário** (sugere regras a partir de movimentos manuais) — Fase 3
6. **IA local opcional** (Ollama) para arquivos sem regra — Fase 3

Este documento cobre só a Fase 1. Cada fase seguinte terá seu próprio spec.

## Decisões

- Regras em **TOML** (`~/.config/tidyflow/config.toml`), versionável nos dotfiles.
- Comportamento padrão: **agir na hora** + notificação com botão Desfazer.
- Exclusão sempre via **lixeira** freedesktop, nunca exclusão definitiva.
- Arquitetura: workspace Rust, biblioteca `tidyflow-core` + binário único
  `tidyflow`; **SQLite (WAL) é o estado compartilhado**, sem IPC. O daemon
  recarrega o TOML quando ele muda. A Fase 2 (Tauri) usa `tidyflow-core` como biblioteca.
- Daemon roda como serviço **systemd de usuário**.

## Componentes

```
tidyflow/
├── crates/core/
│   ├── config      lê/valida TOML, expande ~ e variáveis
│   ├── facts       nome, ext, MIME, tamanho, idade, origem (xattr) — só leitura
│   ├── rules       fatos + regras → ação (função pura)
│   ├── actions     move/copy/rename/trash — único módulo que escreve no disco
│   ├── journal     SQLite — único módulo que acessa o banco
│   └── stability   detecta download em andamento
└── crates/cli/     binário `tidyflow`
    ├── daemon      watcher (notify/inotify) + timer de regras por idade
    ├── run         varredura única, `--dry-run`
    ├── undo        desfaz a última ação ou `undo <id>`
    ├── history     lista o journal
    └── check       valida o config; `check <arquivo>` mostra qual regra casaria
```

Dependências: `notify`, `rusqlite`, `serde`, `toml`, `xattr`, `infer`, `trash`,
`notify-rust`, `clap`, `regex`, `globset`, `tracing`. Dependências de teste: `tempfile`.

## Fluxo de um arquivo

1. Evento inotify `CLOSE_WRITE` ou `MOVED_TO` numa pasta monitorada.
2. **stability**: nomes `.part`, `.crdownload` ou `.tmp` são ignorados (o rename
   final gera um novo `MOVED_TO`). Depois exige tamanho e mtime iguais por 2s; se
   não estiverem, espera e confere de novo.
3. **facts**: `{path, name, ext, mime, size, age, origin_url?}`. O MIME vem de
   `infer` (conteúdo do arquivo), com fallback para a extensão.
4. **rules**: avaliadas na ordem do arquivo; **a primeira que casar vence**. Se
   nenhuma casar, nada acontece.
5. **actions**: resolve o template de destino, cria as pastas e evita colisão
   (`nome (1).ext`). No mesmo sistema de arquivos usa `rename(2)`. Em sistemas
   diferentes: copia, faz fsync, confere o tamanho e só então remove a origem.
6. **journal**: `INSERT (id, ts, rule, action, src, dst, undone=0)`.
7. Notificação D-Bus com o botão `Desfazer`, que chama `journal.undo(id)`
   (move dst → src e marca `undone=1`). Só para arquivos vindos de eventos; uma
   varredura gera um único resumo ("N arquivo(s) organizado(s)"), sem botão.

Também:

- Regras com `older_than` rodam a partir de um timer (`age_scan_interval`, padrão
  1h) que varre as pastas monitoradas e entra no fluxo a partir do passo 3.
- Na inicialização, o daemon faz uma varredura completa (arquivos que chegaram com
  o daemon parado).
- Anti-loop: eventos em caminhos que aparecem como `dst` no journal até 5s antes
  do **momento em que o evento chegou** são ignorados (o horário é registrado no
  callback do watcher, não quando a fila processa o evento).
- Desfazer é definitivo para aquele arquivo: o undo grava o xattr
  `user.tidyflow.keep` no arquivo restaurado, e o engine ignora arquivos com essa
  marca (também nas varreduras). Um download novo com o mesmo nome não tem a marca.
- Pasta `watch` inexistente não é erro: `check` mostra um aviso e o daemon tenta
  monitorá-la a cada 60s; quando ela volta, roda uma varredura.

## Sintaxe das regras

```toml
[settings]
age_scan_interval = "1h"
notify = true

[[rule]]
name   = "Faturas Nubank"
watch  = ["~/Downloads"]
when   = { origin = "*.nubank.com.br", mime = "application/pdf" }
action = { move = "~/Documentos/Finanças/{year}/" }

[[rule]]
name   = "Limpeza"
watch  = ["~/Downloads"]
when   = { older_than = "30d" }
action = { trash = true }
```

**Condições** (todas precisam casar: E lógico; para OU, escreva duas regras). Uma
regra sem nenhuma condição é erro de validação.

| Chave | Tipo | Semântica |
|---|---|---|
| `ext` | lista de strings | extensão, sem diferenciar maiúsculas/minúsculas, sem o ponto |
| `name` | glob | nome do arquivo |
| `name_regex` | regex | nome do arquivo (sintaxe do crate `regex`) |
| `mime` | glob | ex.: `image/*` |
| `origin` | glob | host da URL de origem; sem xattr de origem, a condição não casa |
| `size_gt` / `size_lt` | tamanho | `"100MB"` (base 1000; `MiB` usa base 1024) |
| `older_than` | duração | `"30d"`, `"12h"`; idade calculada pela mtime |

**Ações** (exatamente uma por regra): `move = "<dir>"`, `copy = "<dir>"`,
`rename = "<template de nome>"`, `trash = true`.

**Variáveis de template**: `{year}`, `{month}`, `{day}` (mtime do arquivo, com
zero à esquerda), `{ext}`, `{name}` (sem extensão), `{origin_host}` (`unknown`
quando o arquivo não tem origem).

**Validação**: `tidyflow check` mostra erros com o número da linha (regex inválida,
chave desconhecida, regra sem condição ou com mais de uma ação,
`age_scan_interval` zero). Pastas `watch` inexistentes aparecem como aviso. Se o daemon recarregar um config inválido, ele mantém o config anterior e
notifica o erro.

Fora da Fase 1: OU/NÃO aninhados, várias ações por regra, scripts.

## Tratamento de erros

Princípio: **nenhum erro pode resultar em perda de arquivo.** Ordem: executar →
verificar → gravar no journal. Se o processo cair entre a execução e o registro,
o pior caso é um movimento que não pode ser desfeito.

| Situação | Comportamento |
|---|---|
| Arquivo some antes da ação | Ignora, log em `debug` |
| Sem permissão ou disco cheio | Não move; erro no journal e notificação; arquivo fica onde estava |
| Cópia entre discos falha | Remove a cópia parcial; origem intacta |
| Undo com `dst` ausente | Avisa e não marca como desfeito |
| Undo com `src` ocupado | Restaura com sufixo `(1)` |
| Watch cai | Log, nova tentativa a cada 60s, notifica uma vez |
| SQLite travado | `busy_timeout` 5s; se ainda falhar, a ação já executada fica sem registro (log em `warn`), como no pior caso acima |

Logs com `tracing` no stderr (capturados pelo journald do systemd).

## Testes

- `rules`: testes de tabela (fatos + regras → ação esperada).
- `actions` + `journal`: integração com `tempfile` (mover, colisão, undo,
  fallback de cópia entre discos forçando o caminho de cópia).
- `facts`: arquivos de exemplo (PDF, PNG e um arquivo com xattr de origem).
- `config`: casos de TOML válido e inválido, verificando as mensagens.
- `daemon`: um teste de ponta a ponta (watcher em pasta temporária → arquivo chega ao destino).
- CI no GitHub Actions: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`.

## Entregáveis da Fase 1

- Workspace Rust com `core` e `cli`, comandos `daemon`, `run [--dry-run]`,
  `undo [id]`, `history`, `check [arquivo]`.
- `contrib/tidyflow.service` (unidade systemd de usuário).
- `config.example.toml` e README com instalação e exemplos.
