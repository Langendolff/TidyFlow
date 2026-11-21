# Publicar no addons.mozilla.org

A extensão sai pelo canal **listed** do AMO: a Mozilla revisa cada versão e a publica em
<https://addons.mozilla.org/firefox/addon/tidyflow/>; o Firefox atualiza por lá.

## Uma vez só: a página da loja

No [Developer Hub](https://addons.mozilla.org/developers/addons), abra o TidyFlow (ele já
existe por causa da versão 0.1.0 assinada no canal unlisted) e preencha **Editar página do
produto**. Os textos abaixo estão prontos para colar.

- **Nome:** TidyFlow
- **URL (slug):** `tidyflow` (os READMEs apontam para ela)
- **Categoria:** Gerenciamento de downloads (Download Management)
- **Site:** https://github.com/Langendolff/TidyFlow
- **Suporte:** https://github.com/Langendolff/TidyFlow/issues
- **Licença:** veja [Licença](#licença) abaixo
- **Política de privacidade:** o texto de [`PRIVACY.md`](../PRIVACY.md)
- **Capturas de tela** (1280×800, em [`docs/images/amo/`](../docs/images/amo/)), com as legendas:
  1. `1-conectado.png`: "A extensão conectada ao TidyFlow. Nada sai do seu computador."
  2. `2-biblioteca.png`: "A Biblioteca do TidyFlow mostra de qual site veio cada download."
  3. `3-regras.png`: "Regras por site: PDFs do banco vão direto para Documentos/Finanças."

**Resumo (pt-BR, até 250 caracteres):**
> Diz ao TidyFlow, no seu computador, de onde veio cada download, para as suas regras
> organizarem o arquivo pelo site. Requer o app TidyFlow instalado. Nada sai do computador.

**Summary (en-US):**
> Tells TidyFlow, on your computer, where each download came from, so your rules can
> organize files by website. Requires the TidyFlow desktop app. Nothing leaves your computer.

**Descrição (pt-BR):**
> O TidyFlow organiza sua pasta de Downloads sozinho, por regras. Esta extensão informa ao
> app a URL, a página de origem e o título da aba de cada download, e avisa quando ele
> termina. Assim funcionam regras como "PDFs que vieram do site do banco vão para
> Documentos/Finanças".
>
> **Requer o app TidyFlow** (Linux, Windows ou macOS), gratuito, com código-fonte disponível:
> https://github.com/Langendolff/TidyFlow/releases. Sem ele, a extensão não faz nada.
>
> A extensão só observa: não baixa, não move e não renomeia arquivos. Os dados vão apenas
> para o programa no seu computador, por native messaging, sem rede. Downloads de janelas
> privadas nunca são informados.

**Description (en-US):**
> TidyFlow keeps your Downloads folder organized on its own, by rules. This extension tells
> the app the URL, the referring page and the tab title of each download, and when it
> finishes, so rules like "PDFs from my bank's website go to Documents/Finance" work.
>
> **Requires the TidyFlow desktop app** (Linux, Windows or macOS), free, with source code available:
> https://github.com/Langendolff/TidyFlow/releases. Without it, the extension does nothing.
>
> The extension only observes: it never downloads, moves or renames files. Data goes only to
> the program on your computer, via native messaging, with no network access. Downloads
> from private windows are never reported.

## Licença

O TidyFlow usa a PolyForm Noncommercial 1.0.0, que não está na lista do AMO. Na versão,
em **Licença**, escolha a opção de licença personalizada (*Custom License*, ou "Outra"):

- **Nome:** `PolyForm Noncommercial 1.0.0`
- **Texto:** o conteúdo inteiro do [`LICENSE`](../LICENSE)

Ela não é de código aberto no sentido da OSI, porque proíbe uso comercial: por isso os
textos da loja dizem "código-fonte disponível", não "código aberto". A Mozilla aceita
licenças personalizadas, inclusive restritivas. O que ela exige é o código-fonte legível
para revisar, e esse vai junto em cada envio.

## A cada versão: automático

Uma tag `v*` roda o `release.yml`, que envia a versão com `web-ext sign --channel=listed`,
o código-fonte (`browser/` + `crates/cli/Cargo.toml`) e as notas para o revisor
(`amo.mjs`). O workflow não espera a revisão. Acompanhe o status no Developer Hub; a
Mozilla manda e-mail quando aprova ou pede mudanças.

A versão vem de `crates/cli/Cargo.toml`, e cada número só pode ir ao AMO uma vez. A 0.1.0
já foi usada.
