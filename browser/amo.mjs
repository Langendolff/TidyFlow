// Publicação no addons.mozilla.org (canal listed).
// Uso: node amo.mjs > amo-metadata.json   (passado ao `web-ext sign --amo-metadata`)

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

export function metadata() {
  return { version: { approval_notes: APPROVAL_NOTES } }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  process.stdout.write(JSON.stringify(metadata(), null, 2) + '\n')
}
