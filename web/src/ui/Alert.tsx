import type { ReactNode } from 'react'
import { isApiError } from '../api/client'
import { Button } from './Button'
import { cn } from './cn'
import { Icon, type IconName } from './Icon'

export type AlertTone = 'info' | 'success' | 'warning' | 'danger'

const STYLES: Record<AlertTone, { box: string; icon: IconName; iconColor: string }> = {
  info: { box: 'bg-info-50 border-info/30', icon: 'info', iconColor: 'text-info' },
  success: { box: 'bg-success-50 border-success/30', icon: 'checkCircle', iconColor: 'text-success' },
  warning: { box: 'bg-warning-50 border-warning-line', icon: 'alert', iconColor: 'text-warning' },
  danger: { box: 'bg-danger-50 border-danger/30', icon: 'xCircle', iconColor: 'text-danger' },
}

export interface AlertProps {
  tone?: AlertTone
  title?: ReactNode
  children?: ReactNode
  /** Buttons/links under the text. */
  actions?: ReactNode
  onDismiss?: () => void
  className?: string
}

/**
 * Inline message box. `danger`/`warning` are announced immediately (role="alert").
 *   <Alert tone="warning" title="Payment required">Pay the hire fee to confirm your booking.</Alert>
 */
export function Alert({ tone = 'info', title, children, actions, onDismiss, className }: AlertProps) {
  const s = STYLES[tone]
  return (
    <div role={tone === 'danger' || tone === 'warning' ? 'alert' : 'status'} className={cn('flex gap-3 rounded-xl border px-4 py-3.5', s.box, className)}>
      <Icon name={s.icon} size={22} className={cn('mt-0.5', s.iconColor)} />
      <div className="min-w-0 flex-1">
        {title ? <p className="font-semibold leading-snug">{title}</p> : null}
        {children ? <div className={cn('text-[0.95rem] text-ink/90', title ? 'mt-0.5' : null)}>{children}</div> : null}
        {actions ? <div className="mt-3 flex flex-wrap gap-2">{actions}</div> : null}
      </div>
      {onDismiss ? (
        <button type="button" onClick={onDismiss} className="-m-1.5 flex size-9 shrink-0 items-center justify-center rounded-md text-muted hover:bg-black/5">
          <Icon name="x" size={18} title="Dismiss" />
        </button>
      ) : null}
    </div>
  )
}

/**
 * Shows any error (usually an `ApiError` from a query/mutation) as a danger Alert.
 *   {mutation.error ? <ErrorAlert error={mutation.error} /> : null}
 *   <ErrorAlert error={query.error} onRetry={() => query.refetch()} />
 */
export function ErrorAlert({ error, title = 'Something went wrong', onRetry, className }: { error: unknown; title?: string; onRetry?: () => void; className?: string }) {
  if (!error) return null
  const message = isApiError(error) ? error.message : error instanceof Error ? error.message : 'Unexpected error.'
  const fieldMessages = isApiError(error) ? Object.values(error.fields) : []
  return (
    <Alert
      tone="danger"
      title={isApiError(error, 'not_found') ? 'Not found' : title}
      className={className}
      actions={onRetry ? <Button size="sm" variant="secondary" icon="clock" onClick={onRetry}>Try again</Button> : undefined}
    >
      <p>{message}</p>
      {fieldMessages.length > 0 ? (
        <ul className="mt-1 list-disc pl-5">
          {fieldMessages.map((m) => (
            <li key={m}>{m}</li>
          ))}
        </ul>
      ) : null}
    </Alert>
  )
}
