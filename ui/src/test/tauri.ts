// Simula o backend Tauri nos testes: cada comando responde com o handler registrado.
import { vi } from 'vitest'

type Handler = (args: Record<string, unknown>) => unknown

export const calls: { cmd: string; args: Record<string, unknown> }[] = []
let handlers: Record<string, Handler> = {}

vi.mock('@tauri-apps/api/core', () => ({
  invoke: async (cmd: string, args: Record<string, unknown> = {}) => {
    calls.push({ cmd, args })
    const h = handlers[cmd]
    if (!h) throw new Error(`comando sem mock: ${cmd}`)
    return h(args)
  },
}))

vi.mock('@tauri-apps/api/event', () => ({ listen: async () => () => {} }))

export function mockBackend(h: Record<string, Handler>) {
  handlers = h
  calls.length = 0
}

export const callsOf = (cmd: string) => calls.filter((c) => c.cmd === cmd)

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: async () => '/home/u/Desktop' }))
vi.mock('@tauri-apps/api/path', () => ({ homeDir: async () => '/home/u' }))
vi.mock('@tauri-apps/api/webview', () => ({
  getCurrentWebview: () => ({ onDragDropEvent: async () => () => {} }),
}))
