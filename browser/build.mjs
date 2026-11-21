// Gera dist/firefox e dist/chromium a partir do mesmo código.
// `--package` também cria os arquivos do release em dist/.

import { build } from 'esbuild'
import { execFileSync } from 'node:child_process'
import { cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { GECKO_ID, UPDATE_URL } from './updates.mjs'

const version = readFileSync('../crates/cli/Cargo.toml', 'utf8').match(/^version = "([^"]+)"/m)[1]
const base = JSON.parse(readFileSync('manifest.base.json', 'utf8'))

const targets = {
  firefox: (m) => {
    delete m.key
    m.background = { scripts: ['background.js'] }
    m.browser_specific_settings = {
      gecko: {
        id: GECKO_ID,
        strict_min_version: '142.0',
        // Distribuição fora da loja: o Firefox procura versões novas aqui.
        update_url: UPDATE_URL,
        // Os dados só vão para o app local, pelo native messaging.
        data_collection_permissions: { required: ['none'] },
      },
    }
    return m
  },
  chromium: (m) => {
    m.background = { service_worker: 'background.js' }
    m.minimum_chrome_version = '116'
    return m
  },
}

for (const [name, adapt] of Object.entries(targets)) {
  const out = `dist/${name}`
  rmSync(out, { recursive: true, force: true })
  mkdirSync(out, { recursive: true })
  await build({
    entryPoints: ['src/background.ts', 'src/setup.ts'],
    bundle: true,
    format: 'iife',
    target: 'es2022',
    outdir: out,
    logLevel: 'warning',
  })
  cpSync('src/setup.html', `${out}/setup.html`)
  cpSync('icons', `${out}/icons`, { recursive: true })
  const manifest = adapt({ ...structuredClone(base), version })
  writeFileSync(`${out}/manifest.json`, JSON.stringify(manifest, null, 2) + '\n')
}

if (process.argv.includes('--package')) {
  for (const name of Object.keys(targets)) {
    const zip = `../tidyflow-${name}-${version}.zip`
    rmSync(`dist/${zip.slice(3)}`, { force: true })
    execFileSync('zip', ['-qr', zip, '.'], { cwd: `dist/${name}` })
  }
  console.log(`dist/tidyflow-{firefox,chromium}-${version}.zip`)
}
