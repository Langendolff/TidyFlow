# TidyFlow — Fase 2 (interface) — Design

Data: 2026-11-16

## Objetivo

Interface gráfica (Tauri 2 + React) para ver o que o TidyFlow fez, desfazer,
editar regras e testar um arquivo, mais o menu de contexto no gerenciador de
arquivos. O daemon e a CLI da Fase 1 continuam sendo o produto principal: a
interface é opcional, e tudo o que ela faz também pode ser feito pelo TOML e
pela CLI.

Referência visual: `docs/design/tidyflow-ui.png` (fonte em `tidyflow-ui.figcraft`).
Quatro telas (Visão geral, Regras, Histórico, Testar arquivo) e a notificação.

## Decisões

- **Sem IPC, como na Fase 1.** O app usa `tidyflow-core` como biblioteca, lê o
  journal SQLite (WAL) e edita o `config.toml`; o daemon recarrega o TOML sozinho.
  Para ver ações novas, o app consulta `max(id)` do journal a cada 2s.
- **O TOML continua sendo a fonte da verdade.** O editor de regras grava com
  `toml_edit`, que preserva comentários, ordem e formatação do resto do arquivo.
  Antes de gravar, o resultado passa pelo `config::parse`; se for inválido, nada é
  gravado e o erro aparece no formulário. A gravação é atômica (arquivo temporário
  + `rename`). Se o arquivo mudou em disco desde que foi lido (mtime diferente), o
  app recarrega e pede para o usuário salvar de novo, em vez de sobrescrever.
- **Status do daemon pelo systemd**: `systemctl --user show tidyflow -p
  ActiveState,ActiveEnterTimestamp`. Sem systemd ou sem a unidade, o rodapé mostra
  "Daemon parado" e um botão para iniciar (`systemctl --user start tidyflow`).
- **"Varrer agora" e "Aplicar agora" rodam no processo do app** (`engine::scan` /
  `engine::organize`), como `tidyflow run` já faz ao lado do daemon.
- **Menu de contexto: Nautilus e Nemo**, ambos como arquivos simples que chamam o
  binário, sem extensão Python: script em `~/.local/share/nautilus/scripts/` e
  `.nemo_action` em `~/.local/share/nemo/actions/`.
- Um único app por sessão (`tauri-plugin-single-instance`): abrir de novo, ou
  chamar `tidyflow-app --test <arquivo>`, foca a janela existente.

## Componentes

```
tidyflow/
├── crates/core/           (Fase 1, com as adições abaixo)
│   ├── config/edit        toml_edit: salvar, excluir, reordenar, pausar regra
│   ├── rules::explain     resultado de cada condição, para a tela Testar
│   ├── journal            coluna size, filtros, busca, paginação, estatísticas
│   └── engine::organize   organizar um arquivo pedido explicitamente
├── crates/cli/            + comando `organize`, notificação com "Abrir pasta"
├── crates/app/            binário `tidyflow-app` (Tauri 2): comandos que embrulham o core
├── ui/                    React + TypeScript + Vite + Tailwind
└── contrib/               tidyflow.desktop, script Nautilus, .nemo_action
```

Dependências novas: `toml_edit`, `tauri`, `tauri-plugin-single-instance`,
`tauri-plugin-dialog` (escolher arquivo), `tauri-plugin-opener` (abrir pasta /
config). Front: `react`, `@tauri-apps/api`, `tailwindcss`, `@dnd-kit/sortable`
(arrastar regras), `vitest` + `@testing-library/react`.

## Mudanças no core

### Config

- Campo novo `enabled` por regra (padrão `true`). Regras com `enabled = false`
  são ignoradas pelo `rules::first_match` e aparecem como "pausada" na interface.
  `check` lista as pausadas.
- `config::edit` trabalha sobre um `toml_edit::DocumentMut`:
  - `RuleDraft { name, enabled, watch: Vec<String>, when: Vec<(chave, valor)>,
    action: (tipo, valor) }`, com os valores como o usuário escreveu (`"100MB"`,
    `"14d"`), para que o TOML gravado seja legível.
  - `save_rule(doc, index: Option<usize>, draft)` (cria no fim ou substitui),
    `delete_rule(doc, index)`, `move_rule(doc, from, to)`, `set_enabled(doc, index, bool)`.
  - `draft_of(doc, index)` lê uma regra de volta para o formulário.
  - Ao substituir uma regra, comentários acima do `[[rule]]` são mantidos.
- Nomes de regra passam a ser únicos (erro de validação), porque o journal e a
  prévia do editor identificam a regra pelo nome.

### Rules

`explain(c, f, now) -> Vec<Check>` com `Check { key, expected: String, ok: bool }`,
uma entrada por condição presente, na ordem da tabela de condições. `matches`
passa a ser `explain(...).iter().all(|c| c.ok)` para que os dois nunca divirjam.

### Journal

- Migração por `PRAGMA user_version` (0 → 1): coluna `size INTEGER` (bytes do
  arquivo no momento da ação; `NULL` nas entradas antigas e nos erros).
- `page(filter, search, before_id, limit)`: filtros `Todas`, `Movidos` (move,
  copy e rename), `Lixeira`, `Desfeitos`, `Erros`; busca por substring no nome do
  arquivo ou da regra; paginação por `id < before_id`.
- `counts(search)`: total por filtro (os números dos chips).
- `stats(now)`: organizados hoje e ontem (ações sem erro e não desfeitas, pelo dia
  local), enviados à lixeira nos últimos 7 dias, espaço liberado (soma de `size`
  de move e trash não desfeitos nos últimos 7 dias cujo `src` está numa pasta
  monitorada) e erros nos últimos 7 dias.
- `last_for_rule(name)`: última entrada com sucesso daquela regra (prévia do editor).
- `max_id()`.

### Engine

`organize(cfg, journal, path, now)`: para um arquivo escolhido pelo usuário (menu
de contexto, "Aplicar agora"). Diferenças em relação a `process`:

- considera **todas** as regras ativas, independentemente do `watch`;
- ignora a marca `user.tidyflow.keep` e a remove depois da ação (o pedido
  explícito vence o undo anterior);
- não aplica a idade mínima de 10s.

Continua com a mesma ordem executar → verificar → gravar no journal, e a ação
pode ser desfeita normalmente.

## CLI

- `tidyflow organize <ARQUIVO>...`: chama `engine::organize` e mostra uma
  notificação por arquivo (com Desfazer), ou "nenhuma regra casou".
- Notificação ganha o botão **Abrir pasta** (abre a pasta de destino com
  `xdg-open`). O texto segue o design: título "`<arquivo>` organizado", corpo
  "Movido para `<pasta>` pela regra "`<regra>`"".

## Telas

### Visão geral

- Cartões: organizados hoje (e a diferença para ontem), enviados à lixeira (7
  dias), espaço liberado (7 dias), erros (link para o Histórico filtrado).
- Atividade recente: as 6 últimas entradas, com Desfazer (ou Detalhes nos erros).
- Pastas monitoradas: por pasta, número de regras, número de arquivos e tamanho
  total; estado "monitorando" ou "aguardando pasta" (a pasta não existe). O
  "% já organizado" do design fica de fora: não há uma definição útil.
- Botões "Simular (dry-run)" (abre a lista do que seria feito, sem agir) e
  "Varrer agora".
- Dica do atalho de desfazer (texto fixo do README).

### Regras

- Lista na ordem do arquivo, com número, resumo (condições → ação) e o interruptor
  ativa/pausada. Arrastar muda a ordem (`move_rule`).
- Editor: nome, pastas monitoradas (adicionar com o seletor de pasta), condições
  (tipo + valor; o tipo de cada condição só pode aparecer uma vez), ação (Mover,
  Copiar, Renomear, Lixeira) com o campo de destino e os botões que inserem
  variáveis.
- Prévia: a última entrada de `last_for_rule` passa pelo template atual,
  mostrando origem → destino. Sem entrada, a prévia não aparece.
- Excluir pede confirmação. "Abrir config.toml" abre no editor padrão.
- Erros de validação aparecem no campo correspondente quando possível, senão no
  topo do formulário.

### Histórico

Busca, chips de filtro com contagem, tabela (ID, quando, ação, arquivo com
origem → destino, regra, Desfazer/Detalhes), "Desfazer a última" e "Carregar mais"
(50 por página). Entradas desfeitas aparecem riscadas. Detalhes mostra a mensagem
de erro completa e os caminhos.

### Testar arquivo

- Escolher ou soltar um arquivo (drag-and-drop do Tauri). Também abre por
  `tidyflow-app --test <arquivo>` (menu de contexto "Testar no TidyFlow").
- "O que foi lido": extensão, MIME, origem, tamanho, modificado, marcado para manter.
- Regras na ordem: a que vence aparece destacada; as anteriores mostram quais
  condições falharam; as seguintes mostram "não testada" com as marcas do que
  aconteceria (via `explain`). Regras pausadas aparecem como pausadas.
- Resultado com o destino calculado e "Aplicar agora" (`engine::organize`). Sem
  regra que case, mostra "nenhuma regra casaria".

### Rodapé da barra lateral

Estado do daemon (ativo há X / parado), número de pastas monitoradas.

## Atualização ao vivo

A cada 2s o app compara `max_id()` com o último valor visto; se mudou, recarrega a
tela aberta. Também recarrega o config quando o mtime do `config.toml` muda
(edição manual com o app aberto).

## Menu de contexto

Dois itens, para um ou vários arquivos:

- **Organizar com TidyFlow** → `tidyflow organize <arquivos>`
- **Testar no TidyFlow** → `tidyflow-app --test <arquivo>` (só o primeiro, se vários)

Instalação manual documentada no README (copiar os arquivos de `contrib/`). O
script do Nautilus lê `NAUTILUS_SCRIPT_SELECTED_FILE_PATHS`.

## Tratamento de erros

| Situação | Comportamento |
|---|---|
| Journal não existe ainda | Telas vazias com "Nenhuma ação ainda"; o arquivo é criado normalmente |
| Config inválido ao abrir o app | Tela de Regras mostra o erro com a linha e o botão "Abrir config.toml"; as outras telas funcionam |
| Config mudou em disco durante a edição | Não grava; recarrega e avisa |
| Undo falha | Toast com a mensagem do core; a linha continua como estava |
| Daemon parado | Aviso no rodapé com botão para iniciar; o app continua funcionando |

## Testes

- Core: `config::edit` (preserva comentários, reordena, pausa, rejeita draft
  inválido sem tocar no arquivo, detecta arquivo alterado), `explain` (tabela
  igual à de `matches`), journal (migração de um banco da Fase 1, filtros, busca,
  paginação, stats com dias locais), `organize` (ignora `watch`, remove a marca keep).
- CLI: `organize` em pasta temporária.
- App: os comandos Tauri são funções finas sobre o core, testadas pelo core.
- UI: Vitest + Testing Library nos componentes com lógica (editor de condições,
  resumo da regra, filtros do histórico), com a API do Tauri simulada.
- CI: novo job com `npm ci`, `npm run lint`, `npm test`, `npm run build`; o job
  Rust instala `libwebkit2gtk-4.1-dev` para compilar o crate `app`.

## Fora da Fase 2

Bandeja do sistema, editor de `[settings]` na interface, empacotamento
(AppImage/.deb/AUR), temas claro/escuro (segue o design, claro com barra escura),
i18n.

## Entregáveis da Fase 2

- Adições ao core e à CLI descritas acima.
- `crates/app` + `ui/` com as quatro telas.
- `contrib/tidyflow.desktop`, `contrib/nautilus/TidyFlow/` (um script por item), `contrib/nemo/*.nemo_action`.
- README com a instalação do app e do menu de contexto.
