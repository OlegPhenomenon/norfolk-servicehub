import { useCallback, useMemo, useRef, useState, type ReactNode } from 'react'
import { isApiError } from '../api/client'
import { cn } from './cn'
import { Icon, type IconName } from './Icon'
import { ToastContext, type ToastApi, type ToastTone } from './toastContext'

interface ToastItem {
  id: number
  tone: ToastTone
  title?: string
  message: string
}

const STYLE: Record<ToastTone, { icon: IconName; color: string }> = {
  success: { icon: 'checkCircle', color: 'text-pine-300' },
  info: { icon: 'info', color: 'text-primary-200' },
  warning: { icon: 'alert', color: 'text-focus' },
  danger: { icon: 'xCircle', color: 'text-[#ff9b8f]' },
}

/** Mount once near the root (done in App.tsx). Use `useToast()` to show messages. */
export function ToastProvider({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<ToastItem[]>([])
  const nextId = useRef(1)

  const dismiss = useCallback((id: number) => setItems((list) => list.filter((t) => t.id !== id)), [])

  const show = useCallback<ToastApi['show']>(
    (message, opts = {}) => {
      const id = nextId.current++
      const tone = opts.tone ?? 'info'
      setItems((list) => [...list.slice(-3), { id, tone, title: opts.title, message }])
      window.setTimeout(() => dismiss(id), opts.durationMs ?? (tone === 'danger' ? 8000 : 5000))
      return id
    },
    [dismiss],
  )

  const api = useMemo<ToastApi>(
    () => ({
      show,
      dismiss,
      success: (message, title) => show(message, { tone: 'success', title }),
      info: (message, title) => show(message, { tone: 'info', title }),
      error: (error, title) => {
        const message = typeof error === 'string' ? error : isApiError(error) || error instanceof Error ? error.message : 'Something went wrong.'
        return show(message, { tone: 'danger', title })
      },
    }),
    [show, dismiss],
  )

  return (
    <ToastContext.Provider value={api}>
      {children}
      <div aria-live="polite" aria-atomic="false" className="pointer-events-none fixed inset-x-0 bottom-0 z-50 flex flex-col items-center gap-2 p-4 sm:items-end sm:p-6">
        {items.map((t) => (
          <div
            key={t.id}
            role={t.tone === 'danger' ? 'alert' : 'status'}
            className="pointer-events-auto flex w-full max-w-sm items-start gap-3 rounded-xl bg-primary-900 px-4 py-3 text-white shadow-[var(--shadow-raised)] [animation:toast-in_180ms_ease-out]"
          >
            <Icon name={STYLE[t.tone].icon} size={20} className={cn('mt-0.5', STYLE[t.tone].color)} />
            <div className="min-w-0 flex-1 text-[0.95rem]">
              {t.title ? <p className="font-semibold">{t.title}</p> : null}
              <p className="text-white/90">{t.message}</p>
            </div>
            <button type="button" onClick={() => dismiss(t.id)} className="-m-1 flex size-8 shrink-0 items-center justify-center rounded-md text-white/70 hover:bg-white/10 hover:text-white">
              <Icon name="x" size={16} title="Dismiss" />
            </button>
          </div>
        ))}
      </div>
    </ToastContext.Provider>
  )
}
