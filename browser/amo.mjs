// Publicação no addons.mozilla.org (canal listed).
// Uso: node amo.mjs > amo-metadata.json   (passado ao `web-ext sign --amo-metadata`)

import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

export const GECKO_ID = 'tidyflow@langendolff.github.io'
export const REPO = 'https://github.com/Langendolff/TidyFlow'

/** Lido pelo revisor do AMO em cada versão. */
export const APPROVAL_NOTES = `TidyFlow is a local, offline file organizer (${REPO}).
This extension only observes downloads (downloads API) and sends the URL, referrer page and tab
title of each one, via native messaging, to the TidyFlow binary on the same computer
(host "io.github.langendolff.tidyflow"). Nothing is sent over the network; private-window
downloads are never reported. Without the desktop app installed, the extension does nothing
except show "not connected" on its page.

How to test: install a release from ${REPO}/releases (Linux: install.sh; Windows: Instalar.cmd;
macOS: install.sh), which registers the native host, then download any file.

Source build (the submitted source zip is the "browser" folder of the repository plus
crates/cli/Cargo.toml, which holds the version): Node 22, then
  cd browser && npm ci && npm run build
The Firefox build is in browser/dist/firefox (esbuild bundles src/*.ts; nothing is minified).`

const SUMMARY = {
  'en-US':
    'Tells TidyFlow, on your computer, where each download came from, so your rules can organize files by website. Requires the TidyFlow desktop app. Nothing leaves your computer.',
  'pt-BR':
    'Diz ao TidyFlow, no seu computador, de onde veio cada download, para as suas regras organizarem o arquivo pelo site. Requer o app TidyFlow instalado. Nada sai do computador.',
}

const DESCRIPTION = {
  'en-US': `TidyFlow keeps your Downloads folder organized on its own, by rules. This extension tells the app the URL, the referring page and the tab title of each download, and when it finishes, so rules like "PDFs from my bank's website go to Documents/Finance" work.

<b>Requires the TidyFlow desktop app</b> (Linux, Windows or macOS), free, with source code available: ${REPO}/releases. Without it, the extension does nothing.

The extension only observes: it never downloads, moves or renames files. Data goes only to the program on your computer, via native messaging, with no network access. Downloads from private windows are never reported.`,
  'pt-BR': `O TidyFlow organiza sua pasta de Downloads sozinho, por regras. Esta extensão informa ao app a URL, a página de origem e o título da aba de cada download, e avisa quando ele termina. Assim funcionam regras como "PDFs que vieram do site do banco vão para Documentos/Finanças".

<b>Requer o app TidyFlow</b> (Linux, Windows ou macOS), gratuito, com código-fonte disponível: ${REPO}/releases. Sem ele, a extensão não faz nada.

A extensão só observa: não baixa, não move e não renomeia arquivos. Os dados vão apenas para o programa no seu computador, por native messaging, sem rede. Downloads de janelas privadas nunca são informados.`,
}

/** Metadados do `web-ext sign --amo-metadata`: página da loja e dados da versão. */
export function metadata(licenseText = readFileSync(new URL('../LICENSE', import.meta.url), 'utf8')) {
  return {
    categories: { firefox: ['download-management'] },
    summary: SUMMARY,
    description: DESCRIPTION,
    homepage: { url: { 'en-US': REPO } },
    support_url: { url: { 'en-US': `${REPO}/issues` } },
    version: {
      // A PolyForm Noncommercial não está na lista do AMO: vai como licença personalizada.
      custom_license: {
        name: { 'en-US': 'PolyForm Noncommercial 1.0.0' },
        text: { 'en-US': licenseText },
      },
      approval_notes: APPROVAL_NOTES,
    },
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  process.stdout.write(JSON.stringify(metadata(), null, 2) + '\n')
}
