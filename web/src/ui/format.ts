import type { Cents, LocalDate, Timestamp } from '../api/types'

/** All dates and times are shown in Norfolk Island time. */
export const NORFOLK_TZ = 'Pacific/Norfolk'

const AUD = new Intl.NumberFormat('en-AU', { style: 'currency', currency: 'AUD' })

/** 12345 → "$123.45"; -500 → "-$5.00". */
export function formatMoney(cents: Cents): string {
  return AUD.format(cents / 100)
}

export type DateFormat = 'date' | 'datetime' | 'time' | 'long' | 'short'

const FORMATS: Record<DateFormat, Intl.DateTimeFormatOptions> = {
  date: { day: 'numeric', month: 'short', year: 'numeric' }, // 7 Oct 2026
  datetime: { day: 'numeric', month: 'short', year: 'numeric', hour: 'numeric', minute: '2-digit' }, // 7 Oct 2026, 3:15 pm
  time: { hour: 'numeric', minute: '2-digit' }, // 3:15 pm
  long: { weekday: 'long', day: 'numeric', month: 'long', year: 'numeric' }, // Wednesday 7 October 2026
  short: { day: 'numeric', month: 'short' }, // 7 Oct
}

const formatters = new Map<string, Intl.DateTimeFormat>()

function formatter(format: DateFormat, timeZone: string): Intl.DateTimeFormat {
  const key = `${format}|${timeZone}`
  let f = formatters.get(key)
  if (!f) {
    f = new Intl.DateTimeFormat('en-AU', { ...FORMATS[format], timeZone })
    formatters.set(key, f)
  }
  return f
}

const LOCAL_DATE = /^\d{4}-\d{2}-\d{2}$/

/**
 * Format a UTC timestamp (shown in Pacific/Norfolk) or a local date `YYYY-MM-DD` (shown as-is, no shifting).
 * Returns '' for null/invalid input.
 */
export function formatDateTime(value: Timestamp | LocalDate | null | undefined, format: DateFormat = 'datetime'): string {
  if (!value) return ''
  if (LOCAL_DATE.test(value)) {
    const d = new Date(`${value}T12:00:00Z`)
    return Number.isNaN(d.getTime()) ? '' : formatter(format === 'datetime' || format === 'time' ? 'date' : format, 'UTC').format(d)
  }
  const d = new Date(value)
  return Number.isNaN(d.getTime()) ? '' : formatter(format, NORFOLK_TZ).format(d)
}

const RELATIVE = new Intl.RelativeTimeFormat('en-AU', { numeric: 'auto' })

/** "in 3 days", "2 hours ago", "yesterday". */
export function formatRelative(value: Timestamp, now: Date = new Date()): string {
  const diffSec = (new Date(value).getTime() - now.getTime()) / 1000
  const abs = Math.abs(diffSec)
  if (abs < 45) return 'just now'
  if (abs < 3600) return RELATIVE.format(Math.round(diffSec / 60), 'minute')
  if (abs < 86400) return RELATIVE.format(Math.round(diffSec / 3600), 'hour')
  if (abs < 86400 * 30) return RELATIVE.format(Math.round(diffSec / 86400), 'day')
  return formatDateTime(value, 'date')
}

/** Today's date in Norfolk Island as `YYYY-MM-DD`. */
export function norfolkToday(now: Date = new Date()): LocalDate {
  // en-CA formats as YYYY-MM-DD.
  return new Intl.DateTimeFormat('en-CA', { timeZone: NORFOLK_TZ, year: 'numeric', month: '2-digit', day: '2-digit' }).format(now)
}
