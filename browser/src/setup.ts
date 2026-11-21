import { HOST, api } from './runtime'

const status = document.getElementById('status')!
const help = document.getElementById('help')!

async function check(): Promise<void> {
  try {
    await api.runtime.sendNativeMessage(HOST, { type: 'ping' })
    status.textContent = 'Conectado ao TidyFlow. Os próximos downloads já serão informados.'
    status.className = 'ok'
    help.hidden = true
    await api.action.setBadgeText({ text: '' })
  } catch {
    status.textContent = 'O TidyFlow não foi encontrado neste computador.'
    status.className = 'bad'
    help.hidden = false
  }
}

document.getElementById('retry')!.addEventListener('click', () => void check())
void check()
