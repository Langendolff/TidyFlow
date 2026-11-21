# Extensão de navegador do TidyFlow

Informa ao TidyFlow, no seu computador, a URL, a página de origem e o título da aba
de cada download. Com isso:

- as regras por `origin` e `site` funcionam também no Firefox, que não grava a origem
  no arquivo;
- o daemon sabe exatamente quando o download terminou, em vez de adivinhar pelos
  arquivos `.part`.

A extensão só observa: não baixa, não move e não renomeia nada. Downloads de janelas
privadas nunca são informados. A comunicação é por native messaging com o binário
`tidyflow`, sem rede.

## Instalação

1. Registre o host no navegador (o `install.sh` já faz isso):

   ```sh
   tidyflow browser install
   ```

2. Instale a extensão:
   - **Firefox:** pela loja, em <https://addons.mozilla.org/firefox/addon/tidyflow/>
     (se atualiza por lá). Para testar sem a loja: `about:debugging` → Este Firefox → Carregar
     extensão temporária → `dist/firefox/manifest.json` (some ao fechar o navegador).
   - **Chromium, Chrome, Brave:** `chrome://extensions` → Modo do desenvolvedor →
     Carregar sem compactação → `dist/chromium/` (ou o zip do release, descompactado).

3. Clique no ícone do TidyFlow na barra: a página deve dizer "Conectado ao TidyFlow".

Navegadores em Flatpak ou Snap não enxergam o host instalado na home e ficam de fora
por enquanto.

## Desenvolvimento

```sh
npm ci
npm run build     # dist/firefox e dist/chromium
npm run lint      # oxlint, tsc e web-ext lint (rode o build antes)
npm test -- --run
npm run package   # zips do release em dist/
```

Cada release envia a versão ao AMO pelo canal *listed*, com os segredos
`AMO_JWT_ISSUER` e `AMO_JWT_SECRET`; a Mozilla revisa e publica na loja. O AMO aceita cada
versão uma vez só: não assine localmente uma versão que ainda vai sair em release.
Página da loja e passo a passo: [`AMO.md`](AMO.md).

A conversão de eventos em mensagens fica em `src/messages.ts`, sem API do navegador,
e é o que os testes cobrem. O ID fixo da extensão no Chromium vem da `key` em
`manifest.base.json`; um teste confere que ele bate com `CHROMIUM_ID` em
`crates/cli/src/browser.rs`.

## Roteiro de teste manual

Faça no Firefox e no Chromium, com o daemon rodando (`journalctl --user -u tidyflow -f`):

1. `tidyflow browser install` e a extensão carregada; a página do ícone diz "Conectado".
2. Baixe o asset de um release do GitHub (vem de `objects.githubusercontent.com`).
   `tidyflow check <arquivo>` mostra a origem, a página de origem e `site: github.com`.
3. Com uma regra `when = { site = "github.com" }`, o arquivo é movido logo que o
   download termina, sem esperar a varredura.
4. Comece um download grande e cancele: nada é movido; a linha fica `interrupted`.
5. Baixe algo numa janela privada: nenhuma linha nova em `downloads`
   (`sqlite3 ~/.local/share/tidyflow/journal.db 'select * from downloads order by id desc limit 3'`).
6. `tidyflow browser uninstall` e reinicie o navegador: um download mostra "!" no
   ícone, o arquivo baixa normalmente e o TidyFlow volta à heurística antiga.
