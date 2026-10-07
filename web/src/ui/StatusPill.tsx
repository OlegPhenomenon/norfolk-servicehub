import { Badge, type Tone } from './Badge'
import { STATUS_STYLES, humanize } from './status'

export interface StatusPillProps {
  /** Raw status value from the API, e.g. `waiting_on_applicant`, `paid`, `breached`. */
  status: string
  /** Override the label (e.g. applicant-facing wording). */
  label?: string
  tone?: Tone
  className?: string
}

/** <StatusPill status={c.status} /> — consistent colour + wording for any status in the schema. */
export function StatusPill({ status, label, tone, className }: StatusPillProps) {
  const style = STATUS_STYLES[status]
  return (
    <Badge dot tone={tone ?? style?.tone ?? 'neutral'} className={className}>
      {label ?? style?.label ?? humanize(status)}
    </Badge>
  )
}
