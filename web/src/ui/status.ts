import type { Tone } from './Badge'

/**
 * Labels and tones for status values used across the schema. Feature slices may rely on these;
 * unknown statuses fall back to a humanised label with a neutral tone.
 */
export const STATUS_STYLES: Record<string, { label: string; tone: Tone }> = {
  // cases
  draft: { label: 'Draft', tone: 'neutral' },
  submitted: { label: 'Submitted', tone: 'info' },
  in_progress: { label: 'In progress', tone: 'primary' },
  waiting_on_applicant: { label: 'Waiting on applicant', tone: 'warning' },
  completed: { label: 'Completed', tone: 'success' },
  refused: { label: 'Refused', tone: 'danger' },
  withdrawn: { label: 'Withdrawn', tone: 'neutral' },
  cancelled: { label: 'Cancelled', tone: 'neutral' },
  closed_duplicate: { label: 'Closed (duplicate)', tone: 'neutral' },
  // tasks / bookings / exhibitions / deadlines / payments / outbox
  open: { label: 'Open', tone: 'info' },
  requested: { label: 'Requested', tone: 'info' },
  confirmed: { label: 'Confirmed', tone: 'success' },
  done: { label: 'Done', tone: 'success' },
  running: { label: 'Running', tone: 'primary' },
  paused: { label: 'Paused', tone: 'warning' },
  met: { label: 'Met', tone: 'success' },
  breached: { label: 'Overdue', tone: 'danger' },
  pending: { label: 'Pending', tone: 'neutral' },
  pending_approval: { label: 'Pending approval', tone: 'warning' },
  issued: { label: 'Issued', tone: 'success' },
  returned: { label: 'Returned', tone: 'warning' },
  closed: { label: 'Closed', tone: 'neutral' },
  void: { label: 'Void', tone: 'neutral' },
  paid: { label: 'Paid', tone: 'success' },
  unpaid: { label: 'Unpaid', tone: 'warning' },
  expired: { label: 'Expired', tone: 'neutral' },
  failed: { label: 'Failed', tone: 'danger' },
  dead: { label: 'Failed permanently', tone: 'danger' },
  processing: { label: 'Processing', tone: 'primary' },
  queued: { label: 'Queued', tone: 'neutral' },
  sending: { label: 'Sending', tone: 'primary' },
  sent: { label: 'Sent', tone: 'success' },
  accepted: { label: 'Accepted', tone: 'success' },
  read: { label: 'Read', tone: 'neutral' },
  matched: { label: 'Matched', tone: 'success' },
  unmatched: { label: 'Unmatched', tone: 'warning' },
  ignored: { label: 'Ignored', tone: 'neutral' },
  duplicate: { label: 'Duplicate', tone: 'neutral' },
  reversed: { label: 'Reversed', tone: 'neutral' },
  active: { label: 'Active', tone: 'success' },
  revoked: { label: 'Revoked', tone: 'neutral' },
  invited: { label: 'Invited', tone: 'info' },
  published: { label: 'Published', tone: 'success' },
  retired: { label: 'Retired', tone: 'neutral' },
}

/** `waiting_on_applicant` → `Waiting on applicant` */
export function humanize(value: string): string {
  const s = value.replace(/[_-]+/g, ' ').trim()
  return s.charAt(0).toUpperCase() + s.slice(1)
}
