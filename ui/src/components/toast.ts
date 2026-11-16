import { createContext, useContext } from 'react'

export type Tone = 'ok' | 'error'

export const ToastCtx = createContext<(text: string, tone?: Tone) => void>(() => {})

/** `toast("texto")` ou `toast("texto", "error")`. */
export const useToast = () => useContext(ToastCtx)
