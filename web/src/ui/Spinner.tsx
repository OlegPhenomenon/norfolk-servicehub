import { cn } from './cn'

export interface SpinnerProps {
  size?: number
  /** Screen-reader text; `null` for spinners inside labelled controls (e.g. a loading Button). */
  label?: string | null
  className?: string
}

/** <Spinner /> — inline loading indicator. */
export function Spinner({ size = 20, label = 'Loading', className }: SpinnerProps) {
  return (
    <span role={label ? 'status' : undefined} className={cn('inline-flex items-center', className)}>
      <svg viewBox="0 0 24 24" width={size} height={size} className="animate-spin" aria-hidden="true">
        <circle cx="12" cy="12" r="9.5" fill="none" stroke="currentColor" strokeOpacity="0.2" strokeWidth="3" />
        <path d="M21.5 12A9.5 9.5 0 0 0 12 2.5" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" />
      </svg>
      {label ? <span className="sr-only">{label}</span> : null}
    </span>
  )
}

/** Centered loading block for a page or card body: <LoadingState label="Loading cases…" /> */
export function LoadingState({ label = 'Loading…', className }: { label?: string; className?: string }) {
  return (
    <div role="status" className={cn('flex items-center justify-center gap-3 py-12 text-muted', className)}>
      <Spinner label={null} size={22} className="text-primary" />
      <span>{label}</span>
    </div>
  )
}
