import { useEffect, useId, useRef, useState, type ReactNode } from 'react'
import { Button } from './Button'
import { cn } from './cn'
import { ErrorAlert } from './Alert'
import { Icon } from './Icon'

export interface DialogProps {
  open: boolean
  /** Called on Escape, the close button, or a backdrop click. */
  onClose: () => void
  title: ReactNode
  description?: ReactNode
  /** Action buttons, right-aligned at the bottom. */
  footer?: ReactNode
  size?: 'sm' | 'md' | 'lg'
  children?: ReactNode
}

const WIDTHS = { sm: 'max-w-md', md: 'max-w-xl', lg: 'max-w-3xl' } as const

/**
 * Modal dialog built on native `<dialog>` (focus trapping, Escape and top-layer come from the browser).
 *
 *   const [open, setOpen] = useState(false)
 *   <Dialog open={open} onClose={() => setOpen(false)} title="Request more information"
 *     footer={<><Button variant="secondary" onClick={() => setOpen(false)}>Cancel</Button><Button onClick={send}>Send</Button></>}>
 *     …form fields…
 *   </Dialog>
 */
export function Dialog({ open, onClose, title, description, footer, size = 'md', children }: DialogProps) {
  const ref = useRef<HTMLDialogElement>(null)
  const titleId = useId()
  const descId = useId()

  useEffect(() => {
    const el = ref.current
    if (!el) return
    if (open && !el.open) el.showModal()
    if (!open && el.open) el.close()
  }, [open])

  return (
    <dialog
      ref={ref}
      aria-labelledby={titleId}
      aria-describedby={description ? descId : undefined}
      onCancel={(e) => {
        e.preventDefault()
        onClose()
      }}
      onClick={(e) => {
        if (e.target === ref.current) onClose()
      }}
      className={cn('m-auto w-[calc(100%-2rem)] rounded-2xl border border-line bg-surface p-0 text-ink shadow-[var(--shadow-raised)]', WIDTHS[size])}
    >
      {open ? (
        <div className="flex max-h-[85vh] flex-col">
          <header className="flex items-start justify-between gap-4 border-b border-line px-6 py-4">
            <div>
              <h2 id={titleId} className="text-xl font-semibold leading-snug">
                {title}
              </h2>
              {description ? (
                <p id={descId} className="mt-1 text-muted">
                  {description}
                </p>
              ) : null}
            </div>
            <button type="button" onClick={onClose} className="-mr-2 flex size-10 shrink-0 items-center justify-center rounded-lg text-muted hover:bg-sunken">
              <Icon name="x" title="Close" />
            </button>
          </header>
          <div className="overflow-y-auto px-6 py-5">{children}</div>
          {footer ? <footer className="flex flex-wrap justify-end gap-3 border-t border-line bg-sunken/40 px-6 py-4">{footer}</footer> : null}
        </div>
      ) : null}
    </dialog>
  )
}

export interface ConfirmDialogProps {
  open: boolean
  onClose: () => void
  title: ReactNode
  children?: ReactNode
  confirmLabel?: string
  cancelLabel?: string
  tone?: 'primary' | 'danger'
  /** May be async; the confirm button shows a spinner and errors are displayed in the dialog. */
  onConfirm: () => void | Promise<void>
}

/** Yes/no confirmation: <ConfirmDialog open={o} onClose={…} title="Withdraw this request?" tone="danger" confirmLabel="Withdraw" onConfirm={withdraw} /> */
export function ConfirmDialog({ open, onClose, title, children, confirmLabel = 'Confirm', cancelLabel = 'Cancel', tone = 'primary', onConfirm }: ConfirmDialogProps) {
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<unknown>(null)
  const confirm = async () => {
    setBusy(true)
    setError(null)
    try {
      await onConfirm()
      onClose()
    } catch (e) {
      setError(e)
    } finally {
      setBusy(false)
    }
  }
  return (
    <Dialog
      open={open}
      onClose={onClose}
      title={title}
      size="sm"
      footer={
        <>
          <Button variant="secondary" onClick={onClose} disabled={busy}>
            {cancelLabel}
          </Button>
          <Button variant={tone === 'danger' ? 'danger' : 'primary'} loading={busy} onClick={() => void confirm()}>
            {confirmLabel}
          </Button>
        </>
      }
    >
      {children ? <div className="text-ink/90">{children}</div> : null}
      {error ? <ErrorAlert error={error} className="mt-4" /> : null}
    </Dialog>
  )
}
