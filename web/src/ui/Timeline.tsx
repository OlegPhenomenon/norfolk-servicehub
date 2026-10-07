import type { ReactNode } from 'react'
import type { Timestamp } from '../api/types'
import type { Tone } from './Badge'
import { cn } from './cn'
import { DateTime } from './DateTime'
import { Icon, type IconName } from './Icon'

export interface TimelineItem {
  id: string | number
  at: Timestamp
  title: ReactNode
  /** Longer text under the title. */
  body?: ReactNode
  /** "Olga Novak (Customer Care)". */
  actor?: ReactNode
  tone?: Tone
  icon?: IconName
}

const MARKER: Record<Tone, string> = {
  neutral: 'bg-surface text-muted ring-line-strong',
  info: 'bg-info-50 text-info ring-info/30',
  primary: 'bg-primary-50 text-primary ring-primary/30',
  success: 'bg-success-50 text-success ring-success/30',
  warning: 'bg-warning-50 text-warning ring-warning-line',
  danger: 'bg-danger-50 text-danger ring-danger/30',
  accent: 'bg-pine-50 text-pine ring-pine/30',
}

/**
 * Vertical history of events (case events, messages, audit). Pass items in display order.
 *   <Timeline items={events.map((e) => ({ id: e.id, at: e.created_at, title: e.summary, actor: e.actor_name }))} />
 */
export function Timeline({ items, className }: { items: TimelineItem[]; className?: string }) {
  return (
    <ol className={cn('relative', className)}>
      {items.map((item, i) => (
        <li key={item.id} className="relative flex gap-4 pb-6 last:pb-0">
          {i < items.length - 1 ? <span aria-hidden="true" className="absolute left-[15px] top-8 bottom-0 w-px bg-line" /> : null}
          <span className={cn('relative z-10 mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-full ring-1', MARKER[item.tone ?? 'neutral'])}>
            {item.icon ? <Icon name={item.icon} size={16} /> : <span className="size-2 rounded-full bg-current" />}
          </span>
          <div className="min-w-0 flex-1 pt-1">
            <div className="flex flex-wrap items-baseline justify-between gap-x-3">
              <p className="font-semibold leading-snug">{item.title}</p>
              <DateTime value={item.at} className="text-sm text-muted whitespace-nowrap" />
            </div>
            {item.actor ? <p className="text-sm text-muted">{item.actor}</p> : null}
            {item.body ? <div className="mt-1.5 text-[0.95rem] text-ink/90">{item.body}</div> : null}
          </div>
        </li>
      ))}
    </ol>
  )
}
