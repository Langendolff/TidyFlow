import { invoke } from '@tauri-apps/api/core'
import { useEffect, useState } from 'react'

export default function App() {
  const [pong, setPong] = useState('…')
  useEffect(() => {
    invoke<string>('ping').then(setPong, (e) => setPong(String(e)))
  }, [])
  return <div className="p-8 text-2xl font-semibold">TidyFlow: {pong}</div>
}
