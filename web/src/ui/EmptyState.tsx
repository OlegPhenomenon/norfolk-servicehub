import type { ReactNode } from 'react'
import { cn } from './cn'
import { Icon, type IconName } from './Icon'

export interface EmptyStateProps {
  title: ReactNode
  description?: ReactNode
  icon?: IconName
  /** Usually a Button/ButtonLink. */
  action?: ReactNode
  className?: string
}

/** Friendly "nothing here yet" block: <EmptyState icon="inbox" title="No requests yet" action={<ButtonLink to="/services">Find a service</ButtonLink>} /> */
export function EmptyState({ title, description, icon = 'inbox', action, className }: EmptyStateProps) {
  return (
    <div className={cn('flex flex-col items-center rounded-[var(--radius-card)] border border-dashed border-line-strong bg-surface/60 px-6 py-12 text-center', className)}>
      <span className="mb-4 flex size-12 items-center justify-center rounded-full bg-primary-50 text-primary">
        <Icon name={icon} size={24} />
      </span>
      <p className="text-lg font-semibold">{title}</p>
      {description ? <p className="mt-1 max-w-md text-muted">{description}</p> : null}
      {action ? <div className="mt-5">{action}</div> : null}
    </div>
  )
}
