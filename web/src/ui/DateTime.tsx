import type { LocalDate, Timestamp } from '../api/types'
import { formatDateTime, formatRelative, type DateFormat } from './format'

export interface DateTimeProps {
  value: Timestamp | LocalDate | null | undefined
  /** `date` 7 Oct 2026 · `datetime` 7 Oct 2026, 3:15 pm · `time` · `long` · `short` · `relative` (2 hours ago). */
  format?: DateFormat | 'relative'
  /** Shown when value is empty. */
  fallback?: string
  className?: string
}

/**
 * Renders a semantic `<time>` in Pacific/Norfolk time. Relative format shows the absolute time as a tooltip.
 *   <DateTime value={c.submitted_at} format="date" />
 */
export function DateTime({ value, format = 'datetime', fallback = '—', className }: DateTimeProps) {
  if (!value) return <span className={className}>{fallback}</span>
  const absolute = formatDateTime(value, format === 'relative' ? 'datetime' : format)
  const text = format === 'relative' ? formatRelative(value) : absolute
  return (
    <time dateTime={value} title={format === 'relative' ? `${absolute} (Norfolk Island time)` : undefined} className={className}>
      {text}
    </time>
  )
}
