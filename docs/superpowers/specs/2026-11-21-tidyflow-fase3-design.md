# TidyFlow — Fase 3 (navegador, biblioteca, sugestões) — Design

Data: 2026-11-21

## Objetivo

Fazer do TidyFlow o lugar onde o usuário vê e organiza tudo o que baixou, sem
virar um gerenciador de arquivos nem um motor de download próprio. Três partes,
cada uma entregue e mesclada separadamente:

- **3a. Extensão de navegador.** Firefox e Chromium avisam o TidyFlow sobre cada
  download: URL, página de origem e título. Resolve a origem no Firefox, que não
  grava `user.xdg.origin.url`, e substitui a heurística do marcador vazio por um
  sinal exato de "ainda baixando".
- **3b. Biblioteca.** Uma tela com tudo o que chegou nas pastas monitoradas: de
  onde veio, onde está agora e o que o TidyFlow fez com cada arquivo.
- **3c. Sugestões.** O TidyFlow agrupa os arquivos que nenhuma regra pega e sugere
  regras. Com o Ollama rodando, um modelo local escolhe os nomes e as pastas. Nada
  é aplicado sem o usuário aceitar.

Continua valendo tudo das fases anteriores: o TOML é a fonte da verdade, tudo é
desfazível, nada é sobrescrito e nada sai da máquina.

## Decisões

- **Sem IPC, como antes.** O host de native messaging (`tidyflow native-host`) é
  aberto pelo navegador e grava no mesmo SQLite do journal (WAL). O daemon e o app
  leem de lá. Não há socket nem servidor.
- **A extensão só observa.** Ela não baixa, não move e não renomeia nada. O download
  continua sendo do navegador, e as regras continuam rodando no daemon.
- **Os metadados também vão para xattrs.** Quando o download termina, o host grava
  `user.xdg.origin.url` (se ainda não existir) e `user.xdg.referrer.url`. Assim,
  `Facts::read` e as regras atuais funcionam sem mudança, e a informação acompanha
  o arquivo quando ele é movido.
- **Janelas privadas são ignoradas.** Downloads com `incognito = true` não são
  enviados ao host.
- **Uma extensão, dois manifests.** O mesmo código WebExtension (MV3, TypeScript
  sem framework) com um manifest para Firefox e outro para Chromium, gerados por
  um script de build. No Firefox, o `.xpi` é assinado como *unlisted* pelo AMO e vai
  no release. No Chromium, por enquanto, a instalação é como extensão descompactada,
  com ID fixo pela `key` no manifest. Publicar na Chrome Web Store fica para depois.
- **Ollama é opcional e desligado por padrão.** Sem ele, as sugestões saem só do
  agrupamento, com nome e pasta genéricos. Com ele, o modelo dá nomes e destinos
  melhores. O padrão é `http://localhost:11434`. Se o usuário apontar para um host
  que não é loopback, o app avisa que os nomes dos arquivos sairão da máquina.
- **Só metadados vão para o modelo.** Nome, extensão, MIME, tamanho, site e datas.
  O conteúdo dos arquivos (texto de PDF, OCR, EXIF) fica fora desta fase.
- **Sugestão só vale se passar no próprio TidyFlow.** Toda resposta do modelo vira
  um `RuleDraft`, passa pelo `config::parse` e é simulada sobre os arquivos reais.
  Sugestões inválidas ou que não casam com nenhum arquivo do grupo são descartadas.

## Componentes

```
tidyflow/
├── crates/core/
│   ├── journal           migrações v3 (downloads) e v4 (suggestions)
│   ├── downloads         registro de downloads e localização atual de cada arquivo
│   ├── facts / rules     xattr do referrer, condição `site`, variável {site}
│   └── suggest/          agrupamento, cliente do Ollama, validação e simulação
├── crates/cli/           + `native-host`, `browser install|uninstall`, `suggest`
├── crates/app/           + comandos da Biblioteca e das Sugestões
├── ui/                   + telas Biblioteca e Sugestões
└── browser/              extensão WebExtension (TypeScript) + script de build
```

Dependências novas: `ureq` (cliente HTTP do Ollama, atrás da feature `ai` do core,
ligada na CLI e no app) e `serde_json` no core. Extensão: `typescript` e
`web-ext` (lint, empacotamento e assinatura).

## 3a. Extensão de navegador

### Extensão

Permissões: `downloads`, `nativeMessaging`, `tabs` (só para ler o título da aba
de onde o download saiu).

Eventos enviados ao host, um por mensagem:

| Evento do navegador | Mensagem |
|---|---|
| `downloads.onCreated` | `started { id, path, url, final_url, referrer, page_title, mime, browser, started_at }` |
| `downloads.onChanged` com `state = complete` | `finished { id, path, size }` |
| `downloads.onChanged` com `state = interrupted` | `interrupted { id, path, error }` |
| `downloads.onChanged` com `filename` novo | `renamed { id, path }` |

`page_title` vem da aba cuja URL é igual ao `referrer`, quando existe uma. Senão,
fica vazio. O `id` é o do navegador e, junto com `browser`, identifica o download.

Se o host não estiver instalado, a extensão mostra um aviso no ícone e uma página
com o comando `tidyflow browser install`. Ela não guarda eventos para reenviar depois.

### Host de native messaging

`tidyflow native-host` lê e escreve o protocolo padrão: 4 bytes de tamanho em
little-endian, seguidos de JSON. Para cada mensagem:

- `started`: insere uma linha em `downloads` com `state = 'in_progress'`.
- `finished`: primeiro atualiza `state = 'complete'`, `finished_at` e `size`;
  depois grava os xattrs no arquivo. Nessa ordem, quando o daemon recebe o evento
  do xattr, o registro já diz que o download terminou. Além de origem e referrer
  (só se ainda não existirem, porque o Chromium já grava os dois), o host sempre
  grava `user.tidyflow.download` com o id da linha. Isso garante um evento de
  metadados mesmo quando os outros xattrs já existiam, e liga o arquivo à linha da
  biblioteca. Se o arquivo não existir mais, marca `state = 'gone'`.
- `interrupted`: `state = 'interrupted'`.
- `renamed`: atualiza `path`.

Responde `{ ok: true }` ou `{ ok: false, error }`. Um erro numa mensagem não
derruba o host.

`tidyflow browser install` grava os manifests do host em
`~/.mozilla/native-messaging-hosts/`, `~/.config/chromium/NativeMessagingHosts/`
e `~/.config/google-chrome/NativeMessagingHosts/`, apontando para o binário atual,
com o ID da extensão em `allowed_extensions` (Firefox) ou `allowed_origins`
(Chromium). Também cobre LibreWolf (`~/.librewolf/`) e Brave, quando a pasta de
configuração do navegador existe. O manifest não aceita argumentos, então ele
aponta para um script `~/.local/share/tidyflow/native-host` que faz
`exec <binário> native-host "$@"`. Nome do host: `io.github.langendolff.tidyflow`.
`uninstall` remove esses arquivos. O `install.sh` chama `browser install`.
Navegadores em Flatpak ou Snap ficam fora desta fase.

### Daemon

- **Download em andamento pelo registro.** Antes de processar um arquivo, o daemon
  consulta `downloads`. Se existe uma linha `in_progress` para aquele caminho,
  iniciada há menos de 24h, ele pula o arquivo como "ainda baixando". A heurística
  atual de `stability::download_in_progress` continua valendo para quem não tem a
  extensão.
- **Retomar quando o download termina.** O daemon passa a tratar também eventos de
  metadados (`ModifyKind::Metadata`, gerados quando o host grava o xattr), mas só
  para arquivos com uma linha `complete` em `downloads` que ainda não foram
  processados depois do `finished_at`. A varredura periódica continua como rede
  de segurança.

### Regras

- Condição nova `site`: glob sobre o host da página de origem (o `referrer`). Ela
  existe porque o arquivo muitas vezes vem de uma CDN
  (`objects.githubusercontent.com`), enquanto o site que o usuário reconhece é
  outro (`github.com`). Sem referrer, `site` usa o host da origem.
- Variável de template `{site}`, com a mesma regra.
- `Facts` ganha `referrer_url`, lido de `user.xdg.referrer.url`. A tela Testar
  arquivo mostra o referrer e o site.

## 3b. Biblioteca

### Dados

Tabela `downloads` (migração v3), que também serve de biblioteca:

```
id, source ('browser' | 'folder'), browser, browser_id,
path            caminho onde o arquivo chegou
current_path    onde ele está agora (ou onde estava antes da lixeira)
url, referrer, page_title, mime, size,
state           'in_progress' | 'complete' | 'interrupted' | 'trashed' | 'gone'
started_at, finished_at
```

- Arquivos que chegam numa pasta monitorada sem uma linha correspondente (Chromium
  sem a extensão, `wget`, cópia manual) ganham uma linha `source = 'folder'` quando
  o daemon os vê pela primeira vez, com a origem lida do xattr.
- `current_path` acompanha o arquivo: quando o engine grava uma ação no journal, a
  mesma transação atualiza as linhas cujo `current_path` era o `src`. Lixeira grava
  `state = 'trashed'` e mantém o último caminho em `current_path`. O desfazer faz o
  caminho inverso.
- O `sweep_gone` da fase 2 também marca como `gone` as linhas cujo `current_path`
  não existe mais.
- Para onde o usuário levou um arquivo à mão não é rastreado nesta fase: exigiria
  monitorar a home inteira. O arquivo aparece como "não encontrado".

### Tela

Item novo na barra lateral, entre Visão geral e Regras.

- Lista paginada (50 por vez, mais recentes primeiro) com nome, site, tipo,
  tamanho, quando chegou, onde está agora e a última ação do TidyFlow sobre ele.
- Filtros: período, site (os mais frequentes viram chips), tipo (documento,
  imagem, vídeo, áudio, compactado, instalador, outro, pelo MIME), estado (na pasta
  de downloads, organizado, na lixeira, não encontrado, baixando).
- Busca por substring no nome, na URL e no título da página.
- Ações por item: abrir, abrir a pasta, testar no TidyFlow, organizar agora,
  desfazer a última ação, copiar a URL, abrir a página de origem no navegador.
- Downloads em andamento aparecem no topo com "baixando", sem progresso (a
  extensão não envia progresso nesta fase).

## 3c. Sugestões

### Agrupamento (sem IA)

Os candidatos são os arquivos nas pastas monitoradas que nenhuma regra pega. A
biblioteca completa os metadados (site, título da página). Cada arquivo entra em
no máximo um grupo, e os grupos são formados nesta ordem:

1. Mesmo `site` e mesma família de MIME (`application/pdf`, `image/*`...).
2. Mesmo padrão de nome: dígitos viram `#` e os tokens iniciais em comum são o
   padrão (`fatura-####-##.pdf`).
3. Mesma extensão de instalador ou compactado (`deb`, `AppImage`, `iso`, `zip`...).

Só viram sugestão os grupos com pelo menos 3 arquivos. Cada grupo tem uma
assinatura estável (critério + valor), usada para não repetir sugestões já
dispensadas.

Também entram como sugestão as **regras que o usuário desfaz muito**: 3 ou mais
desfazeres e pelo menos 30% das ações da regra nos últimos 30 dias. A sugestão é
"rever a regra", com os arquivos desfeitos como exemplo, sem proposta automática.

Sem Ollama, a proposta de cada grupo é montada por modelo fixo: as condições do
critério e `move` para a pasta XDG do tipo (`~/Documentos/{site}/`,
`~/Imagens/{site}/`... lidas de `~/.config/user-dirs.dirs`), pela família de MIME.
Instaladores e compactados vão para subpastas da própria pasta monitorada. O nome é descritivo ("PDFs de nubank.com.br").

### Ollama

Configuração no `config.toml`:

```toml
[ai]
enabled = false
url     = "http://localhost:11434"
model   = "qwen2.5:7b"
```

- `GET /api/tags` diz se o Ollama está rodando e quais modelos existem. O app
  mostra o estado e deixa escolher o modelo. A escolha é gravada com `toml_edit`.
- Uma chamada a `POST /api/chat` por análise, com `stream: false`, `format` igual
  ao JSON Schema da resposta e temperatura baixa. Tempo limite de 120s.
- O prompt leva: os grupos (critério, quantidade, até 10 nomes de exemplo, sites,
  tamanhos), as regras atuais resumidas, as pastas de primeiro e segundo nível da
  home e as variáveis de template disponíveis. Pede no máximo uma regra por grupo,
  com preferência por pastas que já existem.
- Resposta esperada: `{ suggestions: [{ group, name, when, action, reason }] }`,
  com `when` e `action` no formato do `RuleDraft`.
- Se o Ollama falhar (fora do ar, tempo esgotado, JSON inválido), as sugestões do
  agrupamento são mostradas mesmo assim, com o aviso "IA indisponível".

### Validação e simulação

Para cada proposta, com IA ou sem:

1. Converter em `RuleDraft` e montar o config com a regra nova. Se o nome já
   existir, recebe um sufixo. O resultado passa pelo `config::parse`; se falhar, a
   proposta é descartada (com IA, cai para a proposta sem IA do mesmo grupo).
2. Posição: antes da primeira regra atual que casaria com algum arquivo do grupo.
   Se nenhuma casa, vai para o fim.
3. Simular sobre os arquivos do grupo e das pastas monitoradas: quantos arquivos a
   regra pegaria, destinos de exemplo e quais arquivos ela tiraria de outras
   regras ("3 arquivos deixam de ir para a lixeira pela regra Limpeza geral").
4. Se ela não pegar nenhum arquivo do grupo, a proposta é descartada.

### Dados

Tabela `suggestions` (migração v4): `id, signature, created_at, status ('new' |
'accepted' | 'dismissed'), payload (JSON)`. Uma assinatura dispensada não é
sugerida de novo por 90 dias. Uma análise nova substitui as sugestões `new`
anteriores.

### Tela

Item novo na barra lateral: Sugestões, com o número de sugestões novas.

- Cartão de estado da IA: desligada, Ollama não encontrado, ou pronta (com o
  modelo). Botão para ligar e seletor de modelo.
- Botão "Analisar agora". A análise roda numa thread do app, com indicador de
  progresso e cancelamento. Ela só roda quando o usuário pede, nunca sozinha.
- Cada sugestão: nome, resumo (condições → ação), motivo, quantos arquivos pega,
  exemplos de origem → destino, conflitos com regras atuais. Ações: **Aceitar**
  (`Editor::save_rule` na posição calculada), **Editar antes** (abre o editor de
  regras preenchido) e **Dispensar**.
- Aceitar não move nada na hora. A regra passa a valer pelo daemon, e o cartão
  oferece "Aplicar agora aos N arquivos", que usa `engine::process` (respeita
  `watch` e a marca de desfeito) e fica no histórico como qualquer ação.

### CLI

`tidyflow suggest [--no-ai] [--json]` roda a mesma análise e imprime as sugestões
com os blocos TOML prontos para colar. Não grava nada no config.

## Tratamento de erros

| Situação | Comportamento |
|---|---|
| Host não instalado | A extensão avisa no ícone; o TidyFlow funciona como antes |
| Extensão sem o TidyFlow rodando | O host grava no SQLite normalmente; o daemon processa quando subir (varredura inicial) |
| Navegador fecha no meio do download | A linha fica `in_progress`; depois de 24h o daemon a ignora e segue a heurística antiga |
| xattr não suportado no destino | Os metadados ficam só no SQLite; `site` e `origin` usam o registro como alternativa |
| Ollama fora do ar ou lento | Sugestões sem IA, com aviso |
| Resposta do modelo inválida | Proposta descartada, cai para a versão sem IA do grupo |
| Config mudou durante "Aceitar" | Mesmo comportamento do editor de regras da fase 2: recarrega e avisa |

## Testes

- Core: migrações v3 e v4 a partir de um banco v2; `current_path` acompanhando
  move, rename, lixeira e desfazer; condição `site` e `{site}` com e sem referrer;
  agrupamento (tabelas de nomes → padrão, limiar de 3, assinaturas estáveis);
  posição e conflitos da simulação; descarte de propostas inválidas; cliente do
  Ollama contra um servidor HTTP falso em `TcpListener` (resposta boa, JSON
  inválido, tempo esgotado, 404 de modelo).
- CLI: `native-host` com mensagens no protocolo real via stdin/stdout
  (started → finished grava xattr e estado); `browser install` em uma home
  temporária; daemon pulando um arquivo `in_progress` e processando depois do
  `finished`.
- Extensão: a conversão de eventos do navegador em mensagens é uma função pura,
  testada com Vitest. O fluxo completo é testado à mão no Firefox e no Chromium,
  com o roteiro no README da extensão.
- UI: Biblioteca (filtros, busca, estados) e Sugestões (aceitar, dispensar,
  estado da IA) com a API do Tauri simulada.
- CI: job da extensão com `npm ci`, lint, testes e `web-ext lint`.

## Fora da Fase 3

- Baixar por URL dentro do app (via `aria2c` ou `yt-dlp`) e progresso de download.
  Candidato para a fase 4.
- Ler o conteúdo dos arquivos (texto de PDF, OCR, EXIF) para regras ou sugestões.
- Rastrear arquivos que o usuário move à mão para fora das pastas monitoradas.
- Publicação na Chrome Web Store e na listagem pública do AMO.
- Sugestões automáticas em segundo plano.

## Entregáveis

Em três PRs, nesta ordem:

1. **3a:** `browser/`, `tidyflow native-host`, `tidyflow browser install|uninstall`,
   migração v3, condição `site`, mudanças no daemon, README (seção do navegador,
   com a origem no Firefox resolvida).
2. **3b:** registro de arquivos `source = 'folder'`, `current_path`, tela Biblioteca.
3. **3c:** `core::suggest`, migração v4, `[ai]` no config, `tidyflow suggest`, tela
   Sugestões, README.
