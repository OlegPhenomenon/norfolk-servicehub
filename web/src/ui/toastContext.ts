import { createContext, useContext } from 'react'

export type ToastTone = 'success' | 'info' | 'warning' | 'danger'

export interface ToastApi {
  /** Show a toast; returns its id. */
  show: (message: string, opts?: { tone?: ToastTone; title?: string; durationMs?: number }) => number
  success: (message: string, title?: string) => number
  info: (message: string, title?: string) => number
  /** Accepts a string or any error (ApiError messages are shown as-is). */
  error: (error: unknown, title?: string) => number
  dismiss: (id: number) => void
}

export const ToastContext = createContext<ToastApi | null>(null)

/**
 * Brief, non-blocking confirmation after an action. Requires <ToastProvider> (already in App).
 *   const toast = useToast()
 *   onSuccess: () => toast.success('Message sent')
 *   onError: (e) => toast.error(e)
 */
export function useToast(): ToastApi {
  const ctx = useContext(ToastContext)
  if (!ctx) throw new Error('useToast must be used inside <ToastProvider>')
  return ctx
}
