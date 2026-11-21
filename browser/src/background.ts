// Escuta os downloads e informa o TidyFlow pelo host de native messaging.
// A extensão só observa: não baixa, não move e não renomeia nada.

import { onChanged, onCreated, titleFor, type BrowserName, type DownloadLike, type HostMessage } from './messages'
import { HOST, api, isFirefox } from './runtime'

const BROWSER: BrowserName = isFirefox ? 'firefox' : 'chromium'

// Ids que já receberam `started`. Em storage.session porque o service worker do
// Chromium pode ser encerrado entre um evento e outro.
const KEY = 'started'

async function startedIds(): Promise<number[]> {
  const got = await api.storage.session.get(KEY)
  return (got[KEY] as number[] | undefined) ?? []
}

async function setStarted(id: number, on: boolean): Promise<void> {
  const ids = (await startedIds()).filter((x) => x !== id)
  if (on) ids.push(id)
  await api.storage.session.set({ [KEY]: ids.slice(-200) })
}

async function pageTitle(referrer: string | undefined): Promise<string | null> {
  if (!referrer) return null
  try {
    return titleFor(referrer, await api.tabs.query({}))
  } catch {
    return null
  }
}

async function send(msg: HostMessage): Promise<void> {
  try {
    await api.runtime.sendNativeMessage(HOST, msg)
    await api.action.setBadgeText({ text: '' })
  } catch (e) {
    // Host não instalado: avisa no ícone. Nada é guardado para reenviar.
    console.warn('TidyFlow: host indisponível', e)
    await api.action.setBadgeText({ text: '!' })
    await api.action.setBadgeBackgroundColor({ color: '#B42318' })
  }
}

async function dispatch(msgs: HostMessage[]): Promise<void> {
  for (const m of msgs) {
    await send(m)
    if (m.type === 'started') await setStarted(m.id, true)
    if (m.type === 'finished' || m.type === 'interrupted') await setStarted(m.id, false)
  }
}

// Registrados no topo: o service worker precisa deles na primeira execução.
api.downloads.onCreated.addListener(async (d) => {
  const item = d as unknown as DownloadLike
  const msg = onCreated(item, BROWSER, await pageTitle(item.referrer))
  if (msg) await dispatch([msg])
})

api.downloads.onChanged.addListener(async (delta) => {
  const [found] = await api.downloads.search({ id: delta.id })
  const item = found as unknown as DownloadLike | undefined
  const started = (await startedIds()).includes(delta.id)
  const title = started ? null : await pageTitle(item?.referrer)
  await dispatch(onChanged(delta, item, BROWSER, started, title))
})

api.action.onClicked.addListener(() => {
  void api.tabs.create({ url: api.runtime.getURL('setup.html') })
})
