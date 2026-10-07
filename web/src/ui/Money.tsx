import type { Cents } from '../api/types'
import { cn } from './cn'
import { formatMoney } from './format'

export interface MoneyProps {
  /** Integer cents, AUD. */
  cents: Cents
  /** Colour negatives (credits/refunds) green. */
  signed?: boolean
  className?: string
}

/** <Money cents={12500} /> → "$125.00" with tabular digits. */
export function Money({ cents, signed, className }: MoneyProps) {
  return <span className={cn('tabular-nums whitespace-nowrap', signed && cents < 0 && 'text-success', className)}>{formatMoney(cents)}</span>
}
