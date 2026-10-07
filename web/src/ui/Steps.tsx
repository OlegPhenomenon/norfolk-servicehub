import type { ReactNode } from 'react'
import { cn } from './cn'
import { Icon } from './Icon'

export type StepState = 'complete' | 'current' | 'upcoming' | 'skipped' | 'failed'

export interface StepItem {
  key: string
  label: ReactNode
  description?: ReactNode
  /** Force a state; otherwise derived from `current`. */
  state?: StepState
}

export interface StepsProps {
  steps: StepItem[]
  /** Key of the current step. Earlier steps are complete, later ones upcoming. */
  current?: string | null
  /** The whole process finished: every step shows as complete. */
  finished?: boolean
  orientation?: 'vertical' | 'horizontal'
  /** Accessible name, e.g. "Progress of your request". */
  label?: string
  className?: string
}

const STATE_TEXT: Record<StepState, string> = {
  complete: 'Completed',
  current: 'Current step',
  upcoming: 'Not started',
  skipped: 'Skipped',
  failed: 'Stopped',
}

/**
 * Progress through a workflow (applicant view of case steps, multi-page forms).
 *   <Steps label="Progress" current={c.current_step} steps={def.workflow.steps.map((s) => ({ key: s.key, label: s.applicant_label }))} />
 */
export function Steps({ steps, current, finished, orientation = 'vertical', label = 'Progress', className }: StepsProps) {
  const currentIndex = finished ? steps.length : steps.findIndex((s) => s.key === current)
  const horizontal = orientation === 'horizontal'
  return (
    <ol aria-label={label} className={cn(horizontal ? 'flex flex-col gap-3 sm:flex-row sm:gap-0' : 'flex flex-col', className)}>
      {steps.map((step, i) => {
        const state: StepState = step.state ?? (i < currentIndex ? 'complete' : i === currentIndex ? 'current' : 'upcoming')
        const last = i === steps.length - 1
        return (
          <li key={step.key} aria-current={state === 'current' ? 'step' : undefined} className={cn('relative flex gap-3', horizontal ? 'sm:flex-1 sm:flex-col sm:items-start sm:pr-4' : 'pb-5 last:pb-0')}>
            {!last ? (
              <span
                aria-hidden="true"
                className={cn(
                  'absolute',
                  horizontal ? 'hidden sm:block sm:left-9 sm:right-2 sm:top-[15px] sm:h-0.5' : 'left-[15px] top-8 bottom-0 w-0.5',
                  state === 'complete' ? 'bg-pine' : 'bg-line',
                )}
              />
            ) : null}
            <span
              className={cn(
                'relative z-10 flex size-8 shrink-0 items-center justify-center rounded-full text-sm font-bold',
                state === 'complete' && 'bg-pine text-white',
                state === 'current' && 'bg-primary text-white ring-4 ring-primary-100',
                state === 'upcoming' && 'bg-surface text-muted ring-2 ring-inset ring-line-strong',
                state === 'skipped' && 'bg-sunken text-muted',
                state === 'failed' && 'bg-danger text-white',
              )}
            >
              {state === 'complete' ? <Icon name="check" size={16} /> : state === 'failed' ? <Icon name="x" size={16} /> : i + 1}
            </span>
            <div className={cn('min-w-0', horizontal ? 'pt-1 sm:pt-2' : 'pt-1')}>
              <p className={cn('leading-snug', state === 'current' ? 'font-semibold text-ink' : state === 'upcoming' ? 'text-muted' : 'text-ink', state === 'skipped' && 'line-through')}>
                {step.label}
              </p>
              <span className="sr-only">({STATE_TEXT[state]})</span>
              {step.description ? <p className="mt-0.5 text-sm text-muted">{step.description}</p> : null}
            </div>
          </li>
        )
      })}
    </ol>
  )
}
