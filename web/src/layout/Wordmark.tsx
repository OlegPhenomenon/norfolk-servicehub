import { Link } from 'react-router'
import { cn } from '@/ui'

/**
 * Norfolk Island pine: a straight trunk with tiers of slightly drooping horizontal branches.
 * Drawn for this demo — deliberately not the council's logo.
 */
export function PineGlyph({ size = 32, className }: { size?: number; className?: string }) {
  const tiers = [
    { y: 7, w: 4 },
    { y: 10.5, w: 8 },
    { y: 14, w: 12 },
    { y: 17.5, w: 16 },
    { y: 21, w: 20 },
    { y: 24.5, w: 24 },
  ]
  return (
    <svg viewBox="0 0 28 32" width={size * 0.875} height={size} className={className} aria-hidden="true" focusable="false">
      <path d="M14 1.5v28" stroke="currentColor" strokeOpacity="0.75" strokeWidth="1.6" strokeLinecap="round" />
      <path d="M14 2.5 12.6 6h2.8z" fill="currentColor" />
      {tiers.map(({ y, w }) => (
        <path
          key={y}
          d={`M${14 - w / 2} ${y + 1.4}Q14 ${y - 1.2} ${14 + w / 2} ${y + 1.4}`}
          fill="none"
          stroke="currentColor"
          strokeWidth="2.1"
          strokeLinecap="round"
        />
      ))}
      <path d="M8 30.25h12" stroke="currentColor" strokeOpacity="0.55" strokeWidth="1.4" strokeLinecap="round" />
    </svg>
  )
}

/** "Norfolk ServiceHub" wordmark linking home. `inverted` for dark backgrounds. */
export function Wordmark({ to = '/', subtitle, inverted, className }: { to?: string; subtitle?: string; inverted?: boolean; className?: string }) {
  return (
    <Link to={to} className={cn('group inline-flex min-w-0 items-center gap-2.5 rounded-md py-1', className)}>
      <PineGlyph size={30} className={cn('shrink-0', inverted ? 'text-pine-300' : 'text-pine')} />
      <span className="flex min-w-0 flex-col leading-none">
        <span className={cn('text-base sm:text-[1.2rem] tracking-[-0.01em]', inverted ? 'text-white' : 'text-ink')}>
          <span className="font-serif font-semibold">Norfolk</span> <span className={cn('block sm:inline font-semibold', inverted ? 'text-primary-100' : 'text-primary')}>ServiceHub</span>
        </span>
        {subtitle ? <span className={cn('mt-1 text-xs font-medium tracking-wide uppercase', inverted ? 'text-white/70' : 'text-muted')}>{subtitle}</span> : null}
      </span>
    </Link>
  )
}
