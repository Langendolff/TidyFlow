// Manifesto de atualização do Firefox (`update_url`), publicado em cada release.
// Uso: node updates.mjs <tag> > updates.json   (lê a versão de dist/firefox/manifest.json)

import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

export const GECKO_ID = 'tidyflow@langendolff.github.io'
export const REPO = 'https://github.com/Langendolff/TidyFlow'
/** Sempre o release mais recente: o Firefox consulta este endereço. */
export const UPDATE_URL = `${REPO}/releases/latest/download/updates.json`

export function updatesJson(version, tag) {
  return {
    addons: {
      [GECKO_ID]: {
        updates: [
          {
            version,
            update_link: `${REPO}/releases/download/${tag}/tidyflow-firefox-${tag.replace(/^v/, '')}.xpi`,
          },
        ],
      },
    },
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const tag = process.argv[2]
  if (!tag) throw new Error('uso: node updates.mjs <tag>')
  const { version } = JSON.parse(readFileSync('dist/firefox/manifest.json', 'utf8'))
  process.stdout.write(JSON.stringify(updatesJson(version, tag), null, 2) + '\n')
}
