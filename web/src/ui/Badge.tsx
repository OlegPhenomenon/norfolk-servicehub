import type { ReactNode } from 'react'
import { cn } from './cn'
import { Icon, type IconName } from './Icon'

export type Tone = 'neutral' | 'info' | 'success' | 'warning' | 'danger' | 'accent' | 'primary'

const TONES: Record<Tone, string> = {
  neutral: 'bg-sunken text-ink/80 ring-line-strong/60',
  info: 'bg-info-50 text-info ring-info/25',
  success: 'bg-success-50 text-success ring-success/25',
  warning: 'bg-warning-50 text-warning ring-warning-line',
  danger: 'bg-danger-50 text-danger ring-danger/25',
  accent: 'bg-pine-50 text-pine-700 ring-pine/25',
  primary: 'bg-primary-50 text-primary ring-primary/20',
}

const DOTS: Record<Tone, string> = {
  neutral: 'bg-subtle',
  info: 'bg-info',
  success: 'bg-success',
  warning: 'bg-warning',
  danger: 'bg-danger',
  accent: 'bg-pine',
  primary: 'bg-primary',
}

export interface BadgeProps {
  tone?: Tone
  icon?: IconName
  /** Small coloured dot before the text (used by StatusPill). */
  dot?: boolean
  className?: string
  children: ReactNode
}

/** Small label: <Badge tone="accent">Online</Badge>, <Badge tone="warning" icon="lock">Confidential</Badge> */
export function Badge({ tone = 'neutral', icon, dot, className, children }: BadgeProps) {
  return (
    <span className={cn('inline-flex items-center gap-1.5 rounded-full px-2.5 py-0.5 text-[0.8125rem] font-semibold leading-5 whitespace-nowrap ring-1 ring-inset', TONES[tone], className)}>
      {dot ? <span aria-hidden="true" className={cn('size-1.5 rounded-full', DOTS[tone])} /> : null}
      {icon ? <Icon name={icon} size={14} /> : null}
      {children}
    </span>
  )
}
