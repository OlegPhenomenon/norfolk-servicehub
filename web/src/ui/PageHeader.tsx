import type { ReactNode } from 'react'
import { Link } from 'react-router'
import { cn } from './cn'
import { Icon } from './Icon'

export interface Crumb {
  label: string
  /** Omit for the current page. */
  to?: string
}

export interface PageHeaderProps {
  title: ReactNode
  /** Small label above the title, e.g. the case number or area name. */
  eyebrow?: ReactNode
  description?: ReactNode
  breadcrumbs?: Crumb[]
  /** Primary page actions (right side on desktop, below on mobile). */
  actions?: ReactNode
  /** Badges/metadata row under the title. */
  meta?: ReactNode
  className?: string
}

/**
 * Top of every page: the only <h1> on the page.
 *   <PageHeader eyebrow="NSH-2026-000123" title="Hire of Rawson Hall" meta={<StatusPill status="in_progress" />}
 *     breadcrumbs={[{ label: 'My requests', to: '/my' }, { label: 'Hire of Rawson Hall' }]} actions={<Button>Pay now</Button>} />
 */
export function PageHeader({ title, eyebrow, description, breadcrumbs, actions, meta, className }: PageHeaderProps) {
  return (
    <div className={cn('mb-8', className)}>
      {breadcrumbs && breadcrumbs.length > 0 ? (
        <nav aria-label="Breadcrumb" className="mb-3">
          <ol className="flex flex-wrap items-center gap-1 text-sm text-muted">
            {breadcrumbs.map((c, i) => (
              <li key={`${c.label}-${i}`} className="flex items-center gap-1">
                {i > 0 ? <Icon name="chevronRight" size={14} className="text-subtle" /> : null}
                {c.to ? (
                  <Link to={c.to} className="underline-offset-2 hover:text-primary hover:underline">
                    {c.label}
                  </Link>
                ) : (
                  <span aria-current="page">{c.label}</span>
                )}
              </li>
            ))}
          </ol>
        </nav>
      ) : null}
      <div className="flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between">
        <div className="min-w-0">
          {eyebrow ? <p className="mb-1 text-sm font-semibold tracking-wide text-pine uppercase">{eyebrow}</p> : null}
          <h1 className="font-serif text-[1.875rem] leading-tight font-semibold tracking-[-0.01em] sm:text-4xl">{title}</h1>
          {meta ? <div className="mt-3 flex flex-wrap items-center gap-2">{meta}</div> : null}
          {description ? <p className="mt-2 max-w-2xl text-lg text-muted">{description}</p> : null}
        </div>
        {actions ? <div className="flex shrink-0 flex-wrap gap-2">{actions}</div> : null}
      </div>
    </div>
  )
}
