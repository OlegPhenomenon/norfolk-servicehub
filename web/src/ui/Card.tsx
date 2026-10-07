import type { ElementType, ReactNode } from 'react'
import { cn } from './cn'

export interface CardProps {
  /** Card heading (rendered as h2 by default; set `headingLevel` to fit the page outline). */
  title?: ReactNode
  headingLevel?: 2 | 3 | 4
  description?: ReactNode
  /** Buttons/links shown at the right of the header. */
  actions?: ReactNode
  /** Footer row, e.g. form buttons. */
  footer?: ReactNode
  /** `false` removes body padding (for tables/lists that touch the edges). */
  padded?: boolean
  as?: ElementType
  className?: string
  children?: ReactNode
}

/**
 * White surface with border and soft shadow — the basic building block of every page.
 *   <Card title="Applicant" actions={<Button size="sm" variant="ghost">Edit</Button>}>…</Card>
 */
export function Card({ title, headingLevel = 2, description, actions, footer, padded = true, as: As = 'section', className, children }: CardProps) {
  const Heading = `h${headingLevel}` as const
  const hasHeader = title || description || actions
  return (
    <As className={cn('rounded-[var(--radius-card)] border border-line bg-surface shadow-[var(--shadow-card)]', className)}>
      {hasHeader ? (
        <header className="flex flex-wrap items-start justify-between gap-x-4 gap-y-2 border-b border-line px-5 py-4 sm:px-6">
          <div className="min-w-0">
            {title ? <Heading className="text-lg font-semibold leading-snug">{title}</Heading> : null}
            {description ? <p className="mt-0.5 text-sm text-muted">{description}</p> : null}
          </div>
          {actions ? <div className="flex shrink-0 flex-wrap items-center gap-2">{actions}</div> : null}
        </header>
      ) : null}
      <div className={cn(padded && 'px-5 py-5 sm:px-6')}>{children}</div>
      {footer ? <footer className="flex flex-wrap items-center justify-end gap-3 border-t border-line bg-sunken/40 px-5 py-4 sm:px-6 rounded-b-[var(--radius-card)]">{footer}</footer> : null}
    </As>
  )
}

export interface DescriptionListProps {
  items: Array<{ label: ReactNode; value: ReactNode; key?: string }>
  /** Two columns of label/value pairs on wide screens. */
  columns?: 1 | 2
  className?: string
}

/** Label/value pairs (case details, applicant info): <DescriptionList items={[{ label: 'Number', value: c.number }]} /> */
export function DescriptionList({ items, columns = 1, className }: DescriptionListProps) {
  return (
    <dl className={cn('grid gap-x-8 gap-y-4', columns === 2 && 'sm:grid-cols-2', className)}>
      {items.map((item, i) => (
        <div key={item.key ?? i} className="min-w-0">
          <dt className="text-sm font-medium text-muted">{item.label}</dt>
          <dd className="mt-0.5 break-words text-ink">{item.value ?? <span className="text-subtle">—</span>}</dd>
        </div>
      ))}
    </dl>
  )
}
