import { listen } from '@tauri-apps/api/event'
import { useCallback, useEffect, useState } from 'react'
import { api, type Filter } from './api'
import { ToastProvider } from './components/feedback'
import { Sidebar, type Screen } from './components/Sidebar'
import { useLive } from './hooks/useLive'
import { History } from './screens/History'
import { Library } from './screens/Library'
import { Overview } from './screens/Overview'
import { Rules } from './screens/Rules'
import { TestFile } from './screens/TestFile'

export interface Go {
  (screen: Screen, opts?: { filter?: Filter; file?: string }): void
}

export default function App() {
  const [screen, setScreen] = useState<Screen>('overview')
  const [filter, setFilter] = useState<Filter>('all')
  const [file, setFile] = useState<string | null>(null)
  const version = useLive()

  const go: Go = useCallback((s, opts) => {
    if (s === 'history') setFilter(opts?.filter ?? 'all')
    if (opts?.file) setFile(opts.file)
    setScreen(s)
  }, [])

  // Menu de contexto "Testar no TidyFlow": na abertura (--test) ou com o app já aberto.
  useEffect(() => {
    api.initialTestFile().then((f) => f && go('test', { file: f }), () => {})
    const off = listen<string>('test-file', (e) => go('test', { file: e.payload }))
    return () => {
      void off.then((f) => f())
    }
  }, [go])

  return (
    <ToastProvider>
      <div className="flex h-full">
        <Sidebar screen={screen} onNavigate={(s) => go(s)} />
        <main className="min-w-0 flex-1 overflow-auto px-8 pt-7 pb-8">
          {screen === 'overview' && <Overview version={version} go={go} />}
          {screen === 'library' && <Library version={version} go={go} />}
          {screen === 'rules' && <Rules version={version} />}
          {screen === 'history' && <History key={filter} version={version} initialFilter={filter} />}
          {screen === 'test' && <TestFile file={file} onFile={setFile} />}
        </main>
      </div>
    </ToastProvider>
  )
}
